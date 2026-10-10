//! Database-worker endpoints for coordinator messages. Owns cancellation and ports.

use std::rc::Weak;
use super::protocol::BridgeEvent;
use super::{byte_reply, error_message, field, object, BytesFuture, BytesOperation, Port};
use futures_channel::oneshot;
use js_sys::{Array, Uint8Array};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{DedicatedWorkerGlobalScope, MessageChannel, MessageEvent};

type Cancel = Box<dyn FnOnce(JsValue)>;
struct Bridge {
    scope: DedicatedWorkerGlobalScope,
    pending: super::pending::Pending<Cancel>,
}
thread_local! {static BRIDGE:RefCell<Option<Rc<Bridge>>>=const{RefCell::new(None)};}
fn bridge() -> Result<Rc<Bridge>, JsValue> {
    BRIDGE.with(|slot| {
        if let Some(bridge) = slot.borrow().as_ref() {
            return Ok(bridge.clone());
        }
        let bridge = Rc::new(Bridge { scope: js_sys::global().unchecked_into(), pending: Default::default() });
        let weak = Rc::downgrade(&bridge);
        let listener = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let data = event.data();
            if field(&data, "transport").as_string().as_deref() != Some("sync_failed") {
                return;
            }
            if let Some(bridge) = weak.upgrade() {
                let error: JsValue = js_sys::Error::new(&error_message(&field(&data, "error"))).into();
                bridge.pending.fail_all(|cancel| cancel(error.clone()));
            }
        })
        .into_js_value();
        bridge.scope.add_event_listener_with_callback("message", listener.unchecked_ref())?;
        *slot.borrow_mut() = Some(bridge.clone());
        Ok(bridge)
    })
}
type Reply = oneshot::Sender<Result<Vec<u8>, String>>;
struct Request {
    bridge: Rc<Bridge>,
    id: u64,
    port: RefCell<Option<Port>>,
    reply: RefCell<Option<Reply>>,
    cancelled: async_channel::Receiver<()>,
    run: BytesOperation,
}
impl Request {
    fn finish(&self, result: Result<Vec<u8>, String>) {
        self.cancelled.close();
        self.port.borrow_mut().take();
        self.bridge.pending.take(self.id);
        if let Some(reply) = self.reply.borrow_mut().take() {
            let _ = reply.send(result);
        }
    }
    fn receive(self: &Rc<Self>, data: JsValue) {
        match BridgeEvent::decode(&data) {
            Ok(BridgeEvent::Complete(result)) => self.finish(result),
            Ok(BridgeEvent::Storage { id, bytes }) => self.execute(id, bytes),
            Err(error) => self.finish(Err(error)),
        }
    }
    fn execute(self: &Rc<Self>, id: u32, bytes: Vec<u8>) {
        if self.cancelled.is_closed() {
            return;
        }
        let run = (self.run)(bytes);
        let state = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = tokio::select! {
                biased;
                _ = state.cancelled.recv() => return,
                result = run => result,
            };
            let (message, transfer) = byte_reply(&[("id", id.into())], result);
            let result = state.port.borrow().as_ref().map(|port| port.raw.post_message_with_transferable(&message, &transfer));
            if let Some(Err(error)) = result {
                state.finish(Err(error_message(&error)));
            }
        });
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        if let Some(port) = self.port.get_mut().as_ref() {
            let _ = port.raw.post_message(&object(&[("kind", "cancel".into())]));
        }
        self.bridge.pending.take(self.id);
    }
}
pub fn sync_request(key: String, bytes: Vec<u8>, run: BytesOperation) -> Result<BytesFuture, JsValue> {
    let bridge = bridge()?;
    let id = bridge.pending.next_id();
    let channel = MessageChannel::new()?;
    let (reply, response) = oneshot::channel();
    // Dropping the returned future closes this sender and cancels active work.
    // finish() closes the receiver to cancel work when a coordinator reply arrives.
    let (cancel, cancelled) = async_channel::unbounded();
    let message = object(&[("transport", "sync_request".into()), ("key", key.into()), ("port", channel.port2().into())]);
    let transfer = Array::of1(&channel.port2());
    let bytes = Uint8Array::from(bytes.as_slice());
    transfer.push(&bytes.buffer());
    js_sys::Reflect::set(&message, &"bytes".into(), &bytes)?;
    let state = Rc::new_cyclic(|weak: &Weak<Request>| {
        let message = weak.clone();
        let error = weak.clone();
        Request {
            bridge: bridge.clone(),
            id,
            port: RefCell::new(Some(Port::new(
                channel.port1(),
                move |event| {
                    if let Some(state) = message.upgrade() {
                        state.receive(event.data());
                    }
                },
                move |_| {
                    if let Some(state) = error.upgrade() {
                        state.finish(Err("Invalid coordinator message".into()));
                    }
                },
            ))),
            reply: RefCell::new(Some(reply)),
            cancelled,
            run,
        }
    });
    let weak = Rc::downgrade(&state);
    bridge.pending.insert(
        id,
        Box::new(move |error| {
            if let Some(state) = weak.upgrade() {
                state.finish(Err(error_message(&error)));
            }
        }),
    );
    if let Err(error) = bridge.scope.post_message_with_transfer(&message, &transfer) {
        channel.port2().close();
        state.finish(Err(error_message(&error)));
    }
    Ok(Box::pin(async move {
        let _state = state;
        let _cancel = cancel;
        response.await.unwrap_or_else(|_| Err("Coordinator request closed".into()))
    }))
}

struct Background {
    bridge: Rc<Bridge>,
    id: u64,
    port: RefCell<Option<Port>>,
    active: Cell<u32>,
    commands: async_channel::Receiver<bool>,
}
impl Background {
    fn stop(&self) {
        let Some(port) = self.port.borrow_mut().take() else { return };
        self.bridge.pending.take(self.id);
        self.commands.close();
        let _ = port.raw.post_message(&object(&[("kind", "stop".into())]));
    }
}
pub struct BackgroundHost(Rc<Background>);
impl Drop for BackgroundHost {
    fn drop(&mut self) {
        self.0.stop();
    }
}
impl BackgroundHost {
    pub fn new(retry: f64, debounce: f64) -> Result<Self, JsValue> {
        let bridge = bridge()?;
        let id = bridge.pending.next_id();
        let channel = MessageChannel::new()?;
        let (send, commands) = async_channel::bounded(1);
        let state = Rc::new_cyclic(|weak: &Weak<Background>| {
            let message = weak.clone();
            let error = weak.clone();
            let port = Port::new(
                channel.port1(),
                move |event| {
                    let Some(state) = message.upgrade() else {
                        return;
                    };
                    let data = event.data();
                    use super::control::BackgroundReply;
                    match BackgroundReply::decode(&data) {
                        Ok(BackgroundReply::Stopped) | Err(_) => state.stop(),
                        Ok(BackgroundReply::Cycle { id, refresh }) => {
                            state.active.set(id);
                            if send.try_send(refresh).is_err() {
                                state.stop();
                            }
                        }
                    }
                },
                move |_| {
                    if let Some(state) = error.upgrade() {
                        state.stop();
                    }
                },
            );
            Background { bridge: bridge.clone(), id, port: RefCell::new(Some(port)), active: Cell::new(0), commands }
        });
        let weak = Rc::downgrade(&state);
        bridge.pending.insert(
            id,
            Box::new(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.stop();
                }
            }),
        );
        let options = object(&[("retry", retry.into()), ("debounce", debounce.into())]);
        if let Err(error) = bridge.scope.post_message_with_transfer(&object(&[("transport", "background_start".into()), ("port", channel.port2().into()), ("options", options)]), &Array::of1(&channel.port2())) {
            channel.port2().close();
            state.stop();
            return Err(error);
        }
        Ok(Self(state))
    }
    pub fn notify(&self, kind: &str) {
        if let Some(port) = self.0.port.borrow().as_ref() {
            let _ = port.raw.post_message(&object(&[("kind", kind.into())]));
        }
    }
    pub fn wake_after(&self, delay: f64) {
        if let Some(port) = self.0.port.borrow().as_ref() {
            let _ = port.raw.post_message(&object(&[("kind", "wake_after".into()), ("delay", delay.into())]));
        }
    }
    pub fn finish(&self, success: bool) -> Result<(), JsValue> {
        if let Some(port) = self.0.port.borrow().as_ref() {
            port.raw.post_message(&object(&[("kind", "cycle_result".into()), ("id", self.0.active.get().into()), ("success", success.into())]))?;
        }
        Ok(())
    }
}
impl BackgroundHost {
    pub async fn next_cycle(&self) -> Result<bool, String> {
        next_background_cycle(&self.0).await
    }
}

async fn next_background_cycle(state: &Background) -> Result<bool, String> {
    if state.port.borrow().is_none() {
        return Err("Background scheduler stopped".into());
    }
    let result = state.commands.recv().await;
    if state.port.borrow().is_none() {
        return Err("Background scheduler stopped".into());
    }
    result.map_err(|_| "Background scheduler stopped".to_owned())
}

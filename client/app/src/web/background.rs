use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use async_channel::{Receiver, Sender, unbounded};
use web_sys::MessagePort;

use super::{Port, object};
use client_platform_web::reply::matching;
use client_platform_web::transport::control::BackgroundEvent;
use library_runtime::scheduler::{SyncRequest, run_timed};

struct Cycle {
    port: Port,
    replies: Receiver<(u32, bool)>,
    next: u32,
    stopped: Rc<Cell<bool>>,
}
impl Cycle {
    async fn request(&mut self, refresh_remote: bool) -> Result<(), ()> {
        if self.stopped.get() {
            return Err(());
        }
        let id = self.next.checked_add(1).ok_or(())?;
        self.next = id;
        self.port.raw.post_message(&object(&[("kind", "cycle".into()), ("id", id.into()), ("refresh_remote", refresh_remote.into())])).map_err(|_| ())?;
        if matching(&self.replies, id).await.unwrap_or(false) { Ok(()) } else { Err(()) }
    }
}
impl Drop for Cycle {
    fn drop(&mut self) {
        self.stopped.set(true);
        self.replies.close();
        let _ = self.port.raw.post_message(&object(&[("kind", "stopped".into())]));
    }
}

fn stop(stopped: &Cell<bool>, signals: &Sender<SyncRequest>, replies: &Sender<(u32, bool)>) {
    if !stopped.replace(true) {
        let _ = signals.try_send(SyncRequest::Stop);
        replies.close();
    }
}

/// Shared browser timers request whole cycles from the database worker.
/// The scheduler task owns its port and lends it to one cycle at a time.
pub struct BackgroundService {
    signals: Sender<SyncRequest>,
    replies: Sender<(u32, bool)>,
    stopped: Rc<Cell<bool>>,
}
impl Drop for BackgroundService {
    fn drop(&mut self) {
        stop(&self.stopped, &self.signals, &self.replies);
    }
}
impl BackgroundService {
    pub fn new(port: MessagePort, retry: u32, debounce: u32) -> Self {
        let (signals, requests) = unbounded();
        let (replies, incoming) = unbounded();
        let stopped = Rc::new(Cell::new(false));
        let message_signals = signals.clone();
        let message_replies = replies.clone();
        let message_stopped = stopped.clone();
        let error_signals = signals.clone();
        let error_replies = replies.clone();
        let error_stopped = stopped.clone();
        let port = Port::listen(
            port,
            move |data| match BackgroundEvent::decode(&data) {
                Ok(BackgroundEvent::Stop) | Err(_) => stop(&message_stopped, &message_signals, &message_replies),
                Ok(BackgroundEvent::Wake) => {
                    let _ = message_signals.try_send(SyncRequest::Wake(Vec::new()));
                }
                Ok(BackgroundEvent::WakeAfter(delay)) => {
                    let _ = message_signals.try_send(SyncRequest::WakeAfter(Duration::from_millis(delay.into())));
                }
                Ok(BackgroundEvent::Refresh) => {
                    let _ = message_signals.try_send(SyncRequest::RefreshRemote);
                }
                Ok(BackgroundEvent::Result { id, success }) => {
                    let _ = message_replies.try_send((id, success));
                }
            },
            move || stop(&error_stopped, &error_signals, &error_replies),
        );
        let mut cycle = Cycle { port, replies: incoming, next: 0, stopped: stopped.clone() };
        wasm_bindgen_futures::spawn_local(async move {
            run_timed(async |input| cycle.request(input.remote).await, requests, Duration::from_millis(retry.into()), Duration::from_millis(debounce.into())).await;
        });
        Self { signals, replies, stopped }
    }
    pub fn stopped(&self) -> bool {
        self.stopped.get()
    }
}

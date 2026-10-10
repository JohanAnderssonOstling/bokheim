/// Admission limits for concurrent application sync requests.
#[derive(Clone, Copy, Debug)]
pub struct SyncOptions {
    pub jobs: usize,
    pub bytes: usize,
}
impl Default for SyncOptions {
    fn default() -> Self {
        Self { jobs: 256, bytes: 16 * 1024 * 1024 }
    }
}

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use futures_channel::oneshot;
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

use super::{BytesFuture, BytesOperation, Port, byte_reply, error_message, object};
use client_platform_runtime::byte_budget::{ByteBudget, Reservation};
use client_platform_runtime::coordinator::SyncJobs;
use client_platform_runtime::pending::{OnDrop, take_matching};
use client_platform_web::transport::protocol::SyncEvent;

type Reply = oneshot::Sender<Result<Vec<u8>, String>>;
type Pending = Rc<RefCell<Option<(u32, Reply)>>>;
struct Request {
    _bytes: Reservation,
    port: MessagePort,
    bytes: Vec<u8>,
}
struct Job {
    port: Port,
    pending: Pending,
    next: Cell<u32>,
    abort: Rc<RefCell<Option<oneshot::Sender<String>>>>,
}
impl Job {
    fn reject_storage(&self, error: &str) {
        if let Some((_, reply)) = self.pending.borrow_mut().take() {
            let _ = reply.send(Err(error.to_owned()));
        }
    }
    fn storage(&self, bytes: Vec<u8>) -> BytesFuture {
        // SyncEngine and CoordinatorStore await each command before issuing another.
        if self.pending.borrow().is_some() {
            return Box::pin(async { Err("Sync storage command already active".into()) });
        }
        let Some(id) = self.next.get().checked_add(1) else {
            return Box::pin(async { Err("Sync request IDs exhausted".into()) });
        };
        self.next.set(id);
        let (reply, response) = oneshot::channel();
        *self.pending.borrow_mut() = Some((id, reply));
        let bytes = Uint8Array::from(bytes.as_slice());
        let transfer = Array::of1(&bytes.buffer());
        let message = object(&[("kind", "storage".into()), ("id", id.into()), ("bytes", bytes.into())]);
        if let Err(error) = self.port.raw.post_message_with_transferable(&message, &transfer) {
            if let Some((_, reply)) = self.pending.borrow_mut().take() {
                let _ = reply.send(Err(error_message(&error)));
            }
        }
        let pending = self.pending.clone();
        let cleanup = OnDrop::new(move || {
            drop(take_matching(&pending, id));
        });
        Box::pin(async move {
            let _cleanup = cleanup;
            response.await.unwrap_or_else(|_| Err("Sync storage connection closed".into()))
        })
    }
}
struct State {
    policy: SyncJobs<Request>,
    jobs: HashMap<String, Rc<Job>>,
}
struct Service {
    budget: ByteBudget,
    state: RefCell<State>,
}
impl Service {
    fn reply(request: Request, result: &JsValue) {
        let _ = request.port.post_message(result);
        request.port.close();
    }
    fn execute(self: &Rc<Self>, key: String) {
        let (port, bytes) = {
            let mut state = self.state.borrow_mut();
            let Some(request) = state.policy.active_mut(&key) else {
                return;
            };
            (request.port.clone(), std::mem::take(&mut request.bytes))
        };
        let pending: Pending = Rc::new(RefCell::new(None));
        let replies = pending.clone();
        let (abort, aborted) = oneshot::channel();
        let abort = Rc::new(RefCell::new(Some(abort)));
        let cancellation = abort.clone();
        let message = move |data: JsValue| match SyncEvent::decode(&data) {
            Ok(SyncEvent::Cancel) => {
                if let Some(abort) = cancellation.borrow_mut().take() {
                    let _ = abort.send("Sync request cancelled".to_owned());
                }
            }
            Ok(SyncEvent::Reply { id, result }) => {
                let reply = take_matching(&replies, id);
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            Err(error) => {
                if let Some(abort) = cancellation.borrow_mut().take() {
                    let _ = abort.send(error.clone());
                }
                if let Some((_, reply)) = replies.borrow_mut().take() {
                    let _ = reply.send(Err(error));
                }
            }
        };
        let replies = pending.clone();
        let malformed = move || {
            if let Some((_, reply)) = replies.borrow_mut().take() {
                let _ = reply.send(Err("Invalid sync storage response".into()));
            }
        };
        let job = Rc::new(Job { port: Port::listen(port, message, malformed), pending, next: Cell::new(0), abort });
        self.state.borrow_mut().jobs.insert(key.clone(), job.clone());
        let storage = job.clone();
        let storage: BytesOperation = Rc::new(move |bytes| storage.storage(bytes));
        let service = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = tokio::select! {
                biased;
                error = aborted => Err(error.unwrap_or_else(|_| "Sync pass ended".into())),
                result = library_sync::run(&bytes, storage) => result,
            };
            let kind = if result.is_ok() { "done" } else { "error" };
            let (result, _) = byte_reply(&[("kind", kind.into())], result);
            service.state.borrow_mut().jobs.remove(&key);
            job.reject_storage("Sync pass ended");
            let completed = service.state.borrow_mut().policy.complete(&key);
            for request in completed {
                Self::reply(request, &result);
            }
            service.execute(key);
        });
    }
}

/// Sync passes receive their storage endpoint directly as a Rust operation.
pub struct SyncService(Rc<Service>);
impl SyncService {
    pub fn request_bytes(&self, key: String, bytes: Vec<u8>, port: MessagePort) {
        let size = bytes.len();
        let Some(reservation) = self.0.budget.reserve(size) else {
            let error = if size > self.0.budget.limit() { "Sync request exceeds configured input byte limit" } else { "Sync input queue byte limit exceeded; retry after queued work completes" };
            let _ = port.post_message(&object(&[("kind", "error".into()), ("error", error.into())]));
            port.close();
            return;
        };
        let start = self.0.state.borrow_mut().policy.request(key.clone(), Request { _bytes: reservation, port, bytes });
        match start {
            Err((error, request)) => Service::reply(request, &object(&[("kind", "error".into()), ("error", error.into())])),
            Ok(true) => self.0.execute(key),
            Ok(false) => {}
        }
    }
    pub fn with_options(options: SyncOptions) -> Self {
        Self(Rc::new(Service { budget: ByteBudget::new(options.bytes), state: RefCell::new(State { policy: SyncJobs::new(options.jobs), jobs: HashMap::new() }) }))
    }

    pub fn fail(&self, error: JsValue) {
        let error = error_message(&error);
        let (jobs, requests) = {
            let mut state = self.0.state.borrow_mut();
            (state.jobs.values().cloned().collect::<Vec<_>>(), state.policy.fail(error.clone()))
        };
        for job in jobs {
            if let Some(abort) = job.abort.borrow_mut().take() {
                let _ = abort.send(error.clone());
            }
            job.reject_storage(&error);
        }
        let result = object(&[("kind", "error".into()), ("error", error.into())]);
        for request in requests {
            Service::reply(request, &result);
        }
    }
}

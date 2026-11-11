//! Browser CPU worker execution for opaque application requests.

use std::time::Duration;
/// Admission limits and startup deadline for a browser CPU worker.
#[derive(Clone, Copy, Debug)]
pub struct CpuWorkerOptions {
    pub jobs: usize,
    pub bytes: usize,
    pub startup_timeout: Duration,
}
impl Default for CpuWorkerOptions {
    fn default() -> Self {
        Self { jobs: 32, bytes: 64 * 1024 * 1024, startup_timeout: Duration::from_secs(5) }
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use futures_channel::oneshot;
use wasm_bindgen::prelude::*;

use super::BytesFuture;
use operation_failure::{Failure, OperationError};
use crate::transport::endpoint::{OwnedEndpoint, WorkerEndpoint};
use crate::transport::protocol::{CpuEvent, CpuRequest};
use client_platform_runtime::coordinator::CpuJobs;

type Reply = oneshot::Sender<Result<Vec<u8>, OperationError>>;
struct Job {
    _bytes: client_platform_runtime::byte_budget::Reservation,
    source: crate::transport::request::Source,
    bytes: Vec<u8>,
    reply: Reply,
}
type Connection = client_platform_runtime::worker_lifecycle::Connection<OwnedEndpoint, oneshot::Sender<()>>;
struct State {
    policy: CpuJobs<Job>,
    next: u32,
    worker: Option<Connection>,
    next_generation: u64,
}
struct Service {
    budget: client_platform_runtime::byte_budget::ByteBudget,
    startup_timeout: Duration,
    create_worker: Box<dyn Fn() -> Result<WorkerEndpoint, JsValue>>,
    state: RefCell<State>,
}

impl Service {
    fn fail(&self, error: JsValue) {
        let (worker, jobs) = {
            let mut state = self.state.borrow_mut();
            (state.worker.take(), state.policy.fail())
        };
        drop(worker);
        let error = OperationError::from(error);
        for job in jobs {
            let _ = job.reply.send(Err(error.clone()));
        }
    }

    fn start(self: &Rc<Self>) -> Result<(), JsValue> {
        let generation = {
            let mut state = self.state.borrow_mut();
            state.next_generation = state.next_generation.checked_add(1).ok_or_else(|| JsValue::from_str("CPU worker generations exhausted"))?;
            state.next_generation
        };
        let worker = (self.create_worker)()?;
        let service = Rc::downgrade(self);
        let message = move |data: JsValue| {
            let Some(service) = service.upgrade() else {
                return;
            };
            if service.state.borrow().worker.as_ref().is_some_and(|worker| worker.accepts(generation)) {
                service.receive(generation, data);
            }
        };
        let service = Rc::downgrade(self);
        let failed = move |message: String| {
            if let Some(service) = service.upgrade() {
                if service.state.borrow().worker.as_ref().is_some_and(|worker| worker.accepts(generation)) {
                    service.fail(js_sys::Error::new(&message).into());
                }
            }
        };
        let endpoint = OwnedEndpoint::listen(worker, message, failed, "CPU worker stopped", "Invalid CPU worker message");
        let service = Rc::downgrade(self);
        let startup_timeout = self.startup_timeout;
        let (cancel, cancelled) = oneshot::channel();
        self.state.borrow_mut().worker = Some(Connection::new(endpoint, generation, cancel));
        wasm_bindgen_futures::spawn_local(async move {
            let Some(elapsed) = client_platform_runtime::task_scope::until_cancelled(client_platform_runtime::executor::sleep(startup_timeout), cancelled).await else { return };
            if let Some(service) = service.upgrade() {
                if service.state.borrow().worker.as_ref().is_some_and(|worker| worker.waiting(generation)) {
                    let error = elapsed.err().map(OperationError::operation).unwrap_or_else(|| OperationError::classified(Failure::TimedOut, "CPU worker startup timed out"));
                    service.fail(error.into_js());
                }
            }
        });
        Ok(())
    }

    fn receive(&self, generation: u64, message: JsValue) {
        let event = match CpuEvent::decode(&message) {
            Ok(event) => event,
            Err(error) => {
                self.fail(error.into());
                return;
            }
        };
        match event {
            CpuEvent::Failed(error) => {
                self.fail(error.into_js());
                return;
            }
            CpuEvent::Ready => {
                let mut state = self.state.borrow_mut();
                if let Some(worker) = state.worker.as_mut() {
                    worker.ready(generation);
                }
                state.policy.ready();
            }
            CpuEvent::Reply { id, result } => {
                let completed = self.state.borrow_mut().policy.complete(id);
                match completed {
                    Ok(job) => {
                        let _ = job.reply.send(result);
                    }
                    Err(error) => {
                        self.fail(error.into());
                        return;
                    }
                }
            }
        }
        self.pump();
    }

    fn pump(&self) {
        let work = {
            let mut state = self.state.borrow_mut();
            let Some(worker) = state.worker.as_ref().map(|worker| worker.endpoint.endpoint().clone()) else {
                drop(state);
                self.fail("CPU worker is unavailable".into());
                return;
            };
            state.policy.next().map(|(id, job)| (id, std::mem::take(&mut job.bytes), std::mem::take(&mut job.source), worker))
        };
        if let Some((id, bytes, mut source, worker)) = work {
            let (message, transfer) = CpuRequest { id, bytes, source: source.value() }.encode();
            if let Err(error) = worker.post_message_with_transfer(&message, &transfer) {
                self.fail(error);
            } else {
                source.transferred();
            }
        }
    }
}

/// CPU scheduling, pending replies and worker callbacks owned by Rust.
pub struct CpuService(Rc<Service>);
impl CpuService {
    pub fn fail(&self, error: JsValue) {
        self.0.fail(error);
    }
}

impl CpuService {
    pub fn request_source(&self, bytes: Vec<u8>, source: JsValue) -> BytesFuture {
        let source = crate::transport::request::Source::new(source);
        let service = self.0.clone();
        Box::pin(async move {
            let Some(reservation) = service.budget.reserve_when_available(bytes.len()).await else {
                return Err("CPU request exceeds configured input byte limit".into());
            };
            let (sender, receiver) = oneshot::channel();
            let (id, start) = {
                let mut state = service.state.borrow_mut();
                let id = state.next;
                let Some(next) = state.next.checked_add(1) else {
                    return Err("CPU request IDs exhausted".into());
                };
                state.next = next;
                match state.policy.request(id, Job { _bytes: reservation, bytes, source, reply: sender }) {
                    Ok(()) => (id, state.worker.is_none()),
                    Err((error, _)) => return Err(error.into()),
                }
            };
            if start {
                if let Err(error) = service.start() {
                    service.fail(error);
                }
            }
            service.pump();
            let service = Rc::downgrade(&service);
            let cleanup = client_platform_runtime::task_scope::OnDrop::new(move || {
                if let Some(service) = service.upgrade() {
                    let cancelled = service.state.borrow_mut().policy.cancel_pending(id);
                    drop(cancelled);
                }
            });
            let _cleanup = cleanup;
            receiver.await.unwrap_or_else(|_| Err("CPU service closed".into()))
        })
    }

    pub fn with_options(create_worker: impl Fn() -> Result<WorkerEndpoint, JsValue> + 'static, options: CpuWorkerOptions) -> Self {
        Self(Rc::new(Service {
            budget: client_platform_runtime::byte_budget::ByteBudget::new(options.bytes),
            startup_timeout: options.startup_timeout,
            create_worker: Box::new(create_worker),
            state: RefCell::new(State { policy: CpuJobs::new(options.jobs), next: 1, worker: None, next_generation: 0 }),
        }))
    }
}

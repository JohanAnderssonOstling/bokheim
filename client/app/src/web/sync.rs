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

use async_channel::{Receiver, Sender, unbounded};
use futures_util::{StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use js_sys::{Array, Uint8Array};
use std::cell::OnceCell;
use std::mem::take;
use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

use super::{Port, byte_reply, error_message, object};
use client_platform_runtime::byte_budget::{ByteBudget, Reservation};
use client_platform_runtime::coordinator::SyncJobs;
use client_platform_web::storage_job::{self, Event as StorageEvent, ResultBytes};
use client_platform_web::transport::protocol::SyncEvent;

struct Request {
    _bytes: Reservation,
    _slot: Reservation,
    port: MessagePort,
    bytes: Vec<u8>,
}
impl Drop for Request {
    fn drop(&mut self) {
        self.port.close();
    }
}

struct Outcome {
    port: Port,
    result: ResultBytes,
}

async fn run_job(port: MessagePort, bytes: Vec<u8>) -> Outcome {
    let (commands, events) = unbounded();
    let replies = commands.clone();
    let malformed = commands.clone();
    let port = Port::listen(
        port,
        move |data| {
            let event = match SyncEvent::decode(&data) {
                Ok(SyncEvent::Cancel) => StorageEvent::Failed("Sync request cancelled".into()),
                Ok(SyncEvent::Reply { id, result }) => StorageEvent::Reply { id, result },
                Err(error) => StorageEvent::Failed(error),
            };
            let _ = replies.try_send(event);
        },
        move || {
            let _ = malformed.try_send(StorageEvent::Malformed);
        },
    );
    let result = storage_job::run(
        events,
        commands,
        async move |storage| library_sync::run(&bytes, storage).await,
        |id, bytes| {
            let bytes = Uint8Array::from(bytes.as_slice());
            let transfer = Array::of1(&bytes.buffer());
            let message = object(&[("kind", "storage".into()), ("id", id.into()), ("bytes", bytes.into())]);
            port.raw.post_message_with_transferable(&message, &transfer).map_err(|error| error_message(&error))
        },
    )
    .await;
    // Keep the callback owner alive until the service posts the final reply.
    Outcome { port, result }
}

enum Command {
    Request { key: String, request: Request },
    Fail(String),
}

struct State {
    policy: SyncJobs<Request>,
    jobs: FuturesUnordered<LocalBoxFuture<'static, (String, Outcome)>>,
}
impl State {
    fn reply(request: Request, result: &JsValue) {
        let _ = request.port.post_message(result);
        // Request::drop closes the port and releases both admission reservations.
    }

    fn start(&mut self, key: &str) {
        let Some(request) = self.policy.active_mut(key) else { return };
        let port = request.port.clone();
        let bytes = take(&mut request.bytes);
        let key = key.to_owned();
        self.jobs.push(Box::pin(async move { (key, run_job(port, bytes).await) }));
    }

    fn fail(&mut self, error: &str) {
        let requests = self.policy.fail(error.to_owned());
        let result = object(&[("kind", "error".into()), ("error", error.into())]);
        // Send errors while the job ports are still live, then drop the futures.
        // Dropping those owners cancels engines and their active storage calls.
        for request in requests {
            Self::reply(request, &result);
        }
        self.jobs.clear();
    }

    async fn run(mut self, commands: Receiver<Command>) {
        enum Update {
            Command(Command),
            Complete(String, Outcome),
            Closed,
        }
        loop {
            let update = tokio::select! {
                biased;
                command = commands.recv() => match command { Ok(command) => Update::Command(command), Err(_) => Update::Closed },
                outcome = self.jobs.next(), if !self.jobs.is_empty() => {
                    match outcome {
                        Some((key, outcome)) => Update::Complete(key, outcome),
                        None => Update::Closed,
                    }
                },
            };
            if commands.is_closed() {
                if let Update::Command(Command::Request { request, .. }) = update {
                    Self::reply(request, &object(&[("kind", "error".into()), ("error", "Sync service closed".into())]));
                }
                break;
            }
            match update {
                Update::Command(Command::Request { key, request }) => match self.policy.request(key.clone(), request) {
                    Ok(true) => self.start(&key),
                    Ok(false) => {}
                    Err((error, request)) => Self::reply(request, &object(&[("kind", "error".into()), ("error", error.into())])),
                },
                Update::Command(Command::Fail(error)) => self.fail(&error),
                Update::Complete(key, outcome) => {
                    let kind = if outcome.result.is_ok() { "done" } else { "error" };
                    let (result, _) = byte_reply(&[("kind", kind.into())], outcome.result);
                    for request in self.policy.complete(&key) {
                        Self::reply(request, &result);
                    }
                    drop(outcome.port);
                    self.start(&key);
                }
                Update::Closed => break,
            }
        }
        self.fail("Sync service closed");
        while let Ok(command) = commands.try_recv() {
            if let Command::Request { request, .. } = command {
                Self::reply(request, &object(&[("kind", "error".into()), ("error", "Sync service closed".into())]));
            }
        }
    }
}

/// The service task owns queued requests and every running sync future.
/// Admission reservations also bound requests waiting in its mailbox.
pub struct SyncService {
    budget: ByteBudget,
    slots: ByteBudget,
    commands: Sender<Command>,
    failure: OnceCell<String>,
}
impl Drop for SyncService {
    fn drop(&mut self) {
        self.commands.close();
    }
}
impl SyncService {
    pub fn request_bytes(&self, key: String, bytes: Vec<u8>, port: MessagePort) {
        if let Some(error) = self.failure.get() {
            reject(&port, error);
            return;
        }
        let size = bytes.len();
        let Some(reservation) = self.budget.reserve(size) else {
            let error = if size > self.budget.limit() { "Sync request exceeds configured input byte limit" } else { "Sync input queue byte limit exceeded; retry after queued work completes" };
            reject(&port, error);
            return;
        };
        let Some(slot) = self.slots.reserve(1) else {
            reject(&port, "Sync queue is full");
            return;
        };
        let request = Request { _bytes: reservation, _slot: slot, port, bytes };
        if let Err(error) = self.commands.try_send(Command::Request { key, request }) {
            if let Command::Request { request, .. } = error.into_inner() {
                State::reply(request, &object(&[("kind", "error".into()), ("error", "Sync service closed".into())]));
            }
        }
    }

    pub fn with_options(options: SyncOptions) -> Self {
        let (commands, requests) = unbounded();
        let state = State { policy: SyncJobs::new(options.jobs), jobs: FuturesUnordered::new() };
        wasm_bindgen_futures::spawn_local(state.run(requests));
        Self { budget: ByteBudget::new(options.bytes), slots: ByteBudget::new(options.jobs), commands, failure: OnceCell::new() }
    }

    pub fn fail(&self, error: JsValue) {
        let error = error_message(&error);
        if self.failure.set(error.clone()).is_ok() {
            let _ = self.commands.try_send(Command::Fail(error));
        }
    }
}

fn reject(port: &MessagePort, error: &str) {
    let _ = port.post_message(&object(&[("kind", "error".into()), ("error", error.into())]));
    port.close();
}

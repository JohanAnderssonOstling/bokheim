//! Connection state owned by the browser transport task. Callbacks only enqueue
//! messages; reconnecting never replays writes or book capabilities.
use super::{AppStartupState, AppWorkerClientMessage, AppWorkerRequest, AppWorkerStartupMessage, LibrarySubscription, WorkerCommand, WorkerPayload};
use crate::BackendError;
use async_channel::Sender;
use client_runtime::wire::{decode_worker_message, decode_worker_payload, encode_reply};
use library_model::LibraryUpdate;
use std::collections::HashMap;
use std::mem::take;
use tokio::sync::oneshot;

pub(super) type Reply = oneshot::Sender<Result<WorkerPayload, BackendError>>;

struct Subscription {
    events: Sender<WorkerPayload>,
    restore: Option<Vec<u8>>,
}

pub(super) struct WebState {
    startup: Option<oneshot::Sender<Result<AppStartupState, BackendError>>>,
    failure: Option<BackendError>,
    recovering: bool,
    pending: HashMap<u64, Reply>,
    subscriptions: HashMap<u64, Subscription>,
}

impl WebState {
    pub(super) fn new(startup: oneshot::Sender<Result<AppStartupState, BackendError>>) -> Self {
        Self { startup: Some(startup), failure: None, recovering: false, pending: HashMap::new(), subscriptions: HashMap::new() }
    }

    pub(super) fn failed(&self) -> bool {
        self.failure.is_some()
    }

    pub(super) fn fail(&mut self, error: BackendError) {
        if self.failed() {
            return;
        }
        self.failure = Some(error.clone());
        if let Some(startup) = self.startup.take() {
            let _ = startup.send(Err(error.clone()));
        }
        self.fail_pending(error);
        self.subscriptions.clear();
    }

    fn fail_pending(&mut self, error: BackendError) {
        for reply in take(&mut self.pending).into_values() {
            let _ = reply.send(Err(error.clone()));
        }
    }

    pub(super) fn request(&mut self, id: u64, reply: Reply, events: Option<Sender<WorkerPayload>>, restore: Option<Vec<u8>>, send: impl FnOnce() -> Result<(), String>) {
        let error = if let Some(error) = &self.failure {
            Some(error.clone())
        } else if self.recovering {
            Some("Database connection is being restored. Try again shortly.".into())
        } else {
            None
        };
        if let Some(error) = error {
            let _ = reply.send(Err(error));
            return;
        }
        self.pending.insert(id, reply);
        if let Some(events) = events {
            self.subscriptions.insert(id, Subscription { events, restore });
        }
        if let Err(error) = send() {
            self.subscriptions.remove(&id);
            if let Some(reply) = self.pending.remove(&id) {
                let _ = reply.send(Err(BackendError::operation(error).context("failed to send backend request")));
            }
        }
    }

    pub(super) fn cancel(&mut self, id: u64, subscription: bool) {
        self.pending.remove(&id);
        if subscription {
            self.subscriptions.remove(&id);
        }
    }

    pub(super) fn reconnect(&mut self) {
        if self.failed() {
            return;
        }
        self.recovering = true;
        self.subscriptions.retain(|_, subscription| subscription.restore.is_some());
        self.fail_pending("Database connection interrupted. A write may already have committed; check its result before retrying.".into());
    }

    pub(super) fn receive(&mut self, message: AppWorkerStartupMessage, mut send: impl FnMut(&[u8]) -> Result<(), String>) {
        if self.failed() {
            return;
        }
        match message {
            AppWorkerStartupMessage::Ready { startup } => {
                if take(&mut self.recovering) {
                    for subscription in self.subscriptions.values() {
                        if let Some(bytes) = &subscription.restore {
                            if let Err(error) = send(bytes) {
                                self.fail(BackendError::operation(error).context("Could not restore database subscription"));
                                return;
                            }
                        }
                    }
                }
                if let Some(reply) = self.startup.take() {
                    let _ = reply.send(Ok(startup));
                }
            }
            AppWorkerStartupMessage::Failed { error } => self.fail(error),
            AppWorkerStartupMessage::Response { id, result } => {
                if let Some(reply) = self.pending.remove(&id) {
                    let _ = reply.send(result);
                } else if let Some(subscription) = self.subscriptions.get(&id) {
                    if let Some(command) = &subscription.restore {
                        match restored_subscription_updates(command, result) {
                            Ok(updates) => {
                                for update in updates {
                                    let _ = subscription.events.try_send(update);
                                }
                            }
                            Err(error) => self.fail(BackendError::operation(error).context("Could not restore database subscription")),
                        }
                    }
                }
            }
            AppWorkerStartupMessage::Event { id, payload } => {
                if let Some(subscription) = self.subscriptions.get(&id) {
                    let _ = subscription.events.try_send(payload);
                }
            }
        }
    }
}

fn restored_subscription_updates(command: &[u8], result: Result<WorkerPayload, BackendError>) -> Result<Vec<WorkerPayload>, BackendError> {
    let initial = result?;
    let request: AppWorkerClientMessage = decode_worker_message(command)?;
    if let AppWorkerClientMessage::Request(AppWorkerRequest { command: WorkerCommand::LibrarySubscription { subscription, .. }, .. }) = request {
        match subscription {
            LibrarySubscription::Updates => {
                let scanning = decode_worker_payload::<bool>(initial)?;
                [LibraryUpdate::Contents, LibraryUpdate::Scanning(scanning)].into_iter().map(encode_reply).collect()
            }
            #[cfg(target_arch = "wasm32")]
            _ => Ok(vec![initial]),
        }
    } else {
        Ok(vec![initial])
    }
}

#[cfg(test)]
#[path = "../../tests/runtime/web_state/tests.rs"]
mod tests;

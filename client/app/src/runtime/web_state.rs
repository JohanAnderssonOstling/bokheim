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
mod tests {
    use super::*;
    use crate::{AccountStatus, runtime::AppCommand};
    use client_runtime::wire::encode_worker_message;

    fn state() -> WebState {
        WebState::new(oneshot::channel().0)
    }

    fn startup() -> AppStartupState {
        AppStartupState { libraries: Vec::new(), browsing_preferences: Default::default(), account_status: AccountStatus::signed_out(), reader_preferences: Default::default() }
    }

    fn subscribe(state: &mut WebState, id: u64, restore: Option<Vec<u8>>) -> (oneshot::Receiver<Result<WorkerPayload, BackendError>>, async_channel::Receiver<WorkerPayload>) {
        let (reply, response) = oneshot::channel();
        let (events, updates) = async_channel::unbounded();
        state.request(id, reply, Some(events), restore, || Ok(()));
        (response, updates)
    }

    #[test]
    fn reconnect_fails_inflight_writes_and_replays_only_retained_subscriptions() {
        let mut state = state();
        let (reply, mut write) = oneshot::channel();
        state.request(1, reply, None, None, || Ok(()));
        let command = encode_worker_message(&AppWorkerClientMessage::Request(AppWorkerRequest { id: 2, command: WorkerCommand::App(AppCommand::SubscribeLibraryList) })).unwrap();
        let (mut initial, updates) = subscribe(&mut state, 2, Some(command.clone()));
        let (_, capability) = subscribe(&mut state, 3, None);
        state.receive(AppWorkerStartupMessage::Response { id: 2, result: encode_reply(()) }, |_| Ok(()));
        assert!(initial.try_recv().unwrap().is_ok());
        state.reconnect();
        assert!(write.try_recv().unwrap().unwrap_err().to_string().contains("may already have committed"));
        assert!(capability.is_closed());
        assert!(!updates.is_closed());
        let mut replayed = Vec::new();
        state.receive(AppWorkerStartupMessage::Ready { startup: startup() }, |bytes| {
            replayed.push(bytes.to_vec());
            Ok(())
        });
        assert_eq!(replayed, vec![command]);
        state.receive(AppWorkerStartupMessage::Response { id: 2, result: encode_reply(()) }, |_| Ok(()));
        decode_worker_payload::<()>(updates.try_recv().unwrap()).unwrap();
    }

    #[test]
    fn requests_during_recovery_are_rejected_without_being_sent() {
        let mut state = state();
        state.reconnect();
        let (reply, mut response) = oneshot::channel();
        state.request(1, reply, None, None, || panic!("request sent while reconnecting"));
        assert!(response.try_recv().unwrap().is_err());
    }

    #[test]
    fn restored_library_subscription_refreshes_contents_and_scanning() {
        let mut state = state();
        let command = encode_worker_message(&AppWorkerClientMessage::Request(AppWorkerRequest { id: 1, command: WorkerCommand::LibrarySubscription { library_id: uuid::Uuid::nil(), subscription: LibrarySubscription::Updates } })).unwrap();
        let (_, updates) = subscribe(&mut state, 1, Some(command));
        state.receive(AppWorkerStartupMessage::Response { id: 1, result: encode_reply(false) }, |_| Ok(()));
        state.reconnect();
        state.receive(AppWorkerStartupMessage::Ready { startup: startup() }, |_| Ok(()));
        state.receive(AppWorkerStartupMessage::Response { id: 1, result: encode_reply(true) }, |_| Ok(()));
        assert!(matches!(decode_worker_payload::<LibraryUpdate>(updates.try_recv().unwrap()).unwrap(), LibraryUpdate::Contents));
        assert!(matches!(decode_worker_payload::<LibraryUpdate>(updates.try_recv().unwrap()).unwrap(), LibraryUpdate::Scanning(true)));
    }

    #[test]
    fn dropping_owner_releases_pending_replies_and_event_channels() {
        let mut state = state();
        let (mut response, updates) = subscribe(&mut state, 1, Some(vec![1]));
        drop(state);
        assert!(matches!(response.try_recv(), Err(oneshot::error::TryRecvError::Closed)));
        assert!(updates.is_closed());
    }

    #[test]
    fn cancelled_subscription_is_not_replayed_or_delivered_late_events() {
        let mut state = state();
        let (mut response, updates) = subscribe(&mut state, 1, Some(vec![1]));
        state.cancel(1, true);
        assert!(response.try_recv().is_err());
        assert!(updates.is_closed());
        state.reconnect();
        state.receive(AppWorkerStartupMessage::Ready { startup: startup() }, |_| panic!("cancelled subscription replayed"));
        state.receive(AppWorkerStartupMessage::Event { id: 1, payload: encode_reply(()).unwrap() }, |_| Ok(()));
        assert!(updates.try_recv().is_err());
    }

    #[test]
    fn terminal_failure_settles_startup_and_pending_once_and_closes_subscriptions() {
        let (startup, mut started) = oneshot::channel();
        let mut state = WebState::new(startup);
        let (mut pending, updates) = subscribe(&mut state, 1, Some(vec![1]));
        state.fail("first failure".into());
        state.fail("second failure".into());
        assert_eq!(started.try_recv().unwrap().unwrap_err().to_string(), "first failure");
        assert_eq!(pending.try_recv().unwrap().unwrap_err().to_string(), "first failure");
        assert!(updates.is_closed());
        let (reply, mut response) = oneshot::channel();
        state.request(2, reply, None, None, || panic!("request sent after failure"));
        assert_eq!(response.try_recv().unwrap().unwrap_err().to_string(), "first failure");
    }

    #[test]
    fn send_and_restore_failures_release_subscription_senders() {
        let mut state = state();
        let (reply, mut response) = oneshot::channel();
        let (events, updates) = async_channel::unbounded();
        state.request(1, reply, Some(events), Some(vec![1]), || Err("send failed".into()));
        assert!(response.try_recv().unwrap().is_err());
        assert!(updates.is_closed());
        let (_, updates) = subscribe(&mut state, 2, Some(vec![2]));
        state.reconnect();
        state.receive(AppWorkerStartupMessage::Ready { startup: startup() }, |_| Err("restore failed".into()));
        assert!(state.failed());
        assert!(updates.is_closed());
    }
}

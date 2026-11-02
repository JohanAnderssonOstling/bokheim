use super::*;
use crate::AccountStatus;
use client_runtime::wire::encode_worker_message;

fn state() -> WebState {
    WebState::new(oneshot::channel().0)
}

fn startup() -> AppStartupState {
    AppStartupState {
        default_library_root: String::new(),
        dictionary_storage_root: String::new(),
        preference_storage_root: String::new(),
        libraries: Vec::new(),
        browsing_preferences: Default::default(),
        account_status: AccountStatus::signed_out(),
        reader_preferences: Default::default(),
    }
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
    let command = encode_worker_message(&AppWorkerClientMessage::Request(AppWorkerRequest { id: 2, command: WorkerCommand::ManagementSubscription(library_backend::management::ManagementSubscription::LibraryList) })).unwrap();
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

use super::*;
use crate::account::{AccountSession as DeviceAccountSession, SessionStorage};
use library_registry::LibraryRegistry;
use library_services::interest::InterestState;
use std::sync::Arc;

#[tokio::test(start_paused = true)]
async fn subscribed_notifications_have_no_retry_timer_while_offline() {
    use futures_util::task::{waker_ref, ArcWake};
    use library_services::interest::NotificationInterest;
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Context;
    struct Wakes(AtomicUsize);
    impl ArcWake for Wakes {
        fn wake_by_ref(waker: &Arc<Self>) {
            waker.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let root = tempfile::tempdir().unwrap();
    let context = SessionStorage::new(LibraryRegistry::open_with_library_root(root.path().to_string_lossy(), root.path().join("libraries").to_string_lossy()).unwrap());
    let server = "http://127.0.0.1:1".parse().unwrap();
    let account = DeviceAccountSession::load(context, server).unwrap();
    account.set(account_client::Session::try_new("http://127.0.0.1:1".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap()).unwrap();
    let interest = NotificationInterest::default();
    let _browsing = interest.view(true);
    let (wake, _requests) = watch::channel(());
    let (connection, status) = watch::channel(false);
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = waker_ref(&wakes);
    let mut cx = Context::from_waker(&waker);
    let mut notifications = Box::pin(run_with_network(account.library_account(), wake, interest, Weak::new(), Connectivity::new(status)));
    assert!(notifications.as_mut().poll(&mut cx).is_pending());
    tokio::time::advance(Duration::from_secs(3600)).await;
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    connection.send_replace(true);
    assert!(wakes.0.load(Ordering::Relaxed) > 0, "reconnect schedules an immediate notification connection");
}

#[test]
fn account_discovery_wakes_for_account_events_without_open_libraries() {
    let root = tempfile::tempdir().unwrap();
    let context = SessionStorage::new(LibraryRegistry::open_with_library_root(root.path().to_string_lossy(), root.path().join("libraries").to_string_lossy()).unwrap());
    let account = DeviceAccountSession::load(context, "https://example.com".parse().unwrap()).unwrap();
    let libraries = LibraryNotifications::new(account.library_account(), InterestState::default());
    let (wake, mut changes) = watch::channel(());
    for (message, browsing, should_wake) in [
        (NotificationMessage::LibraryChanged { library_id: sync_common::LibraryId::new_v4() }, true, false),
        (NotificationMessage::Hello { heartbeat_seconds: 5, timeout_seconds: 15 }, false, false),
        (NotificationMessage::Hello { heartbeat_seconds: 5, timeout_seconds: 15 }, true, true),
        (NotificationMessage::LibrariesChanged, false, true),
        (NotificationMessage::Reconcile, false, true),
    ] {
        let bytes = sync_common::transport::encode(&message).unwrap();
        notify(&bytes, &wake, browsing, &libraries, &Weak::new()).unwrap();
        assert_eq!(changes.has_changed().unwrap(), should_wake, "{message:?}");
        changes.borrow_and_update();
    }
}

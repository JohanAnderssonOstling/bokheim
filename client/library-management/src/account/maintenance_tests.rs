use super::*;
use crate::account::{AccountSession as DeviceAccountSession, SessionStorage};
use futures_util::poll;
use library_registry::LibraryRegistry;
use tokio::time::advance;

#[tokio::test(start_paused = true)]
async fn offline_maintenance_parks_without_retaining_application_state() {
    let (sender, requests) = watch::channel(());
    let (_account_sender, changes) = watch::channel(0);
    let (connection, status) = watch::channel(false);
    let mut maintenance = Box::pin(run_maintenance_with_network(Weak::new(), requests, changes, Connectivity::new(status)));
    assert!(poll!(&mut maintenance).is_pending());
    sender.send_replace(());
    assert!(poll!(&mut maintenance).is_pending());
    advance(MAINTENANCE_DEBOUNCE).await;
    assert!(poll!(&mut maintenance).is_pending());
    advance(Duration::from_secs(3600)).await;
    assert!(poll!(&mut maintenance).is_pending());
    // Reconnection must be sufficient to schedule work without a new UI event.
    // No library owner remains here, so the worker exits immediately.
    connection.send_replace(true);
    maintenance.await;
}

#[tokio::test(start_paused = true)]
async fn closing_the_owner_while_offline_ends_maintenance() {
    let (sender, requests) = watch::channel(());
    let (_account_sender, changes) = watch::channel(0);
    let (_connection, status) = watch::channel(false);
    let mut maintenance = Box::pin(run_maintenance_with_network(Weak::new(), requests, changes, Connectivity::new(status)));
    assert!(poll!(&mut maintenance).is_pending());
    drop(sender);
    maintenance.await;
}

#[cfg(feature = "kobo")]
#[test]
fn repeated_account_failures_back_off_to_fifteen_minutes() {
    let mut delay = None;
    for seconds in [30, 60, 120, 240, 480, 900, 900] {
        delay = Some(next_maintenance_retry(delay));
        assert_eq!(delay, Some(Duration::from_secs(seconds)));
    }
}

#[tokio::test(start_paused = true)]
async fn failure_retries_after_thirty_seconds_without_an_event() {
    let (_sender, mut requests) = watch::channel(());
    let (_account_sender, mut changes) = watch::channel(0);
    let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, Some(MAINTENANCE_RETRY_INTERVAL)));
    assert!(poll!(&mut waiting).is_pending());
    advance(Duration::from_secs(29)).await;
    assert!(poll!(&mut waiting).is_pending());
    advance(Duration::from_secs(1)).await;
    assert!(waiting.await);
}

#[tokio::test(start_paused = true)]
async fn new_events_bypass_retry_wait_and_coalesce_during_debounce() {
    let (sender, mut requests) = watch::channel(());
    let (_account_sender, mut changes) = watch::channel(0);
    let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, Some(MAINTENANCE_RETRY_INTERVAL)));
    assert!(poll!(&mut waiting).is_pending());
    sender.send_replace(());
    assert!(poll!(&mut waiting).is_pending());
    sender.send_replace(());
    advance(Duration::from_millis(99)).await;
    assert!(poll!(&mut waiting).is_pending());
    advance(Duration::from_millis(1)).await;
    assert!(waiting.await);
    assert!(!requests.has_changed().unwrap());

    // An event raised by a running pass must still schedule the next pass.
    sender.send_replace(());
    let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, None));
    assert!(poll!(&mut waiting).is_pending());
    advance(MAINTENANCE_DEBOUNCE).await;
    assert!(waiting.await);
}

#[tokio::test(start_paused = true)]
async fn session_changes_wake_maintenance_without_a_notification_connection() {
    for retry in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let context = SessionStorage::new(LibraryRegistry::open_with_library_root(root.path().to_string_lossy(), root.path().join("libraries").to_string_lossy()).unwrap());
        let server = "https://example.com".parse().unwrap();
        let account = DeviceAccountSession::load(context, server).unwrap();
        let mut changes = account.subscribe();
        let (_sender, mut requests) = watch::channel(());
        let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, retry.then_some(MAINTENANCE_RETRY_INTERVAL)));
        assert!(poll!(&mut waiting).is_pending());

        let session = account_client::Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
        account.set(session.clone()).unwrap();
        assert!(poll!(&mut waiting).is_pending());
        account.set(session).unwrap();
        advance(MAINTENANCE_DEBOUNCE).await;
        assert!(waiting.await);
        assert!(!changes.has_changed().unwrap(), "session changes during debounce coalesce");
        assert!(!requests.has_changed().unwrap(), "maintenance does not need a forwarded wake");

        // A logout during a maintenance pass must schedule the next pass.
        account.take().unwrap();
        let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, None));
        assert!(poll!(&mut waiting).is_pending());
        advance(MAINTENANCE_DEBOUNCE).await;
        assert!(waiting.await);
    }
}

#[tokio::test(start_paused = true)]
async fn closing_wake_channel_stops_idle_and_retry_waits() {
    for retry in [false, true] {
        let (sender, mut requests) = watch::channel(());
        let (_account_sender, mut changes) = watch::channel(0);
        let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, retry.then_some(MAINTENANCE_RETRY_INTERVAL)));
        assert!(poll!(&mut waiting).is_pending());
        drop(sender);
        assert!(!waiting.await);
    }
}

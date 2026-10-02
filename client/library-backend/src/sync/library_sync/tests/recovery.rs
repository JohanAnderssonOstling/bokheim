use std::sync::atomic::{AtomicUsize, Ordering};
use super::*;
use crate::APP_HIDDEN_DIR;

/// When the server's state regresses beneath a device's cursor the device
/// cannot make progress in either direction. Recovery is the existing
/// republish path: drop the unusable cursor and re-reconcile, rather than
/// failing every future synchronization.

#[tokio::test]
async fn a_rejected_cursor_is_dropped_and_the_exchange_recovers() {
    #[derive(Clone)]
    struct ExchangeState {
        calls: Arc<AtomicUsize>,
    }
    async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        use axum::response::IntoResponse;

        let request = decode_sync_exchange(&headers, &body);
        state.calls.fetch_add(1, Ordering::SeqCst);
        match request.cursor.lifecycle_revision.map(LibraryRevision::get) {
            // The server has regressed beneath this cursor and says so.
            Some(99) => axum::http::StatusCode::GONE.into_response(),
            None => binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull: pull_response(vec![pulled_change(1, added(1, 10))], LibraryRevision::new(1).ok(), false) }),
            Some(1) => binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { rejected: Vec::new() },
                pull: pull_response(Vec::new(), LibraryRevision::new(1).ok(), false),
            }),
            other => panic!("unexpected cursor {other:?}"),
        }
    }

    let state = ExchangeState { calls: Arc::new(AtomicUsize::new(0)) };
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = test_shared_credentials(&manager.server_url);
    {
        let conn = &manager.database;
        conn.set_sync_pull_cursor(&cursor(LibraryRevision::new(99).ok())).unwrap();
    }

    manager.sync_once().await.expect("a rejected cursor must be recoverable without operator intervention");

    assert_eq!(state.calls.load(Ordering::SeqCst), 3, "recovery performs one clean pull before the one-shot publishing retry");
    let conn = &manager.database;
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(1).ok()), "the device adopts the server's current head");
    assert_eq!(conn.sync_metadata(CURSOR_RECOVERY_PENDING_KEY).unwrap(), None, "successful recovery clears its durable marker");
    server.abort();
}

#[tokio::test]
async fn beginning_cursor_recovery_atomically_persists_a_clean_cursor_and_resume_marker() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    {
        let conn = &manager.database;
        conn.set_sync_pull_cursor(&cursor(LibraryRevision::new(99).ok())).unwrap();
    }

    manager.state_engine().unwrap().begin_cursor_recovery().await.unwrap();

    let conn = &manager.database;
    assert_eq!(conn.sync_pull_cursor().unwrap(), SyncCursor::default());
    assert_eq!(conn.sync_metadata(CURSOR_RECOVERY_PENDING_KEY).unwrap().as_deref(), Some("1"));
}

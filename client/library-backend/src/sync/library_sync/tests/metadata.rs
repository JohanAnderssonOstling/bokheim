use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::*;
use crate::APP_HIDDEN_DIR;

#[tokio::test]
async fn committed_pages_notify_before_the_next_http_page_finishes() {
    use axum::response::IntoResponse;
    let app = axum::Router::new().route(
        "/api/sync/exchange",
        axum::routing::post(|headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
            let request = decode_sync_exchange(&headers, &body);
            if request.cursor.lifecycle_revision.is_some() {
                return std::future::pending().await;
            }
            binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull: pull_response(vec![pulled_change(1, added(1, 10))], LibraryRevision::new(1).ok(), true) })
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = test_shared_credentials(&manager.server_url);
    let (events, updates) = async_channel::unbounded();
    manager.event_tx = Some(events);
    let exchange = manager.synchronize_state();
    tokio::pin!(exchange);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        // The exchange also announces its own start (`TransferStatusChanged`)
        // before any page is fetched; only `ContentsChanged` confirms the
        // first page actually committed ahead of the blocked second page.
        loop {
            tokio::select! {
                result = &mut exchange => panic!("second page must remain blocked: {result:?}"),
                event = updates.recv() => match event.unwrap() {
                    LibraryEvent::ContentsChanged => break,
                    _ => continue,
                },
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.database.sync_pull_cursor().unwrap().lifecycle_revision, LibraryRevision::new(1).ok());
    assert!(manager.database.sync_metadata("last_successful_sync_at").unwrap().is_none());
    server.abort();
}

#[tokio::test]
async fn production_client_drains_all_pull_pages_before_finishing() {
    #[derive(Clone)]
    struct PageState(Arc<AtomicUsize>);

    async fn pull_page(axum::extract::State(state): axum::extract::State<PageState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        state.0.fetch_add(1, Ordering::SeqCst);
        let request = decode_sync_exchange(&headers, &body);
        let pull = match request.cursor.lifecycle_revision.map(LibraryRevision::get) {
            None => pull_response(vec![pulled_change(1, added(1, 10))], LibraryRevision::new(1).ok(), true),
            Some(1) => pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false),
            _ => PullStateResponse { mutations: Vec::new(), next_cursor: request.cursor, has_more: false },
        };
        binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull })
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page)).with_state(PageState(calls.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = test_shared_credentials(&manager.server_url);

    manager.state_engine().unwrap().pull_events_from_server(SyncCursor::default()).await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let conn = &manager.database;
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(2).ok()));
    server.abort();
}

#[tokio::test]
async fn production_client_persists_exact_final_cursor_beyond_last_mutation() {
    async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        let request = decode_sync_exchange(&headers, &body);
        binary_sync_response(&SyncExchangeResponse {
            push: PushMutationsResponse { rejected: Vec::new() },
            pull: pull_response(vec![pulled_change(1, added(1, 10))], request.cursor.lifecycle_revision.or(LibraryRevision::new(10).ok()), false),
        })
    }
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = test_shared_credentials(&manager.server_url);

    manager.state_engine().unwrap().pull_events_from_server(SyncCursor::default()).await.unwrap();

    let conn = &manager.database;
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(10).ok()));
    server.abort();
}

#[tokio::test]
async fn production_client_persists_advanced_cursor_from_empty_final_page() {
    async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        let request = decode_sync_exchange(&headers, &body);
        assert_eq!(request.cursor.lifecycle_revision.map(LibraryRevision::get), Some(10));
        binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull: pull_response(Vec::new(), LibraryRevision::new(12).ok(), false) })
    }
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page));
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
        conn.set_sync_pull_cursor(&cursor(LibraryRevision::new(10).ok())).unwrap();
        assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(10).ok()));
    }

    manager.state_engine().unwrap().pull_events_from_server(cursor(LibraryRevision::new(10).ok())).await.unwrap();

    let conn = &manager.database;
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(12).ok()));
    server.abort();
}

#[tokio::test]
async fn production_client_uses_exchange_then_drains_remaining_pull_pages() {
    #[derive(Clone)]
    struct ExchangeState {
        exchanges: Arc<AtomicUsize>,
        pulls: Arc<AtomicUsize>,
    }

    // Every page travels over the exchange endpoint: the first carries the
    // push and the first pull page, and each later page is the same request
    // with an advanced cursor and nothing to push.
    async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        let request = decode_sync_exchange(&headers, &body);
        match request.cursor.lifecycle_revision.map(LibraryRevision::get) {
            None => {
                state.exchanges.fetch_add(1, Ordering::SeqCst);
                binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull: pull_response(vec![pulled_change(1, added(1, 10))], LibraryRevision::new(1).ok(), true) })
            }
            Some(1) => {
                state.pulls.fetch_add(1, Ordering::SeqCst);
                assert!(request.mutations.is_empty(), "a drain page has nothing left to push");
                binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull: pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false) })
            }
            other => panic!("unexpected cursor {other:?}"),
        }
    }

    let state = ExchangeState { exchanges: Arc::new(AtomicUsize::new(0)), pulls: Arc::new(AtomicUsize::new(0)) };
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = test_shared_credentials(&manager.server_url);

    manager.state_engine().unwrap().exchange_events_with_server(SyncCursor::default()).await.unwrap();

    assert_eq!(state.exchanges.load(Ordering::SeqCst), 1);
    assert_eq!(state.pulls.load(Ordering::SeqCst), 1);
    let conn = &manager.database;
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(2).ok()));
    server.abort();
}

#[tokio::test]
async fn push_and_pull_pagination_advance_in_the_same_exchange_loop() {
    #[derive(Clone)]
    struct ExchangeState(Arc<AtomicUsize>);

    async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        let request = decode_sync_exchange(&headers, &body);
        let call = state.0.fetch_add(1, Ordering::SeqCst);
        let pull = match call {
            0 => {
                assert_eq!(request.cursor, SyncCursor::default());
                assert_eq!(request.mutations.len(), sync_common::MAX_PUSH_MUTATIONS);
                pull_response(vec![pulled_change(1, added(1, 10))], LibraryRevision::new(1).ok(), true)
            }
            1 => {
                assert_eq!(request.cursor.lifecycle_revision, LibraryRevision::new(1).ok());
                assert_eq!(request.mutations.len(), 1, "the next push page shares the next pull request");
                pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false)
            }
            2 => {
                assert_eq!(request.mutations.len(), 1, "the later lifecycle declaration schedules one reconciliation");
                assert_eq!(request.mutations[0].state.value.address.kind, "reading_position");
                assert_eq!(request.mutations[0].state.value.address.entity_key, fixture_content_hash(1).to_string());
                PullStateResponse { mutations: vec![], next_cursor: request.cursor, has_more: false }
            }
            _ => panic!("reconciliation must finish without a publication loop"),
        };
        binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull })
    }

    let state = ExchangeState(Arc::new(AtomicUsize::new(0)));
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
        // Fresh reading positions are eligible in this cycle, even across
        // multiple push pages; there is no separate reading quiet period.
        let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap().as_millis() as u64;
        for sequence in 1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 1 {
            let mutation = reading_change(sequence, sequence, "epubcfi(/6/2)", now);
            manager.queue_outbox_reading_change(&mutation, sequence);
        }
    }

    let original = manager.database.sync_publishable_mutations().unwrap().into_iter().find(|m| m.state.value.address().entity_key == fixture_content_hash(1).to_string()).unwrap();
    let result = manager.state_engine().unwrap().exchange_events_with_server(SyncCursor::default()).await.unwrap();

    assert!(result.mutation_issues.is_empty());
    assert_eq!(state.0.load(Ordering::SeqCst), 2);
    let conn = &manager.database;
    let reconciliation = conn.sync_publishable_mutations().unwrap();
    assert_eq!(reconciliation.len(), 1);
    assert_eq!(reconciliation[0].state.value, original.state.value);
    assert_eq!(reconciliation[0].state.version.changed_at, original.state.version.changed_at);
    assert_eq!(reconciliation[0].state.version, original.state.version);
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(2).ok()));
    manager.state_engine().unwrap().exchange_events_with_server(conn.sync_pull_cursor().unwrap()).await.unwrap();
    assert_eq!(state.0.load(Ordering::SeqCst), 3);
    assert!(conn.sync_publishable_mutations().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn transient_rejection_does_not_starve_later_push_pages() {
    #[derive(Clone)]
    struct ExchangeState {
        calls: Arc<AtomicUsize>,
        rejected: Arc<Mutex<Option<sync_common::ReplicaSeq>>>,
    }
    async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        let request = decode_sync_exchange(&headers, &body);
        let call = state.calls.fetch_add(1, Ordering::SeqCst);
        let push = if call == 0 {
            assert_eq!(request.mutations.len(), sync_common::MAX_PUSH_MUTATIONS);
            let rejected = request.mutations[0].publisher_seq;
            *state.rejected.lock().unwrap() = Some(rejected);
            PushMutationsResponse {
                rejected: vec![MutationRejection { publisher_seq: rejected, reason: MutationRejectionReason::TimestampTooFarFuture }],
            }
        } else {
            assert_eq!(request.mutations.len(), 1);
            PushMutationsResponse { rejected: Vec::new() }
        };
        binary_sync_response(&SyncExchangeResponse { push, pull: PullStateResponse { mutations: Vec::new(), next_cursor: request.cursor, has_more: false } })
    }

    let state = ExchangeState { calls: Arc::new(AtomicUsize::new(0)), rejected: Arc::new(Mutex::new(None)) };
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
        for sequence in 1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 1 {
            let mutation = reading_change(sequence, sequence, "epubcfi(/6/2)", sequence);
            manager.queue_outbox_reading_change(&mutation, sequence);
        }
    }

    let exchange = manager.state_engine().unwrap().exchange_events_with_server(SyncCursor::default()).await.expect("a deferred page still completes the exchange");

    assert_eq!(state.calls.load(Ordering::SeqCst), 2);
    let rejected = state.rejected.lock().unwrap().unwrap();
    assert_eq!(exchange.mutation_issues, vec![sync_common::MutationIssue::Retryable(MutationRejection { publisher_seq: rejected, reason: MutationRejectionReason::TimestampTooFarFuture })]);
    let conn = &manager.database;
    let pending = conn.sync_publishable_mutations().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].publisher_seq, rejected);
    server.abort();
}

#[tokio::test]
async fn a_deferred_mutation_does_not_fail_the_surrounding_synchronization() {
    #[derive(Clone)]
    struct ExchangeState {
        deferred: Arc<Mutex<Option<sync_common::ReplicaSeq>>>,
    }
    async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
        let request = decode_sync_exchange(&headers, &body);
        assert_eq!(request.mutations.len(), 2, "both queued positions travel in one page");
        let deferred = request.mutations[0].publisher_seq;
        *state.deferred.lock().unwrap() = Some(deferred);
        binary_sync_response(&SyncExchangeResponse {
            push: PushMutationsResponse { rejected: vec![MutationRejection { publisher_seq: deferred, reason: MutationRejectionReason::TimestampTooFarFuture }] },
            pull: pull_response(vec![pulled_change(1, added(1, 10))], LibraryRevision::new(1).ok(), false),
        })
    }

    let state = ExchangeState { deferred: Arc::new(Mutex::new(None)) };
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = test_shared_credentials(&manager.server_url);
    for sequence in 1..=2 {
        manager.queue_outbox_reading_change(&reading_change(sequence, sequence, "epubcfi(/6/2)", sequence), sequence);
    }

    let (events, updates) = async_channel::unbounded();

    manager.event_tx = Some(events);
    let outcome = manager.synchronize_state().await;

    // Side effects first, so this test reports what the exchange actually
    // did before it reports how it answered.
    let conn = &manager.database;
    let pending = conn.sync_publishable_mutations().unwrap();
    assert_eq!(pending.len(), 1, "the deferred mutation stays queued for a later attempt");
    assert_eq!(pending[0].publisher_seq, state.deferred.lock().unwrap().unwrap());
    assert_eq!(conn.sync_pull_cursor().unwrap(), cursor(LibraryRevision::new(1).ok()), "the pulled page is still applied");

    assert!(std::iter::from_fn(|| updates.try_recv().ok()).any(|event| matches!(event, LibraryEvent::ContentsChanged)), "remote state must invalidate ordinary library views");
    let outcome = outcome.expect("one deferred mutation must not fail the synchronization");
    assert_eq!(outcome.mutation_issues, vec![sync_common::MutationIssue::Retryable(MutationRejection { publisher_seq: state.deferred.lock().unwrap().unwrap(), reason: MutationRejectionReason::TimestampTooFarFuture })]);
    assert!(conn.sync_metadata("last_successful_sync_at").unwrap().is_some(), "the exchange completed, so the sync is checkpointed");
    server.abort();
}

#[tokio::test]
async fn permanent_rejection_removes_poison_mutation_from_durable_outbox() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    let requested = reading_change(1, 1, "epubcfi(/6/24)", 1);
    {
        let conn = &manager.database;
        conn.seed_poison_outbox_mutation(requested.publisher_seq.get() as i64, &sync_common::wire::encode(&requested.state.value).unwrap(), i64::try_from(requested.state.version.changed_at).unwrap())
            .unwrap();
    }
    let response = PushMutationsResponse { rejected: vec![MutationRejection { publisher_seq: requested.publisher_seq, reason: MutationRejectionReason::InvalidPayload }] };

    assert!(manager.state_engine().unwrap().apply_push_response(&[requested.publisher_seq], &response).await.is_err());

    let conn = &manager.database;
    assert!(conn.sync_publishable_mutations().unwrap().is_empty());
}

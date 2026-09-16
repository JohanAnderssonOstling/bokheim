use super::*;

#[tokio::test]
async fn account_switch_rejects_an_in_flight_native_page_and_acknowledgments() {
    for queued in [false, true] {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let observed = started.clone();
        let released = release.clone();
        let app = axum::Router::new().route(
            "/api/sync/exchange",
            axum::routing::post(move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let started = observed.clone();
                let release = released.clone();
                async move {
                    let request = decode_sync_exchange(&headers, &body);
                    started.notify_one();
                    release.notified().await;
                    binary_sync_response(&SyncExchangeResponse {
                        push: PushMutationsResponse { accepted: request.mutations.iter().map(|m| m.mutation_id).collect(), rejected: Vec::new() },
                        pull: pull_response(vec![pulled_change(1, added(99, "Old account book", 10))], LibraryRevision::new(1).ok(), false),
                    })
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        manager.credentials = Arc::new(std::sync::RwLock::new(Some(test_credentials(&manager.server_url))));
        client_library::sync_state::set_sync_metadata(&manager.database.get().unwrap(), "bound_user_id", "old-user").unwrap();
        manager
            .database
            .get()
            .unwrap()
            .execute(include_str!("../../sql/library_sync/account_switch_rejects_an_in_flight_native_page_and_acknowledgments_insert.sql"), [fixture_dir_id("queued-account-edit").to_string(), crate::ROOT_DIR_ID.to_string()])
            .unwrap();
        let pending = client_library::sync_state::get_pending_mutations(&manager.database.get().unwrap()).unwrap().len();
        assert!(pending > 0);
        if !queued {
            manager.database.get().unwrap().execute(include_str!("../../sql/library_sync/exchange_delete.sql"), []).unwrap();
        }
        let pending = if queued { pending } else { 0 };
        let (events, updates) = crate::library::events::library_event_channel();
        manager.database.observe_remote_pages(move || {
            let _ = events.try_send(LibraryEvent::ContentsChanged);
        });
        let exchange = manager.synchronize_state();
        tokio::pin!(exchange);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! { result = &mut exchange => panic!("request must remain held: {result:?}"), _ = started.notified() => {} }
        })
        .await
        .unwrap();
        client_library::sync_state::rebind_sync_account(&manager.database.get().unwrap(), "new-user").unwrap();
        release.notify_one();
        assert!(tokio::time::timeout(std::time::Duration::from_secs(5), &mut exchange).await.unwrap().is_err());
        let conn = manager.database.get().unwrap();
        assert_eq!(client_library::sync_state::get_pending_mutations(&conn).unwrap().len(), pending, "old acknowledgments must not consume the new binding's outbox");
        assert_eq!(client_library::sync_state::get_pull_cursor(&conn).unwrap(), SyncCursor::default());
        assert_eq!(conn.query_row(include_str!("../../sql/library_sync/account_switch_rejects_an_in_flight_native_page_and_acknowledgments_select.sql"), [fixture_content_hash(99).as_str()], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert!(updates.try_recv().is_err());
        server.abort();
    }
}

#[tokio::test]
async fn unauthorized_sync_response_preserves_credentials_for_account_level_refresh() {
    let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(|| async { axum::http::StatusCode::UNAUTHORIZED }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
    manager.credentials = Arc::new(std::sync::RwLock::new(Some(test_credentials(&manager.server_url))));

    let error = manager.sync_once().await.unwrap_err();

    assert!(matches!(error, SyncError::AuthenticationRequired));
    assert!(manager.is_signed_in());
    server.abort();
}

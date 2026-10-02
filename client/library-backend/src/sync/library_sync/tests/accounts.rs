use super::*;

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
    manager.credentials = test_shared_credentials(&manager.server_url);

    let error = manager.sync_once().await.unwrap_err();

    assert!(matches!(error, SyncError::AuthenticationRequired));
    assert!(manager.is_signed_in());
    server.abort();
}

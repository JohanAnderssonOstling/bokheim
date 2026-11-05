#[tokio::test]
async fn native_library_client_retains_host_without_app_client() {
    let root = tempfile::tempdir().unwrap();
    let context = crate::BackendContext::initialize(crate::AppDataLocation::native_path(root.path())).unwrap();
    let backend = crate::app::AppBackend::new(context).await.unwrap();
    let id = *backend.library_service.ensure_import_library().await.unwrap().library_id();
    let lifetime = std::sync::Arc::downgrade(&backend);
    let app = crate::hosting::client(backend, None);
    let library = app.management.library(id).unwrap();
    drop(app);
    assert!(lifetime.upgrade().is_some());
    assert_eq!(library.book_count().await.unwrap(), 0);
    drop(library);
    assert!(lifetime.upgrade().is_none());
}

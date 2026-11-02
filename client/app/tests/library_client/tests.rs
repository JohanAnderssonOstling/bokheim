#[tokio::test]
async fn native_library_client_retains_worker_without_app_client() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let root = tempfile::tempdir().unwrap();
    let context = crate::BackendContext::initialize(crate::AppDataLocation::native_path(root.path())).unwrap();
    let backend = crate::app::AppBackend::new(context).unwrap();
    let id = *backend.state.library_service.ensure_import_library().unwrap().library_id();
    let (app, worker) = crate::runtime::native::channel(&backend);
    let library = app.management.library(id);
    drop(app);
    let stopped = Arc::new(AtomicBool::new(false));
    let finished = stopped.clone();
    let serve = async move {
        worker.serve(backend).await;
        finished.store(true, Ordering::SeqCst);
    };
    let requests = async move {
        tokio::task::yield_now().await;
        assert!(!stopped.load(Ordering::SeqCst));
        assert_eq!(library.book_count().await.unwrap(), 0);
        assert!(!stopped.load(Ordering::SeqCst));
        drop(library);
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(serve, requests);
    })
    .await
    .unwrap();
}

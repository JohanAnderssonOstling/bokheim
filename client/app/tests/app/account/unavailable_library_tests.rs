use super::*;
use crate::app::data::AppDataLocation;
use library_backend::LibrarySyncState;

#[tokio::test]
async fn unavailable_library_does_not_block_account_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let library = backend.library_service.create_with_asset_storage("Unavailable".to_owned(), false).await.unwrap();
    let id = *library.library_id();
    backend.library_service.retire(&id);

    let snapshot = backend.account_service.account_snapshot().await.unwrap();
    let status = snapshot.sync_statuses.iter().find(|status| status.library_id == id).unwrap();
    assert_eq!(status.state, LibrarySyncState::Unavailable);
    assert!(!status.asset_storage_enabled);
    assert!(backend.library_service.local_storage_usage().await.unwrap().iter().all(|usage| usage.library_id != id));
}

#[tokio::test]
async fn discovery_reopens_an_existing_registered_library() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let library = backend.library_service.create_with_asset_storage("Existing".to_owned(), true).await.unwrap();
    let id = *library.library_id();
    backend.library_service.retire(&id);

    let remote = [sync_common::api::libraries::LibrarySummary { library_id: id, library_name: "Existing".to_owned(), storage: Default::default() }];
    assert!(backend.library_service.attach_remote_libraries(&remote).await.unwrap().contains(&id));
    assert!(backend.library_service.handle(&id).is_ok());
}

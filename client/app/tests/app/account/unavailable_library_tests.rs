use super::*;
use crate::app::data::AppDataLocation;
use library_backend::LibrarySyncState;

#[tokio::test]
async fn unavailable_library_does_not_block_account_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
    let library = backend.state.library_service.create_with_asset_storage("Unavailable".to_owned(), false).unwrap();
    let id = *library.library_id();
    backend.state.library_directory.retire(&id).unwrap();

    let snapshot = backend.state.account_service.account_snapshot().await.unwrap();
    let status = snapshot.sync_statuses.iter().find(|status| status.library_id == id).unwrap();
    assert_eq!(status.state, LibrarySyncState::Unavailable);
    assert!(!status.asset_storage_enabled);
    assert!(backend.state.library_service.local_storage_usage().await.unwrap().iter().all(|usage| usage.library_id != id));
}

#[tokio::test]
async fn discovery_reopens_an_existing_registered_library() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
    let library = backend.state.library_service.create_with_asset_storage("Existing".to_owned(), true).unwrap();
    let id = *library.library_id();
    backend.state.library_directory.retire(&id).unwrap();

    let remote = [sync_common::api::libraries::LibrarySummary { library_id: id, library_name: "Existing".to_owned(), storage: Default::default(), kind: Default::default() }];
    assert!(backend.state.library_service.attach_remote_libraries(&remote).unwrap().contains(&id));
    assert!(backend.state.library_directory.try_get(&id).unwrap().is_some());
}

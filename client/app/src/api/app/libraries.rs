use super::AppClient;
use crate::executor::run_blocking;
use crate::runtime::AppCommand;
use crate::{BackendError, LibraryEntry, LibraryFolderError};
use library_backend::library::discovery::estimate_folders;
use std::sync::atomic::AtomicBool;
use sync_common::LibraryId;

impl AppClient {
    pub async fn copy_library_storage(&self, library_id: LibraryId, bokheim: bool) -> Result<LibraryEntry, BackendError> {
        self.request(AppCommand::CopyLibraryStorage { library_id, bokheim }).await
    }

    pub async fn set_library_asset_storage(&self, library_id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        self.request(AppCommand::SetLibraryAssetStorage { library_id, enabled }).await
    }

    pub async fn estimate_library_folders(&self, locators: Vec<String>) -> Result<(u64, u64, u64), BackendError> {
        self.services.executor.spawn(async move { run_blocking(move || estimate_folders(locators, &AtomicBool::new(false))).await.map_err(BackendError::operation)? }).await.map_err(BackendError::operation)?
    }

    pub async fn create_libraries_with_asset_storage(&self, locators: Vec<String>, asset_storage_enabled: bool) -> Result<(Vec<LibraryId>, Vec<LibraryFolderError>), BackendError> {
        self.request(AppCommand::CreateLibrariesWithAssetStorage { locators, asset_storage_enabled }).await
    }

    pub async fn rename_library(&self, library_id: LibraryId, name: String) -> Result<(), BackendError> {
        self.request(AppCommand::RenameLibrary { library_id, name }).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn move_library(&self, library_id: LibraryId, destination: String) -> Result<LibraryEntry, BackendError> {
        self.request(AppCommand::MoveLibrary { library_id, destination }).await
    }

    pub async fn delete_library(&self, library_id: LibraryId) -> Result<(), BackendError> {
        self.request(AppCommand::DeleteLibrary { library_id }).await
    }
}

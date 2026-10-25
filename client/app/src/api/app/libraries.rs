use super::AppClient;
use crate::{BackendError, LibraryEntry};
use crate::runtime::AppCommand;
use sync_common::LibraryId;
use tokio::sync::watch;

impl AppClient {
    /// Subscribe before reading the initial list. Unread changes coalesce;
    /// await `changed()` and fetch `libraries()` for the current entries.
    pub async fn library_list_changes(&self) -> Result<watch::Receiver<()>, BackendError> {
        self.transport.library_list_changes_transport().await
    }

    pub async fn libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.request(AppCommand::Libraries).await
    }

    /// Resolve a destination only when the user imports or downloads a book.
    pub async fn library_for_import(&self, selected: Option<LibraryId>) -> Result<library_backend::LibraryClient, BackendError> {
        let explicitly_selected = selected.is_some();
        let id = match selected {
            Some(id) => id,
            None => {
                let library: LibraryEntry = self.request(AppCommand::EnsureImportLibrary).await?;
                *library.library_id()
            }
        };
        let library = self.library(id);
        if explicitly_selected {
            library.book_count().await.map_err(|error| error.context("selected library is unavailable"))?;
        }
        Ok(library)
    }

    pub async fn set_library_asset_storage(&self, library_id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        self.request(AppCommand::SetLibraryAssetStorage { library_id, enabled }).await
    }

    pub async fn estimate_library_folders(&self, locators: Vec<String>) -> Result<(u64, u64, u64), BackendError> {
        self.request(AppCommand::EstimateLibraryFolders { locators }).await
    }

    pub async fn create_library_with_asset_storage(&self, name: String, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        self.request(AppCommand::CreateLibraryWithAssetStorage { name, asset_storage_enabled }).await
    }

    pub async fn create_libraries_with_asset_storage(&self, locators: Vec<String>, asset_storage_enabled: bool) -> Result<(Vec<LibraryId>, Vec<BackendError>), BackendError> {
        self.request(AppCommand::CreateLibrariesWithAssetStorage { locators, asset_storage_enabled }).await
    }

    pub async fn rename_library(&self, library_id: LibraryId, name: String) -> Result<(), BackendError> {
        self.request(AppCommand::RenameLibrary { library_id, name }).await
    }

    pub async fn delete_library(&self, library_id: LibraryId) -> Result<(), BackendError> {
        self.request(AppCommand::DeleteLibrary { library_id }).await
    }
}

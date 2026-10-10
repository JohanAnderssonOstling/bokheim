use super::AppClient;
use crate::LibraryEntry;
use crate::runtime::AppCommand;
use sync_common::LibraryId;
use tokio::sync::watch;

impl AppClient {
    /// Subscribe before reading the initial list. Unread changes coalesce;
    /// await `changed()` and fetch `libraries()` for the current entries.
    pub async fn library_list_changes(&self) -> Result<watch::Receiver<()>, String> {
        self.transport.library_list_changes_transport().await.map_err(|error| error.to_string())
    }

    pub async fn libraries(&self) -> Result<Vec<LibraryEntry>, String> {
        self.request(AppCommand::Libraries).await
    }

    /// Resolve a destination only when the user imports or downloads a book.
    pub async fn library_for_import(&self, selected: Option<LibraryId>) -> Result<library_backend::LibraryClient, String> {
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
            library.book_count().await.map_err(|error| format!("selected library is unavailable: {error}"))?;
        }
        Ok(library)
    }

    pub async fn set_library_asset_storage(&self, library_id: LibraryId, enabled: bool) -> Result<(), String> {
        self.request(AppCommand::SetLibraryAssetStorage { library_id, enabled }).await
    }

    pub async fn estimate_library_folders(&self, locators: Vec<String>) -> Result<(u64, u64, u64), String> {
        self.request(AppCommand::EstimateLibraryFolders { locators }).await
    }

    pub async fn create_library_with_asset_storage(&self, name: String, asset_storage_enabled: bool) -> Result<LibraryEntry, String> {
        self.request(AppCommand::CreateLibraryWithAssetStorage { name, asset_storage_enabled }).await
    }

    pub async fn create_libraries_with_asset_storage(&self, locators: Vec<String>, asset_storage_enabled: bool) -> Result<(Vec<LibraryId>, Vec<String>), String> {
        self.request(AppCommand::CreateLibrariesWithAssetStorage { locators, asset_storage_enabled }).await
    }

    pub async fn rename_library(&self, library_id: LibraryId, name: String) -> Result<(), String> {
        self.request(AppCommand::RenameLibrary { library_id, name }).await
    }

    pub async fn delete_library(&self, library_id: LibraryId) -> Result<(), String> {
        self.request(AppCommand::DeleteLibrary { library_id }).await
    }
}

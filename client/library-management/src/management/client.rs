//! Direct async access to shared library management services.
use super::{LibraryFolderError, LibraryService, LocalLibraryStorageUsage};
use crate::executor::BackendExecutor;
use crate::{BackendError, LibraryClient, LibraryId};
use library_backend::ThumbnailResolution;
#[cfg(not(target_arch = "wasm32"))]
use library_backend::{ReaderImportSource, ReaderLibrarySummary};
use library_model::CardKey;
use library_registry::LibraryEntry;
use library_services::interest::InterestLease;
use std::sync::Arc;
use tokio::sync::watch;

pub struct NotificationLease {
    _interest: InterestLease,
    _client: ManagementClient,
}

#[derive(Clone)]
pub struct ManagementClient {
    service: Arc<LibraryService>,
    _host: Arc<dyn Send + Sync>,
}

impl ManagementClient {
    pub fn local(service: Arc<LibraryService>, host: Arc<dyn Send + Sync>) -> Self {
        Self { service, _host: host }
    }
    /// Share the app's I/O runtime with device-local services.
    pub fn executor(&self) -> &BackendExecutor {
        &self.service.config.executor
    }
    pub fn library(&self, id: LibraryId) -> Result<LibraryClient, BackendError> {
        let handle = self.service.handle(&id)?;
        Ok(LibraryClient::new(handle, Arc::new(self.clone())))
    }
    pub async fn library_for_import(&self, selected: Option<LibraryId>) -> Result<LibraryClient, BackendError> {
        let explicitly_selected = selected.is_some();
        let id = match selected {
            Some(id) => id,
            None => *self.ensure_import_library().await?.library_id(),
        };
        let library = self.library(id).map_err(|error| if explicitly_selected { error.context("selected library is unavailable") } else { error })?;
        if explicitly_selected {
            library.book_count().await.map_err(|error| error.context("selected library is unavailable"))?;
        }
        Ok(library)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn reader_import_summary(&self, source: ReaderImportSource) -> Result<ReaderLibrarySummary, BackendError> {
        self.executor().with_io_context(library_import::reader_import::reader_import_summary(source)).await
    }
    pub fn evict_cover_caches(&self) {
        library_catalog::evict_cover_caches();
    }
    pub async fn external_catalog_cover(&self, entry: CardKey, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        self.executor()
            .with_io_context(async {
                Ok(match resolution {
                    ThumbnailResolution::HighDensity => library_catalog::external_cover(entry, true).await,
                    ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity => library_catalog::card_cover(entry, resolution.width()).await,
                })
            })
            .await
    }
    pub async fn libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.executor().with_io_context(self.service.list()).await
    }
    pub async fn create_library_with_asset_storage(&self, name: String, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        self.executor().with_io_context(self.service.create_with_asset_storage(name, asset_storage_enabled)).await
    }
    pub async fn ensure_import_library(&self) -> Result<LibraryEntry, BackendError> {
        self.executor().with_io_context(self.service.ensure_import_library()).await
    }
    pub async fn local_storage_usage(&self) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        self.executor().with_io_context(self.service.local_storage_usage()).await
    }
    pub async fn text_search_index_bytes(&self) -> Result<u64, BackendError> {
        self.executor().with_io_context(self.service.text_search_index_bytes()).await
    }
    pub async fn estimate_library_folders(&self, locators: Vec<String>) -> Result<(u64, u64, u64), BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::executor::run_blocking;
            use filesystem_scanner::estimate::count_books;
            use std::sync::atomic::AtomicBool;
            self.executor().with_io_context(run_blocking(move || count_books(locators, &AtomicBool::new(false)))).await.map_err(BackendError::operation)?.ok_or_else(|| BackendError::message("Folder estimate cancelled"))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = locators;
            Err("Folder size estimation is unavailable on this device".into())
        }
    }
    pub async fn create_libraries_with_asset_storage(&self, locators: Vec<String>, asset_storage_enabled: bool) -> Result<(Vec<LibraryId>, Vec<LibraryFolderError>), BackendError> {
        self.executor().with_io_context(self.service.create_libraries_from_paths_with_asset_storage(locators.into_iter().map(std::path::PathBuf::from).collect(), asset_storage_enabled)).await
    }
    pub async fn set_library_asset_storage(&self, library_id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        self.executor().with_io_context(self.service.set_library_asset_storage(library_id, enabled)).await
    }
    pub async fn rename_library(&self, library_id: LibraryId, name: String) -> Result<(), BackendError> {
        self.executor().with_io_context(self.service.rename_library(&library_id, &name)).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn move_library(&self, library_id: LibraryId, destination: String) -> Result<LibraryEntry, BackendError> {
        self.executor().with_io_context(self.service.move_library(library_id, destination)).await
    }
    pub async fn delete_library(&self, library_id: LibraryId) -> Result<(), BackendError> {
        self.executor().with_io_context(self.service.delete_library(&library_id)).await
    }
    pub async fn copy_library_storage(&self, library_id: LibraryId, bokheim: bool) -> Result<LibraryEntry, BackendError> {
        self.executor().with_io_context(self.service.copy_library_storage(library_id, bokheim)).await
    }
    pub fn library_list_changes(&self) -> watch::Receiver<()> {
        self.service.list_changes()
    }
    pub fn notification_interest(&self, browsing: bool) -> NotificationLease {
        NotificationLease { _interest: self.service.notification_interest(browsing), _client: self.clone() }
    }
}

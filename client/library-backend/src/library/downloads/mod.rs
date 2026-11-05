//! Download requests and downloaded-copy eviction.

mod browse;

pub use browse::LibraryBookStorage;

use super::events::LibraryEvent;
use super::{LibraryHandle, LibraryResources};
use crate::asset_store::AssetStore;
use crate::{BackendError, BookId, DirId, DownloadState};
use library_database::DatabaseClient;
use library_model::{LibraryBrowseQuery, LibraryBusyReason};
use library_placement::LibraryState;

impl LibraryResources {
    pub(crate) async fn current_download_state(&self, storage: &LibraryState, book_id: BookId) -> Result<DownloadState, BackendError> {
        let durable = persistent_download_state(storage.database(), storage.assets(), book_id).await?;
        let observed = self.events.updates.download_state(&book_id);
        Ok(match (&durable, observed) {
            (DownloadState::Queued, Some(state @ DownloadState::Downloading(_))) => state,
            _ => durable,
        })
    }

    pub(crate) async fn queue_download(&self, storage: &LibraryState, book_id: BookId) -> Result<DownloadState, BackendError> {
        let state = self.current_download_state(storage, book_id).await?;
        if matches!(state, DownloadState::Downloaded | DownloadState::Downloading(_)) {
            return Ok(state);
        }
        if !self.sync.is_signed_in().await {
            return Err(BackendError::message("sign in before downloading this book"));
        }
        self.events.updates.reset_download(&book_id);
        self.request_downloads(&[book_id]).await?;
        Ok(DownloadState::Queued)
    }
}

impl LibraryHandle {
    pub async fn download_state(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            Ok(library.current_download_state(&storage, book_id).await?)
        })
        .await
    }

    pub async fn download_book(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            Ok(library.queue_download(&storage, book_id).await?)
        })
        .await
    }

    pub async fn download_browse(&self, query: LibraryBrowseQuery) -> Result<(), BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            Ok(library.queue_browse_downloads(&storage, query).await?)
        })
        .await
    }

    pub async fn cloud_quota_books(&self) -> Result<Vec<library_model::CloudQuotaBook>, BackendError> {
        let database = self.database();
        Ok(database.cloud_quota_books().await?)
    }

    pub async fn remove_local_book_copy(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        self.run_io(async {
            let storage = self.storage().await?;
            let library = &self.owner.resources;
            Ok(library.remove_local_book_copy(&storage, book_id).await?)
        })
        .await
    }

    pub async fn evict_folder_downloads(&self, directory_id: DirId) -> Result<usize, BackendError> {
        self.run_io(async {
            let storage = self.storage().await?;
            let library = &self.owner.resources;
            Ok(library.evict_folder_downloads(&storage, &directory_id).await?)
        })
        .await
    }

    pub async fn book_storage(&self, for_download: bool) -> Result<crate::LibraryBookStorage, BackendError> {
        self.run_io(async {
            let _files = self.storage().await?;
            let library = &self.owner.resources;
            library.library_book_storage(for_download).await
        })
        .await
    }

    pub async fn download_all_books(&self, confirmed_books: Vec<BookId>) -> Result<(), BackendError> {
        self.run_io(async {
            let _activity = self.owner.resources.activity.start(Some(LibraryBusyReason::DownloadingBooks));
            let _files = self.storage().await?;
            let library = &self.owner.resources;
            Ok(library.download_books_locally(&confirmed_books).await?)
        })
        .await
    }

    pub async fn local_storage_usage(&self) -> Result<Option<u64>, BackendError> {
        self.run_io(async {
            let storage = self.storage().await?;
            #[cfg(not(target_arch = "wasm32"))]
            let paths = storage.database().local_book_paths().await?;
            let assets = storage.assets().clone();
            #[cfg(not(target_arch = "wasm32"))]
            let bytes = assets.book_bytes_at_paths(paths).await;
            #[cfg(target_arch = "wasm32")]
            let bytes = assets.local_book_bytes().await;
            bytes.map(Some).map_err(BackendError::operation)
        })
        .await
    }
}

impl LibraryResources {
    pub(crate) async fn request_downloads(&self, hashes: &[BookId]) -> Result<(), BackendError> {
        if hashes.is_empty() {
            return Ok(());
        }
        if cfg!(target_arch = "wasm32") {
            return Err(BackendError::message("this library streams books in the browser"));
        }

        self.transfers.request_downloads(hashes).await.map_err(BackendError::operation)?;
        self.events.emit(LibraryEvent::RefreshFileActivity);
        self.events.assets.wake();
        Ok(())
    }

    pub(crate) fn check_book_eviction(&self) -> Result<(), BackendError> {
        if !self.transfers.asset_storage_enabled() {
            return Err(BackendError::message("cloud book storage is off; keep the local downloads"));
        }
        Ok(())
    }

    /// Evicts downloaded book files without deleting the library book,
    /// its placements, reading state, or relationships.
    pub(crate) async fn remove_local_book_copy(&self, storage: &LibraryState, book_id: BookId) -> Result<DownloadState, BackendError> {
        if self.evict_local_book(storage, book_id).await? {
            Ok(DownloadState::NotDownloaded)
        } else {
            persistent_download_state(storage.database(), storage.assets(), book_id).await
        }
    }

    /// Shared policy, completion, and notifications for single and bulk removal.
    pub(super) async fn evict_local_book(&self, storage: &LibraryState, book_id: BookId) -> Result<bool, BackendError> {
        let _eviction = self.evictions.read().await;
        self.check_book_eviction()?;
        if !library_placement::evict_local_book(storage.assets(), storage.database(), &book_id).await? {
            return Ok(false);
        }
        self.events.download_status_changed(book_id, DownloadState::NotDownloaded);
        self.events.notify_contents_changed();
        Ok(true)
    }
}

pub(super) async fn persistent_download_state(db: &DatabaseClient, assets: &AssetStore, book_id: BookId) -> Result<DownloadState, BackendError> {
    let paths = db.book_paths(&book_id).await?;
    let bytes_present = assets.book_exists(&book_id, &paths).await.map_err(BackendError::operation)?;
    Ok(db.transfer_download_state(&book_id, bytes_present).await?)
}

//! Book file state on the session: download state, thumbnails, local eviction.

use super::events::LibraryEvent;
pub(crate) use super::LibrarySession;
pub(crate) use crate::BackendError;
use crate::ThumbnailResolution;
pub(crate) use crate::{BookId, DownloadState};
use client_platform_runtime::startup_profile::StartupProfile;
pub(crate) use library_model::BlobKind;

impl LibrarySession {
    pub(crate) fn check_book_eviction(&self) -> Result<(), BackendError> {
        self.runtime.check_book_eviction()?;
        if !self.is_opds() && !self.sync.asset_storage_enabled() {
            return Err(BackendError::message("cloud book storage is off; keep the local downloads"));
        }
        Ok(())
    }

    pub(crate) fn persistent_download_state(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        let mut profile = StartupProfile::new("book_lookup_availability");
        let paths = self.db.book_paths(&book_id)?;
        profile.mark("book_paths");
        let bytes_present = self.assets.book_exists(&book_id, &paths).map_err(BackendError::operation)?;
        profile.mark("filesystem_exists");
        let state = self.db.transfer_download_state(&book_id, bytes_present).map_err(BackendError::operation);
        profile.mark("download_flags_sql");
        state
    }

    pub(crate) async fn thumbnail(&self, book_id: BookId, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.is_opds() {
            return self.opds_cover(library_model::CardKey(*book_id.identity_digest()), resolution.width(), resolution != ThumbnailResolution::HighDensity).await;
        }
        if matches!(resolution, ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity) {
            if let Some(bytes) = self.assets.read_named(library_files::asset_store::thumbnail_variant_name(&book_id, resolution.width())).await.map_err(BackendError::operation)? {
                return Ok(Some(bytes));
            }
        }
        // Existing libraries may only have the hash-addressed master. Load it
        // directly rather than hiding the cover until a card variant is rebuilt.
        if let Some(bytes) = self.assets.read_bytes(BlobKind::Thumbnail, &book_id).await.map_err(BackendError::operation)? {
            return Ok(Some(bytes));
        }

        // The origin also addresses cover images directly by hash.
        Ok(match resolution {
            ThumbnailResolution::HighDensity => library_catalog::cover(book_id, true).await,
            ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity => library_catalog::card_cover(library_model::CardKey::from(book_id), resolution.width()).await,
        })
    }

    /// Evicts downloaded book files without deleting the library book,
    /// its placements, reading state, or relationships.
    pub(crate) async fn remove_local_book_copy(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        let _activity = self.book_storage_activity()?;
        if self.evict_local_book(book_id).await? {
            Ok(DownloadState::NotDownloaded)
        } else {
            self.persistent_download_state(book_id)
        }
    }

    /// Shared policy, completion, and notifications for single and bulk removal.
    pub(super) async fn evict_local_book(&self, book_id: BookId) -> Result<bool, BackendError> {
        self.check_book_eviction()?;
        if !library_placement::evict_local_book(&self.assets, &self.db, &book_id).await? {
            return Ok(false);
        }
        self.emit(LibraryEvent::DownloadStatusChanged { book_id, state: DownloadState::NotDownloaded });
        self.events.notify_contents_changed();
        Ok(true)
    }
}

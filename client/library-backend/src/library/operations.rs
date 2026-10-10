//! Book file state on the session: download state, thumbnails, local eviction.

pub(crate) use super::LibrarySession;
pub(crate) use crate::BackendError;
use crate::ThumbnailResolution;
pub(crate) use crate::{ContentHash, DownloadState};
pub(crate) use library_model::BlobKind;

impl LibrarySession {
    pub(crate) fn persistent_download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let bytes_present = self.assets.book_exists(&content_hash, &self.db.book_paths(&content_hash)?).map_err(BackendError::operation)?;
        self.db.transfer_download_state(&content_hash, bytes_present).map_err(BackendError::operation)
    }

    pub(crate) async fn thumbnail(&self, content_hash: ContentHash, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        let bytes = match resolution {
            ThumbnailResolution::Browse => self.assets.read_named(library_files::asset_store::thumbnail_variant_name(&content_hash, resolution.width())).await.map_err(BackendError::operation)?,
            ThumbnailResolution::HighDensity => self.assets.read_bytes(BlobKind::Thumbnail, &content_hash).await.map_err(BackendError::operation)?,
        };
        if bytes.is_some() {
            return Ok(bytes);
        }
        if self.assets.book_exists(&content_hash, &self.db.book_paths(&content_hash)?).map_err(BackendError::operation)? && self.db.thumbnail_book_format(content_hash).map_err(BackendError::operation)?.is_some() {
            self.db.request_missing_thumbnail(&content_hash).map_err(BackendError::operation)?;
            return Ok(None);
        }
        // A book the library does not hold may be in the catalog.
        Ok(library_catalog::cover(content_hash, matches!(resolution, ThumbnailResolution::HighDensity)).await)
    }

    /// Evicts downloaded book files without deleting the library book,
    /// its placements, reading state, or relationships.
    pub(crate) fn remove_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        library_placement::evict_local_book(&self.assets, &self.db, &content_hash)?;
        let state = self.persistent_download_state(content_hash)?;
        self.emit(super::events::LibraryEvent::DownloadStatusChanged { content_hash, state: state.clone() });
        self.events.notify_contents_changed();
        Ok(state)
    }
}

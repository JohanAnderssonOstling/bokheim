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
        if matches!(resolution, ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity) {
            if let Some(bytes) = self.assets.read_named(library_files::asset_store::thumbnail_variant_name(&content_hash, resolution.width())).await.map_err(BackendError::operation)? {
                return Ok(Some(bytes));
            }
        }
        // Existing libraries may only have the hash-addressed master. Load it
        // directly rather than hiding the cover until a card variant is rebuilt.
        if let Some(bytes) = self.assets.read_bytes(BlobKind::Thumbnail, &content_hash).await.map_err(BackendError::operation)? {
            return Ok(Some(bytes));
        }
        // The origin also addresses cover images directly by hash.
        Ok(match resolution {
            ThumbnailResolution::HighDensity => library_catalog::cover(content_hash, true).await,
            ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity => library_catalog::card_cover(library_model::CardKey(*content_hash.as_bytes()), resolution.width(), &*self.cpu).await,
        })
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

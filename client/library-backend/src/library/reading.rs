//! Audiobook playback metadata cached in the library database.

use library_model::PlaybackMetadata;
use crate::library::LibrarySession;
use crate::ContentHash;

impl LibrarySession {
    pub(crate) async fn audiobook_playback_metadata(&self, hash: ContentHash) -> Result<Option<PlaybackMetadata>, String> {
        let mut metadata = self.db.audiobook_playback_metadata(&hash).map_err(|error| error.to_string())?;
        if let Some(metadata) = &mut metadata {
            metadata.cover = match self.assets.read_bytes(library_model::BlobKind::Thumbnail, &hash).await {
                Ok(cover) => cover.unwrap_or_default(),
                Err(error) => {
                    log::warn!("Cached audiobook cover unavailable: {error}");
                    Vec::new()
                }
            };
        }
        Ok(metadata)
    }
}

//! Audiobook playback metadata cached in the library database.

use crate::library::playback::PlaybackMetadata;
use crate::library::LibrarySession;
use crate::ContentHash;

impl LibrarySession {
    pub(crate) async fn audiobook_playback_metadata(&self, hash: ContentHash) -> Result<Option<PlaybackMetadata>, String> {
        let mut metadata = {
            let snapshot = self.db.audiobook_playback_snapshot(&hash).map_err(|error| error.to_string())?;
            let tracks = self.db.audiobook_tracks(&hash).map_err(|error| error.to_string())?;
            let format = self.db.book_format(&hash).map_err(|error| error.to_string())?.unwrap_or(book_model::BookFormat::M4b);
            snapshot.cached.map(|metadata| PlaybackMetadata { format, title: snapshot.title, author: snapshot.author, narrator: metadata.narrator, duration_ms: metadata.duration_ms, chapters: metadata.chapters, tracks, cover: Vec::new() })
        };
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

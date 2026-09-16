//! Native library-handle wrappers for shared reading-position persistence.

use crate::BackendError;
use crate::ContentHash;
use crate::library::LibrarySession;
use client_library::runtime_contracts::PlaybackMetadata;
use library_model::ReadProgress;

impl LibrarySession {
    pub(crate) fn book_format(&self, hash: ContentHash) -> Result<Option<library_model::BookFormat>, BackendError> {
        client_library::assets::get_book_format(&self.db.get()?, &hash).map_err(BackendError::operation)
    }

    pub(crate) fn cached_audiobook_metadata(&self, hash: ContentHash) -> Result<Option<PlaybackMetadata>, String> {
        let conn = self.db.get().map_err(|error| error.to_string())?;
        Ok(client_library::audiobooks::cached_playback_metadata(&conn, hash).map_err(|error| error.to_string())?.map(|metadata| PlaybackMetadata {
            title: metadata.title,
            author: metadata.author,
            narrator: metadata.narrator,
            duration_ms: metadata.duration_ms,
            chapters: metadata.chapters,
            cover: Vec::new(),
        }))
    }

    pub(crate) async fn audiobook_playback_metadata(&self, hash: ContentHash) -> Result<Option<PlaybackMetadata>, String> {
        let mut metadata = self.cached_audiobook_metadata(hash)?;
        if let Some(metadata) = &mut metadata {
            metadata.cover = match self.assets.read_bytes(crate::BlobKind::Thumbnail, &hash).await {
                Ok(cover) => cover.unwrap_or_default(),
                Err(error) => {
                    log::warn!("Cached audiobook cover unavailable: {error}");
                    Vec::new()
                }
            };
        }
        Ok(metadata)
    }
    pub fn reading_position(&self, content_hash: ContentHash) -> Result<Option<String>, BackendError> {
        let conn = self.db.get()?;
        Ok(client_library::reading::get_stored_reading_position(&conn, content_hash).map_err(BackendError::operation)?.map(|position| position.as_str()))
    }

    pub fn pdf_reading_position(&self, content_hash: ContentHash) -> Result<Option<(u32, f32)>, BackendError> {
        let conn = self.db.get()?;
        client_library::reading::get_pdf_reading_position(&conn, content_hash).map_err(BackendError::operation)
    }

    /// `entry` is the navigation entry the reader resolved the position to. It
    /// travels with the position because that is when it changes, and it is
    /// stored locally rather than synchronized: any device can recompute it
    /// from the position and the book's own navigation.
    pub fn update_reading_position(&self, content_hash: ContentHash, position: &str, progress: Option<f32>, entry: Option<&str>) -> Result<(), BackendError> {
        let progress = progress.map(ReadProgress::new).transpose().map_err(BackendError::operation)?;
        let conn = self.db.get()?;
        client_library::reading::update_reading_position(&conn, content_hash, position, progress).map_err(BackendError::operation)?;
        if let Some(entry) = entry.map(str::trim).filter(|entry| !entry.is_empty()) {
            client_library::toc::set_reading_entry(&conn, &content_hash, entry).map_err(BackendError::operation)?;
        }
        Ok(())
    }
}

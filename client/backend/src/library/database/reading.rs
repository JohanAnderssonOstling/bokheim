//! Native library-handle wrappers for shared reading-position persistence.

use super::*;
use crate::app::BackendError;
use crate::library::LibraryBackend;

impl LibraryBackend {
    pub fn reading_position(&self, content_hash: ContentHash) -> Result<Option<String>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        Ok(get_book_reading_position(&conn, content_hash).map_err(BackendError::operation)?.map(EpubCfi::into_string))
    }

    pub fn pdf_reading_page(&self, content_hash: ContentHash) -> Result<Option<u32>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_pdf_reading_page(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn pdf_reading_position(&self, content_hash: ContentHash) -> Result<Option<(u32, f32)>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_pdf_reading_position(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn audiobook_reading_position(&self, content_hash: ContentHash) -> Result<Option<u64>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_audiobook_reading_position(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn update_reading_position(&self, content_hash: ContentHash, position: &str, progress: Option<f32>) -> Result<(), BackendError> {
        let progress = progress.map(ReadProgress::new).transpose().map_err(BackendError::operation)?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::update_reading_position(&conn, content_hash, position, progress).map_err(BackendError::operation)?;
        self.app()?.request_reading_position_sync(self.id().clone());
        Ok(())
    }
}

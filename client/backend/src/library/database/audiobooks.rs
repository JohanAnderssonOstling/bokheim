//! Native library-handle wrapper for audiobook database operations.

use super::*;
use crate::app::BackendError;
use crate::library::LibraryBackend;

impl LibraryBackend {
    pub fn audiobook_chapters(&self, content_hash: ContentHash) -> Result<Vec<app_contract::library::AudiobookChapter>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::audiobook_chapters(&conn, &content_hash).map_err(BackendError::operation)
    }
}

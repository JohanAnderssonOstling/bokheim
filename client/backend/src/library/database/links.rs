//! Native library-handle wrappers for shared semantic-link persistence.

use super::*;
use crate::app::BackendError;
use crate::library::LibraryBackend;
use app_contract::semantic_links::BookLink;

impl LibraryBackend {
    pub fn book_links(&self, content_hash: ContentHash) -> Result<Vec<BookLink>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_book_links(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn upsert_book_link(&self, link: &BookLink) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        store_book_link(&conn, link).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_book_link(&self, link_id: &str, modified_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        remove_book_link(&conn, link_id, modified_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }
}

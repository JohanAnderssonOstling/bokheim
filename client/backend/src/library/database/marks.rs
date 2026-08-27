//! Native library-handle wrappers for shared annotation persistence.

use super::*;
use crate::app::BackendError;
use crate::library::LibraryBackend;
use library_model::ReaderAnnotation;

impl LibraryBackend {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_annotations(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn upsert_annotation(&self, annotation: &ReaderAnnotation) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        store_annotation(&conn, annotation).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_annotation(&self, annotation_id: &str, modified_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        remove_annotation(&conn, annotation_id, modified_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }
}

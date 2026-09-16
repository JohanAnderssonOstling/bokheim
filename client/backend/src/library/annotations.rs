//! Native library-handle wrappers for shared annotation persistence.

use crate::BackendError;
use crate::ContentHash;
use crate::library::LibrarySession;
use library_model::ReaderAnnotation;

impl LibrarySession {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, BackendError> {
        let conn = self.db.get()?;
        client_library::marks::load_annotations(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn upsert_annotation(&self, annotation: &ReaderAnnotation) -> Result<(), BackendError> {
        let conn = self.db.get()?;
        client_library::marks::store_annotation(&conn, annotation).map_err(BackendError::operation)?;
        Ok(())
    }

    pub fn delete_annotation(&self, annotation_id: &str, modified_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get()?;
        client_library::marks::remove_annotation(&conn, annotation_id, modified_at).map_err(BackendError::operation)?;
        Ok(())
    }
}

//! Native library-handle wrappers for shared annotation persistence.

use crate::library::LibrarySession;
use crate::BackendError;
use book_model::Annotation;

impl LibrarySession {
    pub fn upsert_annotation(&self, annotation: &Annotation) -> Result<(), BackendError> {
        self.db.upsert_annotation(annotation).map_err(BackendError::operation)?;
        Ok(())
    }

    pub fn delete_annotation(&self, annotation_id: &str, modified_at: i64) -> Result<(), BackendError> {
        self.db.delete_annotation(annotation_id, modified_at).map_err(BackendError::operation)?;
        Ok(())
    }
}

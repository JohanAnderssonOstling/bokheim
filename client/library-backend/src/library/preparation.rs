//! Prepare local files synchronously; `library_enrichment` finalizes book
//! additions in its own lane.
pub(crate) use crate::library::LibrarySession;
pub(crate) use crate::BackendError;

impl LibrarySession {
    pub(crate) fn admit_uploads(&self) -> Result<(), BackendError> {
        if self.scanner_running() {
            return Err(BackendError::message("waiting for library scan"));
        }
        let snapshot = self.db.upload_preparation_snapshot().map_err(BackendError::operation)?;
        // File preparation and enrichment are separate gates. Prepared files
        // remain withheld from upload until enrichment finalizes the addition.
        let batch = self.db.prepared_upload_batch(&snapshot.hashes()).map_err(BackendError::operation)?;
        self.db.finish_upload_preparation(&batch).map_err(BackendError::operation)?;
        self.events.sync.wake();
        self.events.assets.wake();
        Ok(())
    }
}

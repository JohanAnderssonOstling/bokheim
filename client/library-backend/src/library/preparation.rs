//! Upload admission is synchronous; optional enrichment has its own background lane.
pub(crate) use crate::library::LibrarySession;
pub(crate) use crate::BackendError;

impl LibrarySession {
    pub(crate) fn admit_uploads(&self) -> Result<(), BackendError> {
        if self.scanner_running() {
            return Err(BackendError::message("waiting for library scan"));
        }
        let snapshot = self.db.upload_preparation_snapshot().map_err(BackendError::operation)?;
        // Only durable local file work may delay admission. Thumbnail creation
        // and remote metadata providers are optional follow-up work: neither
        // must hold the book bytes or its lifecycle record off the server.
        let batch = self.db.prepared_upload_batch(&snapshot.hashes()).map_err(BackendError::operation)?;
        self.db.finish_upload_preparation(&batch).map_err(BackendError::operation)?;
        self.sync_wake.wake();
        self.asset_wake.wake();
        Ok(())
    }

    pub(crate) async fn run_enrichment(&self) -> Result<(), BackendError> {
        let snapshot = self.db.upload_preparation_snapshot().map_err(BackendError::operation)?;
        let scope = self.db.prepared_upload_batch(&snapshot.hashes()).map_err(BackendError::operation)?.hashes();
        if self.enrich_books(&scope).await? > 0 {
            self.sync_wake.wake();
        }
        Ok(())
    }
}

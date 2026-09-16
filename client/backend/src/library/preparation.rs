//! Coordinates a fixed local preparation/enrichment batch before upload admission.
use crate::BackendError;
use crate::library::LibrarySession;
use crate::storage::storage_queue::Priority;

impl LibrarySession {
    pub(crate) async fn run_upload_preparation(&self) -> Result<(), BackendError> {
        if self.scanner_running() {
            return Err(BackendError::message("waiting for library scan"));
        }
        let mut batch = self.db.command(Priority::Background, |conn| Ok(client_library::upload_pipeline::snapshot(conn)?)).await.map_err(BackendError::operation)?;
        let scope = batch.hashes();
        if self.assets.local_access_enabled() {
            let Some(_files) = self.begin_local_work() else {
                return Err(BackendError::message("local file preparation is already running"));
            };
            if let Err(error) = self.run_file_jobs().await {
                log::warn!("unprepared files will retry in a later batch: {error}");
            }
            // Freeze membership before preparation. Newly discovered files wait
            // for the next batch, including files added during slow inspection.
            let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            let members = scope.clone();
            let thumbnails = self.db.command(Priority::Background, move |conn| Ok(client_library::upload_pipeline::thumbnails(conn, &members, now)?)).await.map_err(BackendError::operation)?;
            let _thumbnails = if thumbnails.is_empty() { None } else { Some(self.begin_thumbnail_work().ok_or_else(|| BackendError::message("local inspection is already running"))?) };
            if let Err(error) = self.run_thumbnail_members(thumbnails).await {
                log::warn!("some local inspection will retry: {error}");
            }
        }
        batch = self.db.command(Priority::Background, move |conn| Ok(client_library::upload_pipeline::prepared(conn, &scope)?)).await.map_err(BackendError::operation)?;
        let scope = batch.hashes();
        // Audible identifiers may feed the ebook metadata phases. Each recording
        // is attempted at most once; unavailable lookups remain durably retryable.
        self.run_audible_batch(&scope).await?;
        let updated = self.enrich_library_batch(&scope).await?;
        self.db
            .command(Priority::Background, move |conn| {
                client_library::upload_pipeline::finish(conn, &batch)?;
                Ok(())
            })
            .await
            .map_err(BackendError::operation)?;
        if updated > 0 {
            self.sync_wake.wake();
        }
        Ok(())
    }
}

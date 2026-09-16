//! One bounded Audible attempt per member of the current enrichment batch.
use crate::BackendError;
use crate::library::LibrarySession;
use crate::storage::storage_queue::Priority;
use client_library::audible_jobs::{self};
use metadata_contract::audible::{LookupResponse, LookupStatus};
#[cfg(test)]
use std::time::Duration;
use web_time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

impl LibrarySession {
    /// Attempt each eligible member once; retry deadlines never extend this batch.
    pub(crate) async fn run_audible_batch(&self, scope: &std::collections::HashSet<crate::ContentHash>) -> Result<(), BackendError> {
        let members: Vec<_> = scope.iter().copied().collect();
        let candidates = self.db.command(Priority::Background, move |conn| Ok(audible_jobs::batch_candidates(conn, now(), &members)?)).await.map_err(BackendError::operation)?;
        for hash in candidates {
            let job = self.db.command(Priority::Background, move |conn| Ok(audible_jobs::claim_batch(conn, now(), &[hash])?)).await.map_err(BackendError::operation)?;
            let Some(job) = job else { continue };
            let response = self.metadata().audible_lookup(&job.request).await.unwrap_or_else(|error| LookupResponse {
                status: LookupStatus::Unavailable,
                candidates: vec![],
                selected_asin: None,
                selected_region: None,
                chapter_plan: None,
                used_website_fallback: false,
                detail: error.to_string(),
            });
            let committed = self.db.command(Priority::Background, move |conn| Ok(audible_jobs::resolved(conn, &job, &response, now() + 300, now())?)).await.map_err(BackendError::operation)?;
            if committed {
                self.sync_wake.wake();
            }
        }
        Ok(())
    }

    #[cfg(test)]
    async fn run_audible_jobs(&self) -> Result<(), BackendError> {
        let scope = client_library::upload_pipeline::snapshot(&*self.db.get()?)?.hashes();
        self.run_audible_batch(&scope).await
    }
}

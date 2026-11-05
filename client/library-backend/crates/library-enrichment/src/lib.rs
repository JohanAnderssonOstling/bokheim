//! The device's enrichment lane: sends the file facts of books the metadata
//! server hasn't answered and stores its bibliographic answers. Missing artwork
//! is looked up and downloaded directly from cover providers on the device.

use library_database::DatabaseClient;
use library_placement::LibraryState;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use futures_util::FutureExt;
use library_services::events::LibraryEventRouter;

mod client;
mod cover_client;
mod covers;
mod enrich;

pub use client::{MetadataClient, MetadataClientError};
pub use cover_client::CoverClient;

pub type EnrichmentResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct LibraryEnrichment {
    state: Arc<RwLock<LibraryState>>,
    database: DatabaseClient,
    pub client: MetadataClient,
    pub covers: CoverClient,
    pub cpu: Arc<dyn cpu_host::CpuHost>,
    pub events: LibraryEventRouter,
}

impl LibraryEnrichment {
    pub fn new(state: Arc<RwLock<LibraryState>>, database: DatabaseClient, client: MetadataClient, covers: CoverClient, cpu: Arc<dyn cpu_host::CpuHost>, events: LibraryEventRouter) -> Self {
        Self { state, database, client, covers, cpu, events }
    }

    /// One worker cycle. `Err` asks the scheduler to come back soon: a pass
    /// failed, or thumbnail work it waits on is still pending.
    pub async fn run(&self) -> Result<(), ()> {
        if let Err(error) = AssertUnwindSafe(self.enrich_prepared()).catch_unwind().await.unwrap_or_else(|_| Err("library enrichment panicked".into())) {
            log::warn!("library enrichment will resume: {error}");
            return Err(());
        }
        if self.database.pending_thumbnail_job_count().await.map_or(true, |count| count > 0) {
            return Err(());
        }
        let retry_at = self.database.enrichment_next_retry().await.map_err(|error| log::warn!("could not schedule enrichment retry; retrying: {error}"))?;
        if let Some(retry_at) = retry_at {
            // Failed lookups retry; definitive answers finalize the addition once.
            let now = enrich::now();
            // Recheck distant deadlines hourly, including after a clock
            // change. They must not leave the worker asleep indefinitely.
            self.events.enrichment.wake_after(retry_delay(retry_at, now));
        }
        Ok(())
    }

    /// Enriches every book prepared for upload, waking sync and asset
    /// transfer when an answer finalized one.
    async fn enrich_prepared(&self) -> EnrichmentResult<()> {
        let scope = self.database.prepared_upload_batch().await?.hashes();
        if self.enrich_books(&scope).await? > 0 {
            self.events.sync.wake();
            self.events.assets.wake();
        }
        Ok(())
    }
}

fn retry_delay(retry_at: i64, now: i64) -> Duration {
    Duration::from_secs(retry_at.saturating_sub(now).clamp(30, 3_600) as u64)
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn distant_retries_keep_waking_until_due_and_overdue_work_is_bounded() {
        assert_eq!(retry_delay(10_000, 0), Duration::from_secs(3_600));
        assert_eq!(retry_delay(10_000, 7_200), Duration::from_secs(2_800));
        assert_eq!(retry_delay(10_000, 10_001), Duration::from_secs(30));
        assert_eq!(retry_delay(i64::MAX, i64::MIN), Duration::from_secs(3_600));
    }
}

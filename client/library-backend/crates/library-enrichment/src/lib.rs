//! The device's enrichment lane: sends the file facts of books the metadata
//! server hasn't answered, stores its answers, and fills missing covers from
//! where those answers point. The server owns every lookup and merge rule;
//! this only sends facts and stores answers.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use library_files::asset_store::AssetStore;
use futures_util::FutureExt;
use library_database::Database;
use library_runtime::events::LibraryEventRouter;

mod client;
mod covers;
mod enrich;

pub use client::{MetadataClient, MetadataClientError};

pub type EnrichmentResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct LibraryEnrichment {
    pub database: Arc<Database>,
    pub assets: AssetStore,
    pub client: MetadataClient,
    pub cpu: Arc<dyn cpu_host::CpuHost>,
    pub events: LibraryEventRouter,
}

impl LibraryEnrichment {
    /// One worker cycle. `Err` asks the scheduler to come back soon: a pass
    /// failed, or thumbnail work it waits on is still pending.
    pub async fn run(&self) -> Result<(), ()> {
        if let Err(error) = AssertUnwindSafe(self.enrich_prepared()).catch_unwind().await.unwrap_or_else(|_| Err("library enrichment panicked".into())) {
            log::warn!("library enrichment will resume: {error}");
            return Err(());
        }
        if self.assets.local_access_enabled() && self.database.pending_thumbnail_job_count().map_or(true, |count| count > 0) {
            return Err(());
        }
        if let Some(retry_at) = self.database.enrichment_next_retry().unwrap_or(None) {
            // Failed lookups retry; definitive answers finalize the addition once.
            let now = enrich::now();
            if retry_at <= now + 3_600 {
                self.events.enrichment.wake_after(Duration::from_secs((retry_at - now).max(30) as u64));
            }
        }
        Ok(())
    }

    /// Enriches every book prepared for upload, waking sync and asset
    /// transfer when an answer finalized one.
    async fn enrich_prepared(&self) -> EnrichmentResult<()> {
        let snapshot = self.database.upload_preparation_snapshot()?;
        let scope = self.database.prepared_upload_batch(&snapshot.hashes())?.hashes();
        if self.enrich_books(&scope).await? > 0 {
            self.events.sync.wake();
            self.events.assets.wake();
        }
        Ok(())
    }
}

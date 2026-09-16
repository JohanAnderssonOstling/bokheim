use super::super::{BackfillBatchMetrics, CatalogProcess, CatalogService, CatalogStore, Instant, METADATA_BACKFILL_BATCH_SIZE, METADATA_BACKFILL_MAX_JOBS_PER_PASS, METADATA_BACKFILL_RETRY_DELAY};
impl CatalogService {
    pub(crate) async fn resume_metadata_backfill(&self) {
        if !self.process_enabled(CatalogProcess::Enrichment) {
            return;
        }
        if !self.schedule_backfill("backfill_schedule", "could not schedule saved catalogue metadata for local backfill", CatalogStore::schedule_metadata_backfill).await {
            return;
        }
        let mut processed = 0;
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                return;
            }
            let claim_started = Instant::now();
            let jobs = match self.store_write_call_named("backfill_claim", |cache| cache.claim_metadata_backfill_indexed_batch(METADATA_BACKFILL_BATCH_SIZE)).await {
                Ok(jobs) if jobs.is_empty() => return,
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::warn!(%error, "could not claim saved catalogue metadata backfill batch");
                    return;
                }
            };
            let batch_metrics = BackfillBatchMetrics::begin(&self.state.metadata_backfill_metrics, jobs.len(), claim_started.elapsed());
            processed += jobs.len();
            let enrich_started = Instant::now();
            match self.enrich_saved_metadata_batch(&jobs).await {
                Ok(updates) => {
                    batch_metrics.record_enrichment(enrich_started.elapsed());
                    if let Err(error) = self.finish_metadata_backfill_indexed_batch_timed(updates).await.map(|(writer_wait_ms, timings)| {
                        batch_metrics.record_metadata_write(writer_wait_ms, &timings);
                    }) {
                        tracing::warn!(%error, "could not commit saved catalogue metadata backfill batch");
                        let release = jobs.clone();
                        let message = error.to_string();
                        let _ = self.store_write_call(move |cache| cache.release_metadata_backfill_indexed_batch(&release, &message)).await;
                        return;
                    }
                }
                Err(error) => {
                    batch_metrics.record_enrichment(enrich_started.elapsed());
                    batch_metrics.record_failure();
                    tracing::warn!(%error, "saved catalogue metadata backfill job failed; recording and continuing");
                    let release = jobs.clone();
                    let message = error.to_string();
                    if let Err(release_error) = self.store_write_call(move |cache| cache.release_metadata_backfill_indexed_batch(&release, &message)).await {
                        tracing::warn!(%release_error, "could not record saved catalogue metadata backfill failure");
                        return;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(METADATA_BACKFILL_RETRY_DELAY) => {}
                        _ = self.wait_for_process_wake(CatalogProcess::Enrichment) => {}
                    }
                }
            }
            if processed >= METADATA_BACKFILL_MAX_JOBS_PER_PASS {
                return;
            }
        }
    }
}

use super::super::{
    catalog_metadata_match, BackfillBatchMetrics, CatalogCache, CatalogDescriptionBackfillJob, CatalogDescriptionBackfillUpdate, CatalogService, CatalogServiceError, HashSet, Instant, Ordering, StatusCode, METADATA_BACKFILL_BATCH_SIZE,
};
impl CatalogService {
    pub(crate) async fn resume_description_backfill(&self) {
        match self.metadata_descriptions_available().await {
            Ok(true) => self.state.description_backfill_available.store(true, Ordering::Relaxed),
            Ok(false) => {
                self.state.description_backfill_available.store(false, Ordering::Relaxed);
                tracing::info!("catalogue description backfill is waiting for a metadata snapshot with Open Library descriptions");
                return;
            }
            Err(error) => {
                self.state.description_backfill_available.store(false, Ordering::Relaxed);
                tracing::warn!(%error, "could not determine whether the metadata snapshot supports description backfill");
                return;
            }
        }
        if !self.schedule_backfill("description_backfill_schedule", "could not schedule catalogue description backfill", CatalogCache::schedule_description_backfill).await {
            return;
        }
        loop {
            let claim_started = Instant::now();
            let jobs = match self.cache_write_call_named("description_backfill_claim", |cache| cache.claim_description_backfill_batch(METADATA_BACKFILL_BATCH_SIZE)).await {
                Ok(jobs) if jobs.is_empty() => return,
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::warn!(%error, "could not claim catalogue description backfill batch");
                    return;
                }
            };
            let batch_metrics = BackfillBatchMetrics::begin(&self.state.description_backfill_metrics, jobs.len(), claim_started.elapsed());
            let enrich_started = Instant::now();
            match self.enrich_description_batch(&jobs).await {
                Ok(updates) => {
                    batch_metrics.record_enrichment(enrich_started.elapsed());
                    if let Err(error) = self.cache_write_call_named("description_backfill_finish", move |cache| cache.finish_description_backfill_batch(&updates)).await {
                        batch_metrics.record_failure();
                        tracing::warn!(%error, "could not commit catalogue description backfill batch");
                        let release = jobs.clone();
                        let message = error.to_string();
                        let _ = self.cache_write_call(move |cache| cache.release_description_backfill_batch(&release, &message)).await;
                        return;
                    }
                }
                Err(error) => {
                    batch_metrics.record_enrichment(enrich_started.elapsed());
                    batch_metrics.record_failure();
                    tracing::warn!(%error, "catalogue description backfill lookup failed; recording and continuing");
                    let release = jobs.clone();
                    let message = error.to_string();
                    if let Err(release_error) = self.cache_write_call(move |cache| cache.release_description_backfill_batch(&release, &message)).await {
                        tracing::warn!(%release_error, "could not record catalogue description backfill failure");
                        return;
                    }
                }
            }
        }
    }

    pub(crate) async fn metadata_descriptions_available(&self) -> Result<bool, CatalogServiceError> {
        let Some(endpoint) = &self.state.metadata_endpoint else { return Ok(false) };
        let mut endpoint = endpoint.clone();
        endpoint.set_path("/health/descriptions");
        let response = self.state.metadata_client.get(endpoint).send().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        if response.status().is_success() {
            return Ok(true);
        }
        if response.status().as_u16() == StatusCode::SERVICE_UNAVAILABLE.as_u16() {
            return Ok(false);
        }
        Err(CatalogServiceError(format!("metadata description capability returned HTTP {}", response.status())))
    }

    pub(crate) async fn enrich_description_batch(&self, jobs: &[CatalogDescriptionBackfillJob]) -> Result<Vec<CatalogDescriptionBackfillUpdate>, CatalogServiceError> {
        let all_isbns = jobs.iter().flat_map(|job| job.isbns.iter().cloned()).collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
        let (snapshot_date, matches_by_isbn) = self.metadata_matches_by_isbn(&all_isbns).await?;
        Ok(jobs
            .iter()
            .map(|job| {
                let records = job.isbns.iter().filter_map(|isbn| matches_by_isbn.get(isbn)).flatten().collect::<Vec<_>>();
                let open_library_description_missing = !records.is_empty() && records.iter().all(|record| record.description.is_none());
                let matched = catalog_metadata_match(records, &snapshot_date);
                CatalogDescriptionBackfillUpdate {
                    source_id: job.source_id.clone(),
                    publication: job.publication.clone(),
                    acquisition_url: job.acquisition_url.clone(),
                    description: matched.description,
                    subjects: matched.subjects,
                    open_library_description_missing,
                }
            })
            .collect())
    }
}

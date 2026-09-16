use super::{
    BTreeMap, CatalogExtractionRuntimeMetrics, CatalogExtractionRuntimeMetricsResponse, CatalogMetadataBackfillWriteTimings, CatalogService, CatalogStore, CatalogStoreError, Duration, Instant, MetadataBackfillRuntimeMetrics, MetadataBackfillRuntimeMetricsResponse, Ordering,
    StdMutex, WriterRuntimeMetric, WriterRuntimeMetricResponse,
};
pub(crate) struct BackfillBatchMetrics<'a> {
    metrics: &'a MetadataBackfillRuntimeMetrics,
    jobs: u64,
    started_at: Instant,
}

impl<'a> BackfillBatchMetrics<'a> {
    pub(crate) fn begin(metrics: &'a MetadataBackfillRuntimeMetrics, jobs: usize, claim_duration: Duration) -> Self {
        let jobs = jobs as u64;
        metrics.claim_ms.fetch_add(duration_millis(claim_duration), Ordering::Relaxed);
        metrics.batches.fetch_add(1, Ordering::Relaxed);
        metrics.jobs.fetch_add(jobs, Ordering::Relaxed);
        Self { metrics, jobs, started_at: Instant::now() }
    }

    pub(crate) fn record_enrichment(&self, duration: Duration) {
        self.metrics.enrich_ms.fetch_add(duration_millis(duration), Ordering::Relaxed);
    }

    pub(crate) fn record_failure(&self) {
        self.metrics.failures.fetch_add(self.jobs, Ordering::Relaxed);
    }

    pub(crate) fn record_metadata_write(&self, writer_wait_ms: u64, timings: &CatalogMetadataBackfillWriteTimings) {
        self.metrics.writer_wait_ms.fetch_add(writer_wait_ms, Ordering::Relaxed);
        self.metrics.transaction_ms.fetch_add(timings.transaction_ms, Ordering::Relaxed);
        self.metrics.commit_ms.fetch_add(timings.commit_ms, Ordering::Relaxed);
        self.metrics.subjects_ms.fetch_add(timings.subjects_ms, Ordering::Relaxed);
        self.metrics.authors_ms.fetch_add(timings.authors_ms, Ordering::Relaxed);
        self.metrics.identity_review_ms.fetch_add(timings.identity_review_ms, Ordering::Relaxed);
        self.metrics.index_ms.fetch_add(timings.index_ms, Ordering::Relaxed);
        self.metrics.status_ms.fetch_add(timings.status_ms, Ordering::Relaxed);
    }
}

impl Drop for BackfillBatchMetrics<'_> {
    fn drop(&mut self) {
        self.metrics.total_ms.fetch_add(duration_millis(self.started_at.elapsed()), Ordering::Relaxed);
    }
}

impl CatalogService {
    pub(crate) async fn schedule_backfill<F>(&self, operation_name: &'static str, failure_message: &'static str, schedule: F) -> bool
    where
        F: Fn(&CatalogStore) -> Result<usize, CatalogStoreError> + Copy + Send + Sync + 'static,
    {
        loop {
            match self.store_write_call_named(operation_name, schedule).await {
                Ok(1000) => tokio::task::yield_now().await,
                Ok(_) => return true,
                Err(error) => {
                    tracing::warn!(%error, "{failure_message}");
                    return false;
                }
            }
        }
    }
}

impl MetadataBackfillRuntimeMetrics {
    pub(crate) fn snapshot(&self) -> MetadataBackfillRuntimeMetricsResponse {
        let batches = self.batches.load(Ordering::Relaxed);
        let jobs = self.jobs.load(Ordering::Relaxed);
        let failures = self.failures.load(Ordering::Relaxed);
        let claim_ms = self.claim_ms.load(Ordering::Relaxed);
        let enrich_ms = self.enrich_ms.load(Ordering::Relaxed);
        let writer_wait_ms = self.writer_wait_ms.load(Ordering::Relaxed);
        let transaction_ms = self.transaction_ms.load(Ordering::Relaxed);
        let commit_ms = self.commit_ms.load(Ordering::Relaxed);
        let subjects_ms = self.subjects_ms.load(Ordering::Relaxed);
        let authors_ms = self.authors_ms.load(Ordering::Relaxed);
        let identity_review_ms = self.identity_review_ms.load(Ordering::Relaxed);
        let index_ms = self.index_ms.load(Ordering::Relaxed);
        let status_ms = self.status_ms.load(Ordering::Relaxed);
        let total_ms = self.total_ms.load(Ordering::Relaxed);
        MetadataBackfillRuntimeMetricsResponse {
            batches,
            jobs,
            failures,
            claim_ms,
            enrich_ms,
            writer_wait_ms,
            transaction_ms,
            commit_ms,
            subjects_ms,
            authors_ms,
            identity_review_ms,
            index_ms,
            status_ms,
            total_ms,
            jobs_per_second: if total_ms == 0 { 0.0 } else { jobs as f64 * 1_000.0 / total_ms as f64 },
        }
    }
}

impl CatalogExtractionRuntimeMetrics {
    pub(crate) fn record(&self, timings: &crate::extraction::CatalogExtractionTimings) {
        self.jobs.fetch_add(1, Ordering::Relaxed);
        self.container_ms.fetch_add(timings.container_ms, Ordering::Relaxed);
        self.evidence_ms.fetch_add(timings.evidence_ms, Ordering::Relaxed);
        self.toc_ms.fetch_add(timings.toc_ms, Ordering::Relaxed);
        self.thumbnail_ms.fetch_add(timings.thumbnail_ms, Ordering::Relaxed);
        self.worker_ms.fetch_add(timings.worker_ms, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> CatalogExtractionRuntimeMetricsResponse {
        let jobs = self.jobs.load(Ordering::Relaxed);
        let container_ms = self.container_ms.load(Ordering::Relaxed);
        let evidence_ms = self.evidence_ms.load(Ordering::Relaxed);
        let toc_ms = self.toc_ms.load(Ordering::Relaxed);
        let thumbnail_ms = self.thumbnail_ms.load(Ordering::Relaxed);
        let worker_ms = self.worker_ms.load(Ordering::Relaxed);
        CatalogExtractionRuntimeMetricsResponse {
            jobs,
            container_ms,
            evidence_ms,
            toc_ms,
            thumbnail_ms,
            worker_ms,
            average_container_ms: if jobs == 0 { 0.0 } else { container_ms as f64 / jobs as f64 },
            average_evidence_ms: if jobs == 0 { 0.0 } else { evidence_ms as f64 / jobs as f64 },
            average_toc_ms: if jobs == 0 { 0.0 } else { toc_ms as f64 / jobs as f64 },
            average_thumbnail_ms: if jobs == 0 { 0.0 } else { thumbnail_ms as f64 / jobs as f64 },
            average_worker_ms: if jobs == 0 { 0.0 } else { worker_ms as f64 / jobs as f64 },
        }
    }
}

pub(crate) fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn writer_metrics_snapshot(metrics: &StdMutex<BTreeMap<String, WriterRuntimeMetric>>) -> BTreeMap<String, WriterRuntimeMetricResponse> {
    let Ok(metrics) = metrics.lock() else { return BTreeMap::new() };
    metrics
        .iter()
        .map(|(name, metric)| {
            (
                name.clone(),
                WriterRuntimeMetricResponse {
                    calls: metric.calls,
                    failures: metric.failures,
                    wait_ms: metric.wait_ms,
                    hold_ms: metric.hold_ms,
                    average_wait_ms: if metric.calls == 0 { 0.0 } else { metric.wait_ms as f64 / metric.calls as f64 },
                    average_hold_ms: if metric.calls == 0 { 0.0 } else { metric.hold_ms as f64 / metric.calls as f64 },
                },
            )
        })
        .collect()
}

use axum::body::{Body, Bytes};
use axum::extract::{Json, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use catalog_contract::{
    materialize_page, CatalogAuthorCredit, CatalogBookFormat, CatalogDescription, CatalogImportRequest, CatalogLinkTarget, CatalogPage, CatalogPageRequest, CatalogPageResponse, CatalogPublication, CatalogPublicationAction,
    CatalogPublicationIdentity, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchRequest, CatalogSearchResponse, CatalogSource, CatalogSourceId, CatalogSourceRegistration, CatalogSourceRegistry,
    CatalogSubjectBrowseCapabilitiesResponse, CatalogSubjectBrowseDeltaResponse, CatalogSubjectBrowseRefreshRequest, CatalogSubjectBrowseRequest, CatalogSubjectCollectionManifestRequest, CatalogSubjectCollectionManifestResponse,
    CatalogSubjectPublicationPayloadRequest, CatalogSubjectPublicationPayloadResponse, LanguageTag, PopularityRank,
};
use catalog_core::{
    canonicalize_opds_catalog_locator, opds_catalog_source_id, opds_page, opds_source, prepare_catalog_subject_assignments, validate_opds_catalog_locator, validated_opds_import, CatalogCache, CatalogCacheError, CatalogCoverCache,
    CatalogCoverDownloadJob, CatalogCrawlGeneration, CatalogDescriptionBackfillJob, CatalogDescriptionBackfillUpdate, CatalogEpubExtractionJob, CatalogEpubMetadata, CatalogInventory, CatalogLanguageFilter,
    CatalogMetadataBackfillIndexedJob, CatalogMetadataBackfillIndexedUpdate, CatalogMetadataBackfillWriteTimings, CatalogPageWrite, CatalogProcessSettings, CatalogPublicationPopularity,
};
use library_model::{canonical_isbn, Identifier, PublicationSubject, Scheme, Scope};
use metadata_contract::{ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityRequest, EditionIdentityResponse, EditionIdentityStatus, LookupStatus, MetadataEnrichmentRequest, RichWorkMetadataMatch};
use opds::{
    endpoints_from_registry, validate_curated_registry, CatalogRegistry, DownloadedOpdsBook, OpdsAcquisitionKind, OpdsBookFormat, OpdsClient, OpdsConditionalCatalog, OpdsDirectAcquisition, OpdsEndpoint, OpdsError, OpdsHttpValidators,
    OpdsSearch,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, Notify, RwLock, Semaphore};
use tower_http::cors::{Any, CorsLayer};
use url::Url;

const MAX_REGISTRY_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_CRAWL_PAGES: usize = 50_000;
const HARD_MAX_CRAWL_PAGES: usize = 250_000;
const MAX_RECENT_BROWSER_COVERS: usize = 4_096;
// SQLite still has one explicitly serialized writer while network-bound source
// crawls overlap. Distinct origins run together; sources sharing an origin run
// sequentially so their request windows cannot stack.
const CRAWL_FRONTIER_BATCH_SIZE: usize = 64;
// A small per-source window overlaps upstream latency without turning the
// crawler into a burst client. Sources sharing an origin are scheduled in the
// same serial origin group, so this is also the maximum concurrency per host.
const CRAWL_PAGE_CONCURRENCY: usize = 4;
const CRAWL_PAGE_CONCURRENCY_MAX: usize = 8;
const CRAWL_ORIGIN_SUCCESS_WINDOW: u32 = 8;
const CRAWL_ORIGIN_INITIAL_SPACING: Duration = Duration::from_millis(50);
const CRAWL_ORIGIN_MIN_SPACING: Duration = Duration::from_millis(10);
const CRAWL_ORIGIN_MAX_SPACING: Duration = Duration::from_secs(2);
const CRAWL_ORIGIN_TRANSIENT_BACKOFF: Duration = Duration::from_secs(1);
const CRAWL_ORIGIN_DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(30);
const CRAWL_ORIGIN_MAX_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(24 * 60 * 60);
const CRAWL_ORIGIN_MAX_ATTEMPTS: usize = 3;
const CRAWL_WRITE_BATCH_INITIAL_PAGES: usize = 32;
const CRAWL_WRITE_BATCH_MIN_PAGES: usize = 8;
const CRAWL_WRITE_BATCH_MAX_PAGES: usize = 64;
const CRAWL_WRITE_BATCH_MAX_PUBLICATIONS: usize = 2_000;
const CRAWL_WRITE_BATCH_MAX_BYTES: usize = 16 * 1024 * 1024;
const CRAWL_WRITE_BATCH_MAX_AGE: Duration = Duration::from_millis(250);
const CRAWL_WRITE_BATCH_FAST_COMMIT: Duration = Duration::from_millis(75);
const CRAWL_WRITE_BATCH_SLOW_COMMIT: Duration = Duration::from_millis(200);
const CRAWL_ACTIVATION_BATCH: usize = 256;
const EXTRACTION_RECONCILIATION_BATCH: usize = 256;
const COVER_RECONCILIATION_BATCH: usize = 256;
const INACTIVE_GENERATION_CLEANUP_BATCH: usize = 256;
const PRIORITY_CRAWL_SOURCE: &str = "litteraturbanken";
const CRAWL_PAUSE_SENTINEL: &str = "/var/lib/bokheim-deploy/catalog-crawls-disabled";
const BACKGROUND_DOWNLOAD_LIMIT: usize = 1;
const MAX_INTERACTIVE_DOWNLOADS: usize = 2;
const MAX_QUEUED_DOWNLOADS: usize = 4;
const MAX_CONCURRENT_EXTRACTIONS: usize = 1;
#[cfg(not(test))]
const EXTRACTION_WORKER_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
#[cfg(not(test))]
const EXTRACTION_WORKER_CPU_SECONDS: u64 = 60;
#[cfg(not(test))]
const EXTRACTION_WORKER_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_EXTRACTION_RESULT_BYTES: usize = 4 * 1024 * 1024;
const MAX_EPUB_METADATA_PUBLICATION_BYTES: u64 = 128 * 1024 * 1024;
const MAX_MOBI_METADATA_PUBLICATION_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PDF_METADATA_PUBLICATION_BYTES: u64 = 32 * 1024 * 1024;
const CATALOG_METADATA_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_EMBEDDED_METADATA_TEXT_BYTES: usize = 4 * 1024;
const MAX_EMBEDDED_DESCRIPTION_BYTES: usize = 64 * 1024;
const BROWSER_UPSTREAM_TIMEOUT: Duration = Duration::from_secs(12);
const CRAWL_CYCLE: Duration = Duration::from_secs(24 * 60 * 60);
const REGISTRY_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const EPUB_EXTRACTION_SWEEP_INTERVAL: Duration = Duration::from_secs(5 * 60);
// A job owns the complete saved EPUB metadata graph, including its navigation
// tree. Keep only a small set live so one run cannot establish a large heap
// high-water mark while the backlog is drained.
const METADATA_BACKFILL_BATCH_SIZE: usize = 32;
const METADATA_BACKFILL_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);
const REGISTRY_CACHE_CONTROL: &str = "public, max-age=300";
const DATA_CACHE_CONTROL: &str = "public, max-age=60";
const COVER_CACHE_CONTROL: &str = "public, max-age=31536000, immutable, no-transform";
const BUNDLED_REGISTRY: &[u8] = include_bytes!("../opds.json");
const EPUB_COVER_KEY_PREFIX: &str = "urn:bokheim:epub-cover:";

#[derive(Clone)]
pub struct CatalogService {
    state: Arc<CatalogServiceState>,
}

struct CatalogServiceState {
    source: RegistrySource,
    snapshot: RwLock<RegistrySnapshot>,
    endpoints: RwLock<Vec<OpdsEndpoint>>,
    opds: OpdsClient,
    cache: CatalogCache,
    covers: CatalogCoverCache,
    // SQLite permits only one writer. Queue writes before entering the
    // blocking pool so in-process jobs never spend the busy timeout fighting
    // one another (or turn ordinary contention into extraction failures).
    cache_write_lock: Mutex<()>,
    crawl_locks: Mutex<HashMap<CatalogSourceId, Arc<Mutex<()>>>>,
    crawl_progress: Mutex<HashMap<CatalogSourceId, CrawlProgress>>,
    origin_throttles: Mutex<HashMap<String, Arc<AdaptiveOriginThrottle>>>,
    source_update_locks: Mutex<HashMap<CatalogSourceId, Arc<Mutex<()>>>>,
    recent_browser_covers: Mutex<RecentBrowserCovers>,
    epub_range_origins: Mutex<HashMap<String, bool>>,
    background_download_slots: Semaphore,
    interactive_download_slots: Semaphore,
    queued_download_slots: Semaphore,
    extraction_slots: Semaphore,
    extraction_notify: Notify,
    processes: CatalogProcessRuntime,
    acquisition_directory: PathBuf,
    metadata_endpoint: Option<Url>,
    metadata_client: reqwest::Client,
    metadata_backfill_metrics: MetadataBackfillRuntimeMetrics,
    description_backfill_metrics: MetadataBackfillRuntimeMetrics,
    description_backfill_available: AtomicBool,
    writer_metrics: StdMutex<BTreeMap<String, WriterRuntimeMetric>>,
    max_crawl_pages: usize,
    started_at: Instant,
}

struct AdaptiveOriginThrottle {
    state: Mutex<AdaptiveOriginThrottleState>,
    notify: Notify,
}

struct AdaptiveOriginThrottleState {
    in_flight: usize,
    concurrency_limit: usize,
    success_streak: u32,
    minimum_spacing: Duration,
    next_request_at: Instant,
    blocked_until: Option<Instant>,
    latency_ewma_ms: Option<u64>,
}

#[derive(Clone, Copy)]
enum OriginThrottleFeedback {
    Success(Duration),
    RateLimited(Option<Duration>),
    TransientFailure,
    OtherFailure,
}

#[derive(Clone, Copy)]
struct OriginThrottleSnapshot {
    in_flight: usize,
    concurrency_limit: usize,
    minimum_spacing_ms: u64,
    blocked_for_ms: Option<u64>,
}

impl Default for OriginThrottleSnapshot {
    fn default() -> Self {
        Self { in_flight: 0, concurrency_limit: CRAWL_PAGE_CONCURRENCY, minimum_spacing_ms: CRAWL_ORIGIN_INITIAL_SPACING.as_millis() as u64, blocked_for_ms: None }
    }
}

#[derive(Default)]
struct MetadataBackfillRuntimeMetrics {
    batches: AtomicU64,
    jobs: AtomicU64,
    failures: AtomicU64,
    claim_ms: AtomicU64,
    enrich_ms: AtomicU64,
    writer_wait_ms: AtomicU64,
    transaction_ms: AtomicU64,
    commit_ms: AtomicU64,
    subjects_ms: AtomicU64,
    authors_ms: AtomicU64,
    identity_review_ms: AtomicU64,
    index_ms: AtomicU64,
    status_ms: AtomicU64,
    total_ms: AtomicU64,
}

#[derive(Default)]
struct WriterRuntimeMetric {
    calls: u64,
    failures: u64,
    wait_ms: u64,
    hold_ms: u64,
}

#[derive(Serialize)]
struct WriterRuntimeMetricResponse {
    calls: u64,
    failures: u64,
    wait_ms: u64,
    hold_ms: u64,
    average_wait_ms: f64,
    average_hold_ms: f64,
}

#[derive(Serialize)]
struct MetadataBackfillRuntimeMetricsResponse {
    batches: u64,
    jobs: u64,
    failures: u64,
    claim_ms: u64,
    enrich_ms: u64,
    writer_wait_ms: u64,
    transaction_ms: u64,
    commit_ms: u64,
    subjects_ms: u64,
    authors_ms: u64,
    identity_review_ms: u64,
    index_ms: u64,
    status_ms: u64,
    total_ms: u64,
    jobs_per_second: f64,
}

enum RegistrySource {
    Bundled,
    File(PathBuf),
}

struct RegistrySnapshot {
    observed_bytes: Vec<u8>,
    registry: CatalogRegistry,
}

#[derive(Clone)]
struct CrawlProgress {
    started_at: Instant,
    pages_fetched: u64,
    publications_seen: u64,
}

struct AdaptiveCrawlWriteBatch {
    pages: Vec<CatalogPageWrite>,
    publications: usize,
    bytes: usize,
    started_at: Option<Instant>,
    page_limit: usize,
}

impl Default for AdaptiveCrawlWriteBatch {
    fn default() -> Self {
        Self { pages: Vec::new(), publications: 0, bytes: 0, started_at: None, page_limit: CRAWL_WRITE_BATCH_INITIAL_PAGES }
    }
}

impl AdaptiveCrawlWriteBatch {
    fn push(&mut self, write: CatalogPageWrite, publications: usize, bytes: usize) {
        self.started_at.get_or_insert_with(Instant::now);
        self.pages.push(write);
        self.publications = self.publications.saturating_add(publications);
        self.bytes = self.bytes.saturating_add(bytes);
    }

    fn reached_capacity(&self) -> bool {
        self.pages.len() >= self.page_limit || self.publications >= CRAWL_WRITE_BATCH_MAX_PUBLICATIONS || self.bytes >= CRAWL_WRITE_BATCH_MAX_BYTES
    }

    fn should_flush_incremental(&self) -> bool {
        self.reached_capacity() || self.started_at.is_some_and(|started| started.elapsed() >= CRAWL_WRITE_BATCH_MAX_AGE)
    }

    fn take_pages(&mut self) -> Vec<CatalogPageWrite> {
        self.publications = 0;
        self.bytes = 0;
        self.started_at = None;
        std::mem::take(&mut self.pages)
    }

    fn record_commit(&mut self, elapsed: Duration) {
        if elapsed >= CRAWL_WRITE_BATCH_SLOW_COMMIT {
            self.page_limit = (self.page_limit / 2).max(CRAWL_WRITE_BATCH_MIN_PAGES);
        } else if elapsed <= CRAWL_WRITE_BATCH_FAST_COMMIT {
            self.page_limit = self.page_limit.saturating_mul(2).min(CRAWL_WRITE_BATCH_MAX_PAGES);
        }
    }
}

#[derive(Clone)]
struct RecentCoverReference {
    identity: CatalogPublicationIdentity,
    acquisition: Option<OpdsDirectAcquisition>,
}

#[derive(Clone, Default)]
struct CatalogMetadataMatch {
    subjects: Vec<PublicationSubject>,
    popularity: Option<CatalogPublicationPopularity>,
    description: Option<String>,
}

#[derive(Default)]
struct RecentBrowserCovers {
    entries: HashMap<String, RecentCoverReference>,
    order: VecDeque<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CatalogProcess {
    CatalogueCrawling,
    CoverDownloads,
    EpubEnrichment,
    CatalogueEnrichment,
}

impl CatalogProcess {
    const ALL: [Self; 4] = [Self::CatalogueCrawling, Self::CoverDownloads, Self::EpubEnrichment, Self::CatalogueEnrichment];

    fn storage_key(self) -> &'static str {
        match self {
            Self::CatalogueCrawling => "catalogue_crawling",
            Self::CoverDownloads => "cover_downloads",
            Self::EpubEnrichment => "epub_enrichment",
            Self::CatalogueEnrichment => "catalogue_enrichment",
        }
    }
}

struct CatalogProcessRuntime {
    catalogue_crawling_enabled: AtomicBool,
    catalogue_crawling_running: AtomicBool,
    catalogue_crawling_notify: Notify,
    cover_downloads_enabled: AtomicBool,
    cover_downloads_running: AtomicBool,
    cover_downloads_notify: Notify,
    epub_enrichment_enabled: AtomicBool,
    epub_enrichment_running: AtomicBool,
    epub_enrichment_notify: Notify,
    catalogue_enrichment_enabled: AtomicBool,
    catalogue_enrichment_running: AtomicBool,
    catalogue_enrichment_notify: Notify,
}

impl CatalogProcessRuntime {
    fn new(settings: CatalogProcessSettings) -> Self {
        Self {
            catalogue_crawling_enabled: AtomicBool::new(settings.catalogue_crawling),
            catalogue_crawling_running: AtomicBool::new(false),
            catalogue_crawling_notify: Notify::new(),
            cover_downloads_enabled: AtomicBool::new(settings.cover_downloads),
            cover_downloads_running: AtomicBool::new(false),
            cover_downloads_notify: Notify::new(),
            epub_enrichment_enabled: AtomicBool::new(settings.epub_enrichment),
            epub_enrichment_running: AtomicBool::new(false),
            epub_enrichment_notify: Notify::new(),
            catalogue_enrichment_enabled: AtomicBool::new(settings.catalogue_enrichment),
            catalogue_enrichment_running: AtomicBool::new(false),
            catalogue_enrichment_notify: Notify::new(),
        }
    }
}

#[derive(Serialize)]
struct CatalogProcessStatus {
    process: CatalogProcess,
    enabled: bool,
    running: bool,
    pause_pending: bool,
    blocked_by_configuration: bool,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum CatalogProcessAction {
    Pause,
    Resume,
    RunNow,
}

/// A per-crawl, private on-disk queue. Keeping both pending and visited URLs in
/// this temporary database avoids duplicating a large graph in heap memory and
/// does not contend with the catalogue index's SQLite writer.
struct DiskCrawlFrontier {
    _directory: tempfile::TempDir,
    connection: Connection,
    discovered: usize,
    limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrawlLocator {
    locator: String,
    popular_listing: bool,
}

impl CrawlLocator {
    #[cfg(test)]
    fn regular(locator: impl Into<String>) -> Self {
        Self { locator: locator.into(), popular_listing: false }
    }
}

impl DiskCrawlFrontier {
    fn new(root: &str, limit: usize) -> Result<Self, CatalogServiceError> {
        let directory = tempfile::Builder::new().prefix("bokheim-crawl-frontier-").tempdir().map_err(|error| CatalogServiceError(error.to_string()))?;
        let connection = Connection::open(directory.path().join("frontier.sqlite3")).map_err(|error| CatalogServiceError(error.to_string()))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=OFF;
                 PRAGMA synchronous=OFF;
                 PRAGMA temp_store=FILE;
                 CREATE TABLE frontier(
                     ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
                     locator TEXT NOT NULL UNIQUE,
                     popular_listing INTEGER NOT NULL CHECK(popular_listing IN (0,1)),
                     pending INTEGER NOT NULL CHECK(pending IN (0,1))
                 );
                 CREATE INDEX frontier_pending ON frontier(pending, ordinal);",
            )
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        connection.execute("INSERT INTO frontier(locator,popular_listing,pending) VALUES(?1,0,1)", [root]).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(Self { _directory: directory, connection, discovered: 1, limit: limit.max(1) })
    }

    fn enqueue(&mut self, locators: impl IntoIterator<Item = CrawlLocator>) -> Result<(), CatalogServiceError> {
        let transaction = self.connection.transaction().map_err(|error| CatalogServiceError(error.to_string()))?;
        for locator in locators {
            let inserted =
                transaction.execute("INSERT OR IGNORE INTO frontier(locator,popular_listing,pending) VALUES(?1,?2,1)", rusqlite::params![locator.locator, locator.popular_listing]).map_err(|error| CatalogServiceError(error.to_string()))?;
            if inserted != 0 {
                self.discovered = self.discovered.saturating_add(1);
                if self.discovered > self.limit {
                    return Err(CatalogServiceError(format!("catalog crawl exceeded {} discovered pages", self.limit)));
                }
            } else if locator.popular_listing {
                // Revisit a page once when a later edge reveals that it is a
                // provider-defined popular listing.
                transaction.execute("UPDATE frontier SET popular_listing=1,pending=CASE WHEN popular_listing=0 THEN 1 ELSE pending END WHERE locator=?1", [locator.locator]).map_err(|error| CatalogServiceError(error.to_string()))?;
            }
        }
        transaction.commit().map_err(|error| CatalogServiceError(error.to_string()))
    }

    fn claim_batch(&mut self) -> Result<Vec<CrawlLocator>, CatalogServiceError> {
        let transaction = self.connection.transaction().map_err(|error| CatalogServiceError(error.to_string()))?;
        let locators = {
            let mut statement = transaction.prepare("SELECT locator,popular_listing FROM frontier WHERE pending=1 ORDER BY ordinal LIMIT ?1").map_err(|error| CatalogServiceError(error.to_string()))?;
            let rows = statement.query_map([CRAWL_FRONTIER_BATCH_SIZE as i64], |row| Ok(CrawlLocator { locator: row.get(0)?, popular_listing: row.get(1)? })).map_err(|error| CatalogServiceError(error.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|error| CatalogServiceError(error.to_string()))?
        };
        for locator in &locators {
            transaction.execute("UPDATE frontier SET pending=0 WHERE locator=?1", [&locator.locator]).map_err(|error| CatalogServiceError(error.to_string()))?;
        }
        transaction.commit().map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(locators)
    }
}

impl RecentBrowserCovers {
    fn remember(&mut self, source_id: &CatalogSourceId, publication: &CatalogPublication) {
        let mut urls = publication.image_url.iter().chain(publication.metadata.images.iter().map(|image| &image.url)).collect::<Vec<_>>();
        urls.sort();
        urls.dedup();
        for image_url in urls {
            let key = format!("{}\n{image_url}", source_id.as_str());
            let reference = RecentCoverReference { identity: publication.identity.clone(), acquisition: publication_metadata_acquisition(publication) };
            self.entries.insert(key.clone(), reference);
            self.touch(&key);
        }
        while self.entries.len() > MAX_RECENT_BROWSER_COVERS {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    fn get(&mut self, source_id: &CatalogSourceId, image_url: &str) -> Option<RecentCoverReference> {
        let key = format!("{}\n{image_url}", source_id.as_str());
        let value = self.entries.get(&key).cloned();
        if value.is_some() {
            self.touch(&key);
        }
        value
    }

    fn touch(&mut self, key: &str) {
        if let Some(index) = self.order.iter().position(|candidate| candidate == key) {
            self.order.remove(index);
        }
        self.order.push_back(key.to_owned());
    }
}

#[derive(Deserialize)]
struct CoverQuery {
    image_url: String,
}

#[derive(Serialize)]
struct CatalogInventoryResponse {
    status: &'static str,
    uptime_seconds: u64,
    registry_revision: u64,
    metadata_enrichment_configured: bool,
    search_index_ready: bool,
    active_page_count: u64,
    active_publication_count: u64,
    active_epub_count: u64,
    cover_downloads: CoverDownloadInventoryResponse,
    epub_extractions: EpubExtractionInventoryResponse,
    metadata_backfill: MetadataBackfillInventoryResponse,
    description_backfill: DescriptionBackfillInventoryResponse,
    description_backfill_available: bool,
    metadata_backfill_metrics: MetadataBackfillRuntimeMetricsResponse,
    description_backfill_metrics: MetadataBackfillRuntimeMetricsResponse,
    processes: Vec<CatalogProcessStatus>,
    writer_metrics: BTreeMap<String, WriterRuntimeMetricResponse>,
    identity_reviews: catalog_core::CatalogIdentityReviewInventory,
    sources: Vec<CatalogSourceInventoryResponse>,
}

#[derive(Serialize)]
struct CoverDownloadInventoryResponse {
    pending: u64,
    running: u64,
    succeeded: u64,
    failed: u64,
    attempts: u64,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct MetadataBackfillInventoryResponse {
    pending: u64,
    running: u64,
    succeeded: u64,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct DescriptionBackfillInventoryResponse {
    pending: u64,
    running: u64,
    succeeded: u64,
    no_description: u64,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct CatalogOperationsResponse {
    #[serde(flatten)]
    inventory: CatalogInventoryResponse,
    extraction_failures: Vec<EpubFailureDiagnosticResponse>,
}

#[derive(Serialize)]
struct EpubFailureDiagnosticResponse {
    category: String,
    retryable: bool,
    count: u64,
    first_failed_at: u64,
    last_failed_at: u64,
    next_retry_at: Option<u64>,
    examples: Vec<EpubFailureExampleResponse>,
}

#[derive(Serialize)]
struct EpubFailureExampleResponse {
    source_id: String,
    acquisition_host: String,
    publication_id: String,
    attempts: u64,
    last_failed_at: u64,
}

#[derive(Serialize)]
struct EpubExtractionInventoryResponse {
    pending: u64,
    running: u64,
    downloading: u64,
    extracting: u64,
    enriching: u64,
    succeeded: u64,
    failed: u64,
    attempts: u64,
    downloaded_bytes: u64,
}

#[derive(Serialize)]
struct CatalogSourceInventoryResponse {
    source_id: String,
    crawl_status: String,
    last_crawl_started_at: Option<u64>,
    last_crawl_succeeded_at: Option<u64>,
    last_crawl_error_category: Option<&'static str>,
    crawl_frontier_count: u64,
    crawl_pages_fetched: u64,
    crawl_publications_seen: u64,
    crawl_elapsed_seconds: u64,
    crawl_origin_in_flight: usize,
    crawl_origin_concurrency: usize,
    crawl_origin_spacing_ms: u64,
    crawl_origin_blocked_ms: Option<u64>,
    active_page_count: u64,
    active_publication_count: u64,
    active_epub_count: u64,
    epub_extractions: EpubExtractionInventoryResponse,
}

#[derive(Deserialize)]
struct IdentityReviewQuery {
    source_id: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct IdentityReviewResponse {
    inventory: catalog_core::CatalogIdentityReviewInventory,
    reviews: Vec<catalog_core::CatalogIdentityReview>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum IdentityReviewAction {
    SelectCandidate { edition_id: String, isbn13: String },
    ManualIsbn { isbn: String },
    Reject,
    Retry,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

#[derive(Debug)]
pub struct CatalogServiceError(String);

mod backfill;
mod covers;
mod crawl;
mod extraction;
mod http;
mod metadata;
mod metrics;
mod processes;
mod registry;
mod service;

pub(crate) use crawl::{canonicalize_page_layout_locators, classify_catalog_failure, reconcile_cached_sources};
pub use extraction::run_catalog_extraction_worker;
pub use http::{app, operations_app};
pub(crate) use metadata::{
    apply_catalog_description_if_missing, apply_catalog_metadata, catalog_metadata_match, extraction_book_format, merge_catalog_epub_metadata, merge_catalog_popularity, metadata_publication_byte_limit, publication_cover_fallback,
    publication_has_acquisition_url, publication_metadata_acquisition,
};
pub(crate) use metrics::{duration_millis, writer_metrics_snapshot, BackfillBatchMetrics};
pub(crate) use registry::catalog_crawls_disabled;
pub(crate) use service::{cache_error, default_cache_path, default_cover_cache_directory};

#[cfg(test)]
mod tests;

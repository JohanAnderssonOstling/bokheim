use axum::body::{Body, Bytes};
use axum::extract::{Json, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use catalog_contract::{
    materialize_page, CatalogAuthorCredit, CatalogBookFormat, CatalogDescription, CatalogImportRequest, CatalogLinkTarget, CatalogPage, CatalogPageRequest, CatalogPageResponse, CatalogPublication, CatalogPublicationAction,
    CatalogPublicationIdentity, CatalogPublicationPayload, CatalogPublicationReference, CatalogRecommendationRequest, CatalogRecommendationResponse, CatalogSearchRequest, CatalogSearchResponse, CatalogSource, CatalogSourceId,
    CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse,
};
use catalog_core::{
    canonicalize_opds_catalog_locator, opds_catalog_source_id, opds_page, opds_source, validate_opds_catalog_locator, validated_opds_import, CatalogCache, CatalogCacheError, CatalogCoverCache, CatalogCoverPipeline,
    CatalogEpubExtractionJob, CatalogEpubMetadata, CatalogInventory, CatalogLanguageFilter, CatalogMetadataBackfillJob, CatalogMetadataBackfillUpdate, CatalogPublicationPopularity,
};
use futures_util::stream::{self, StreamExt};
use library_model::{canonical_isbn, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType, PublicationSubject};
use metadata_contract::{ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityRequest, EditionIdentityResponse, EditionIdentityStatus, LookupStatus, MetadataEnrichmentRequest, RichWorkMetadataMatch};
use opds::{
    endpoints_from_registry, validate_curated_registry, CatalogRegistry, DownloadedOpdsBook, OpdsAcquisitionKind, OpdsBookFormat, OpdsClient, OpdsConditionalCatalog, OpdsDirectAcquisition, OpdsEndpoint, OpdsHttpValidators, OpdsSearch,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, Notify, RwLock, Semaphore};
use tower_http::cors::{Any, CorsLayer};
use url::Url;

const MAX_REGISTRY_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_CRAWL_PAGES: usize = 50_000;
const HARD_MAX_CRAWL_PAGES: usize = 250_000;
const COVER_DOWNLOAD_WORKERS: usize = 4;
// Image decoders have a high transient working set and the system allocator
// may retain those arenas after the image is dropped. One worker keeps cover
// generation comfortably below the service's memory budget.
const COVER_THUMBNAIL_WORKERS: usize = 1;
const MAX_RECENT_BROWSER_COVERS: usize = 4_096;
// SQLite has one writer. Sources are all persisted as queued immediately, but
// staging their feed generations serially avoids writer-lock contention while
// publication enrichment continues independently through its own bounded workers.
// Catalogue traversal is dominated by upstream response latency. Sources have
// independent origins, so overlap several source crawls while each individual
// source remains serial and polite to its host.
const CRAWL_CONCURRENCY: usize = 3;
const CRAWL_FRONTIER_BATCH_SIZE: usize = 64;
const SQLITE_LOCK_RETRY_DELAYS: [Duration; 3] = [Duration::from_millis(100), Duration::from_millis(250), Duration::from_millis(500)];
const CRAWL_ACTIVATION_BATCH: usize = 256;
const EXTRACTION_RECONCILIATION_BATCH: usize = 256;
const INACTIVE_GENERATION_CLEANUP_BATCH: usize = 256;
const PRIORITY_CRAWL_SOURCE: &str = "litteraturbanken";
// Downloads stream to disk and are cheap in heap terms. Keep extraction and
// image decoding serialized, but overlap several upstream transfers with it.
const MAX_CONCURRENT_DOWNLOADS: usize = 4;
const MAX_INTERACTIVE_DOWNLOADS: usize = 2;
const MAX_QUEUED_DOWNLOADS: usize = 4;
const MAX_CONCURRENT_EXTRACTIONS: usize = 1;
const EXTRACTION_WORKER_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
const EXTRACTION_WORKER_CPU_SECONDS: u64 = 60;
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
const METADATA_BACKFILL_BATCH_SIZE: usize = 1;
const METADATA_IDENTITY_BACKFILL_BATCH_SIZE: usize = 16;
const METADATA_IDENTITY_BACKFILL_CONCURRENCY: usize = 4;
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
    cover_pipeline: CatalogCoverPipeline,
    // SQLite permits only one writer. Queue writes before entering the
    // blocking pool so in-process jobs never spend the busy timeout fighting
    // one another (or turn ordinary contention into extraction failures).
    cache_write_lock: Mutex<()>,
    crawl_locks: Mutex<HashMap<CatalogSourceId, Arc<Mutex<()>>>>,
    crawl_progress: Mutex<HashMap<CatalogSourceId, CrawlProgress>>,
    source_update_locks: Mutex<HashMap<CatalogSourceId, Arc<Mutex<()>>>>,
    recent_browser_covers: Mutex<RecentBrowserCovers>,
    epub_range_origins: Mutex<HashMap<String, bool>>,
    background_download_slots: Semaphore,
    interactive_download_slots: Semaphore,
    queued_download_slots: Semaphore,
    extraction_slots: Semaphore,
    extraction_notify: Notify,
    acquisition_directory: PathBuf,
    metadata_endpoint: Option<Url>,
    metadata_client: reqwest::Client,
    max_crawl_pages: usize,
    started_at: Instant,
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

#[derive(Clone)]
struct RecentCoverReference {
    identity: CatalogPublicationIdentity,
    acquisition: Option<OpdsDirectAcquisition>,
}

#[derive(Clone, Default)]
struct CatalogMetadataMatch {
    subjects: Vec<PublicationSubject>,
    popularity: Option<CatalogPublicationPopularity>,
}

#[derive(Default)]
struct RecentBrowserCovers {
    entries: HashMap<String, RecentCoverReference>,
    order: VecDeque<String>,
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
                     pending INTEGER NOT NULL CHECK(pending IN (0,1))
                 );
                 CREATE INDEX frontier_pending ON frontier(pending, ordinal);",
            )
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        connection.execute("INSERT INTO frontier(locator,pending) VALUES(?1,1)", [root]).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(Self { _directory: directory, connection, discovered: 1, limit: limit.max(1) })
    }

    fn enqueue(&mut self, locators: impl IntoIterator<Item = String>) -> Result<(), CatalogServiceError> {
        let transaction = self.connection.transaction().map_err(|error| CatalogServiceError(error.to_string()))?;
        for locator in locators {
            let inserted = transaction.execute("INSERT OR IGNORE INTO frontier(locator,pending) VALUES(?1,1)", [locator]).map_err(|error| CatalogServiceError(error.to_string()))?;
            if inserted != 0 {
                self.discovered = self.discovered.saturating_add(1);
                if self.discovered > self.limit {
                    return Err(CatalogServiceError(format!("catalog crawl exceeded {} discovered pages", self.limit)));
                }
            }
        }
        transaction.commit().map_err(|error| CatalogServiceError(error.to_string()))
    }

    fn claim_batch(&mut self) -> Result<Vec<String>, CatalogServiceError> {
        let transaction = self.connection.transaction().map_err(|error| CatalogServiceError(error.to_string()))?;
        let locators = {
            let mut statement = transaction.prepare("SELECT locator FROM frontier WHERE pending=1 ORDER BY ordinal LIMIT ?1").map_err(|error| CatalogServiceError(error.to_string()))?;
            let rows = statement.query_map([CRAWL_FRONTIER_BATCH_SIZE as i64], |row| row.get::<_, String>(0)).map_err(|error| CatalogServiceError(error.to_string()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|error| CatalogServiceError(error.to_string()))?
        };
        for locator in &locators {
            transaction.execute("UPDATE frontier SET pending=0 WHERE locator=?1", [locator]).map_err(|error| CatalogServiceError(error.to_string()))?;
        }
        transaction.commit().map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(locators)
    }
}

impl RecentBrowserCovers {
    fn remember(&mut self, source_id: &CatalogSourceId, publication: &CatalogPublication) {
        let Some(image_url) = publication.image_url.as_deref() else { return };
        let key = format!("{}\n{image_url}", source_id.as_str());
        let reference = RecentCoverReference { identity: publication.identity.clone(), acquisition: publication_metadata_acquisition(publication) };
        self.entries.insert(key.clone(), reference);
        self.touch(&key);
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
    active_page_count: u64,
    active_publication_count: u64,
    active_epub_count: u64,
    epub_extractions: EpubExtractionInventoryResponse,
    metadata_backfill: MetadataBackfillInventoryResponse,
    sources: Vec<CatalogSourceInventoryResponse>,
}

#[derive(Serialize)]
struct MetadataBackfillInventoryResponse {
    pending: u64,
    running: u64,
    succeeded: u64,
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
    active_page_count: u64,
    active_publication_count: u64,
    active_epub_count: u64,
    epub_extractions: EpubExtractionInventoryResponse,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

#[derive(Debug)]
pub struct CatalogServiceError(String);

impl CatalogService {
    async fn begin_crawl_progress(&self, source_id: &CatalogSourceId) {
        self.state.crawl_progress.lock().await.insert(source_id.clone(), CrawlProgress { started_at: Instant::now(), pages_fetched: 0, publications_seen: 0 });
    }

    async fn record_crawl_page(&self, source_id: &CatalogSourceId, publications: u64) {
        if let Some(progress) = self.state.crawl_progress.lock().await.get_mut(source_id) {
            progress.pages_fetched = progress.pages_fetched.saturating_add(1);
            progress.publications_seen = progress.publications_seen.saturating_add(publications);
        }
    }

    pub fn bundled() -> Result<Self, CatalogServiceError> {
        Self::from_bytes(RegistrySource::Bundled, BUNDLED_REGISTRY.to_vec(), None, default_cover_cache_directory())
    }

    pub fn bundled_with_storage(cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        Self::from_bytes(RegistrySource::Bundled, BUNDLED_REGISTRY.to_vec(), cache_path, cover_cache_directory)
    }

    pub async fn from_file_with_storage(path: impl Into<PathBuf>, cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        let path = path.into();
        let bytes = read_registry_file(&path).await?;
        Self::from_bytes(RegistrySource::File(path), bytes, cache_path, cover_cache_directory)
    }

    pub async fn validate_file(path: &Path) -> Result<CatalogRegistry, CatalogServiceError> {
        parse_registry(read_registry_file(path).await?)
    }

    pub async fn registry(&self) -> CatalogRegistry {
        self.state.snapshot.read().await.registry.clone()
    }

    async fn refresh_registry(&self) {
        let RegistrySource::File(path) = &self.state.source else { return };
        let bytes = match read_registry_file(path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "retaining the last valid catalog registry");
                return;
            }
        };
        {
            let snapshot = self.state.snapshot.read().await;
            if snapshot.observed_bytes == bytes {
                return;
            }
        }
        let registry = match parse_registry(bytes.clone()) {
            Ok(registry) => registry,
            Err(error) => {
                tracing::warn!(%error, "ignoring invalid registry update");
                self.state.snapshot.write().await.observed_bytes = bytes;
                return;
            }
        };
        let active = self.state.snapshot.read().await.registry.clone();
        if registry.revision > active.revision {
            let endpoints = match endpoints_from_registry(registry.clone()) {
                Ok(endpoints) => endpoints,
                Err(error) => {
                    tracing::warn!(%error, "ignoring registry whose validated endpoints could not be constructed");
                    self.state.snapshot.write().await.observed_bytes = bytes;
                    return;
                }
            };
            if let Err(error) = self.reconcile_sources(&endpoints).await {
                tracing::warn!(%error, "retaining the last catalog registry after cache reconciliation failed");
                return;
            }
            *self.state.endpoints.write().await = endpoints;
            let mut snapshot = self.state.snapshot.write().await;
            snapshot.observed_bytes = bytes;
            snapshot.registry = registry;
        } else {
            if registry.revision == active.revision && registry != active {
                tracing::warn!(revision = registry.revision, "registry changed without a revision increment");
            } else if registry.revision < active.revision {
                tracing::warn!(candidate_revision = registry.revision, active_revision = active.revision, "ignoring stale registry");
            }
            self.state.snapshot.write().await.observed_bytes = bytes;
        }
    }

    pub async fn crawl_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let lock = {
            let mut locks = self.state.crawl_locks.lock().await;
            locks.entry(source_id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
        };
        let _crawl = lock.lock().await;
        self.crawl_endpoint(endpoint).await
    }

    async fn source_update_lock(&self, source_id: &CatalogSourceId) -> Arc<Mutex<()>> {
        let mut locks = self.state.source_update_locks.lock().await;
        locks.entry(source_id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    async fn cache_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogCache) -> Result<T, CatalogCacheError> + Send + 'static,
    {
        let cache = self.state.cache.clone();
        tokio::task::spawn_blocking(move || operation(&cache)).await.map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}")))?.map_err(cache_error)
    }

    async fn cache_write_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: Fn(&CatalogCache) -> Result<T, CatalogCacheError> + Send + Sync + 'static,
    {
        let _writer = self.state.cache_write_lock.lock().await;
        let cache = self.state.cache.clone();
        tokio::task::spawn_blocking(move || {
            for delay in SQLITE_LOCK_RETRY_DELAYS {
                match operation(&cache) {
                    Err(error) if error.is_database_locked() => std::thread::sleep(delay),
                    result => return result,
                }
            }
            operation(&cache)
        })
        .await
        .map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}")))?
        .map_err(cache_error)
    }

    async fn cleanup_inactive_generations(&self, source_id: CatalogSourceId) {
        loop {
            let cache_source = source_id.clone();
            match self.cache_write_call(move |cache| cache.cleanup_inactive_generation_batch(&cache_source, INACTIVE_GENERATION_CLEANUP_BATCH)).await {
                Ok(true) => tokio::time::sleep(Duration::from_millis(10)).await,
                Ok(false) => return,
                Err(error) => {
                    tracing::warn!(source = source_id.as_str(), %error, "could not finish bounded inactive-generation cleanup");
                    return;
                }
            }
        }
    }

    async fn reconcile_active_epub_extractions(&self, source_id: CatalogSourceId) -> Result<(), CatalogServiceError> {
        loop {
            let cache_source = source_id.clone();
            let more = self.cache_write_call(move |cache| cache.reconcile_active_epub_extractions_batch(&cache_source, EXTRACTION_RECONCILIATION_BATCH)).await?;
            if !more {
                self.state.extraction_notify.notify_waiters();
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    }

    async fn blocking_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(CatalogCache) -> Result<T, CatalogServiceError> + Send + 'static,
    {
        let cache = self.state.cache.clone();
        tokio::task::spawn_blocking(move || operation(cache)).await.map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}")))?
    }

    async fn crawl_endpoint(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.cache_write_call(move |cache| cache.ensure_source(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not reconcile crawl source: {error}")))?;
        let cache_source = source_id.clone();
        let has_pages = self.cache_call(move |cache| cache.has_pages(&cache_source)).await.map_err(|error| CatalogServiceError(format!("could not inspect cached crawl pages: {error}")))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        let has_frontier = self.cache_call(move |cache| cache.has_missing_page_frontier(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not inspect crawl frontier: {error}")))?;
        if !has_pages || has_frontier {
            return self.crawl_endpoint_incrementally(endpoint).await;
        }
        // Atomic replacement crawls must remain serialized with enrichment so
        // activation cannot discard metadata applied to the previous active
        // generation. Incremental discovery below locks only each page commit.
        let update_lock = self.source_update_lock(&source_id).await;
        let _update = update_lock.lock().await;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        let crawl = self.cache_write_call(move |cache| cache.begin_crawl(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not start replacement crawl: {error}")))?;
        self.begin_crawl_progress(&source_id).await;
        let mut frontier = DiskCrawlFrontier::new(&endpoint.url, self.state.max_crawl_pages)?;
        let result = async {
            loop {
                let batch = frontier.claim_batch()?;
                if batch.is_empty() {
                    break;
                }
                for locator in batch {
                    validate_opds_catalog_locator(&endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
                    let cache_source = source_id.clone();
                    let cache_locator = locator.clone();
                    let cached = self.cache_call(move |cache| cache.page(&cache_source, &cache_locator)).await?;
                    let validators = cached.as_ref().map(|page| OpdsHttpValidators { etag: page.etag.clone(), last_modified: page.last_modified.clone() }).unwrap_or_default();
                    let (mut page, validators) = match self.state.opds.fetch_catalog_conditional(&locator, &validators).await.map_err(|error| CatalogServiceError(error.to_string()))? {
                        OpdsConditionalCatalog::NotModified => {
                            let cached = cached.ok_or_else(|| CatalogServiceError("upstream returned 304 for an uncached catalog page".to_owned()))?;
                            (cached.page, validators)
                        }
                        OpdsConditionalCatalog::Modified { catalog, validators } => {
                            validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
                            (opds_page(source_id.clone(), catalog), validators)
                        }
                    };
                    let cache_source = source_id.clone();
                    page = self
                        .cache_call(move |cache| {
                            cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                            Ok(page)
                        })
                        .await?;
                    self.prepare_publication_cover_fallbacks(&source_id, &mut page).await;
                    frontier.enqueue(internal_page_links(&root_url, &page)?)?;
                    let publications = page_publication_count(&page);
                    let cover_urls = page_cover_urls(&page);
                    let cache_crawl = crawl.clone();
                    let cache_page = page;
                    self.cache_write_call(move |cache| cache.stage_page(&cache_crawl, &cache_page, validators.etag.as_deref(), validators.last_modified.as_deref())).await?;
                    self.record_crawl_page(&source_id, publications).await;
                    self.state.cover_pipeline.enqueue(source_id.clone(), cover_urls).await;
                }
            }
            loop {
                let cache_crawl = crawl.clone();
                let more = self.cache_write_call(move |cache| cache.prepare_crawl_activation_batch(&cache_crawl, CRAWL_ACTIVATION_BATCH)).await?;
                if !more {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let cache_crawl = crawl.clone();
            self.cache_write_call(move |cache| cache.finalize_crawl_activation(&cache_crawl)).await?;
            self.reconcile_active_epub_extractions(source_id.clone()).await?;
            let cleanup_service = self.clone();
            let cleanup_source = source_id.clone();
            tokio::spawn(async move { cleanup_service.cleanup_inactive_generations(cleanup_source).await });
            let service = self.clone();
            let enrichment_source = source_id.clone();
            tokio::spawn(async move { service.enrich_active_source(enrichment_source, true).await });
            Ok::<(), CatalogServiceError>(())
        }
        .await;
        if let Err(error) = &result {
            let message = error.to_string();
            let cache_crawl = crawl.clone();
            let _ = self.cache_write_call(move |cache| cache.abandon_crawl_with_error(&cache_crawl, Some(&message))).await;
            let cleanup_service = self.clone();
            let cleanup_source = source_id.clone();
            tokio::spawn(async move { cleanup_service.cleanup_inactive_generations(cleanup_source).await });
        }
        result
    }

    async fn crawl_endpoint_incrementally(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.cache_write_call(move |cache| cache.begin_incremental_crawl(&cache_source, &cache_root)).await?;
        self.begin_crawl_progress(&source_id).await;
        let mut frontier = DiskCrawlFrontier::new(&endpoint.url, self.state.max_crawl_pages)?;
        let mut fetched = 0usize;
        let result = async {
            loop {
                let batch = frontier.claim_batch()?;
                if batch.is_empty() {
                    break;
                }
                for locator in batch {
                    validate_opds_catalog_locator(&endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
                    let cache_source = source_id.clone();
                    let cache_locator = locator.clone();
                    if let Some(cached) = self.cache_call(move |cache| cache.page(&cache_source, &cache_locator)).await? {
                        frontier.enqueue(internal_page_links(&root_url, &cached.page)?)?;
                        continue;
                    }
                    if fetched >= self.state.max_crawl_pages {
                        return Ok(false);
                    }
                    fetched += 1;
                    let catalog = self.state.opds.fetch_catalog_fresh(&locator).await.map_err(|error| CatalogServiceError(error.to_string()))?;
                    validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
                    let mut page = opds_page(source_id.clone(), catalog);
                    frontier.enqueue(internal_page_links(&root_url, &page)?)?;
                    let (publications, cover_urls) = {
                        // Network traversal is deliberately outside this lock.
                        // Re-read extraction results after acquiring it so either
                        // the page commit or enrichment wins without lost fields.
                        let update_lock = self.source_update_lock(&source_id).await;
                        let _update = update_lock.lock().await;
                        let cache_source = source_id.clone();
                        page = self
                            .cache_call(move |cache| {
                                cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                                Ok(page)
                            })
                            .await?;
                        self.prepare_publication_cover_fallbacks(&source_id, &mut page).await;
                        let publications = page_publication_count(&page);
                        let cover_urls = page_cover_urls(&page);
                        let cache_source = source_id.clone();
                        let cache_root = endpoint.url.clone();
                        let cache_page = page;
                        self.cache_write_call(move |cache| cache.put_page(&cache_source, &cache_root, &cache_page, None, None)).await?;
                        (publications, cover_urls)
                    };
                    self.record_crawl_page(&source_id, publications).await;
                    self.state.cover_pipeline.enqueue(source_id.clone(), cover_urls).await;
                }
            }
            Ok::<_, CatalogServiceError>(true)
        }
        .await;
        let complete = result.as_ref().copied().unwrap_or(false);
        let cache_source = source_id.clone();
        self.cache_write_call(move |cache| cache.finish_incremental_crawl(&cache_source, complete)).await?;
        let service = self.clone();
        let enrichment_source = source_id.clone();
        tokio::spawn(async move { service.enrich_active_source(enrichment_source, true).await });
        result.map(|_| ())
    }

    pub async fn run_refresh_loop(self) {
        loop {
            self.refresh_all_sources().await;
            tokio::time::sleep(CRAWL_CYCLE).await;
        }
    }

    pub async fn run_registry_refresh_loop(self) {
        loop {
            self.refresh_registry().await;
            tokio::time::sleep(REGISTRY_REFRESH_INTERVAL).await;
        }
    }

    /// Drains durable pending extraction rows independently of catalogue
    /// crawling. Successful and failed rows are left untouched; interrupted
    /// `running` rows are changed back to `pending` when the cache opens.
    pub async fn run_epub_extraction_loop(self) {
        loop {
            tokio::join!(self.resume_pending_epub_extractions(), self.resume_metadata_backfill());
            tokio::time::sleep(EPUB_EXTRACTION_SWEEP_INTERVAL).await;
        }
    }

    async fn resume_metadata_backfill(&self) {
        loop {
            match self.cache_write_call(|cache| cache.schedule_metadata_backfill()).await {
                Ok(1000) => tokio::task::yield_now().await,
                Ok(_) => break,
                Err(error) => {
                    tracing::warn!(%error, "could not schedule saved catalogue metadata for local backfill");
                    return;
                }
            }
        }
        loop {
            let jobs = match self.cache_write_call(|cache| cache.claim_metadata_backfill_batch(METADATA_BACKFILL_BATCH_SIZE)).await {
                Ok(jobs) if jobs.is_empty() => return,
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::warn!(%error, "could not claim saved catalogue metadata backfill batch");
                    return;
                }
            };
            match self.enrich_saved_metadata_batch(&jobs).await {
                Ok(updates) => {
                    if let Err(error) = self.cache_write_call(move |cache| cache.finish_metadata_backfill_batch(&updates)).await {
                        tracing::warn!(%error, "could not commit saved catalogue metadata backfill batch");
                        let release = jobs.clone();
                        let message = error.to_string();
                        let _ = self.cache_write_call(move |cache| cache.release_metadata_backfill_batch(&release, &message)).await;
                        return;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "saved catalogue metadata backfill batch failed");
                    let release = jobs.clone();
                    let message = error.to_string();
                    let _ = self.cache_write_call(move |cache| cache.release_metadata_backfill_batch(&release, &message)).await;
                    return;
                }
            }
        }
    }

    async fn resume_pending_epub_extractions(&self) {
        let source_ids = self.state.endpoints.read().await.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        for source_id in &source_ids {
            let cache_source = source_id.clone();
            if let Err(error) = self.cache_write_call(move |cache| cache.retry_failed_epub_extractions(&cache_source)).await {
                tracing::warn!(source = source_id.as_str(), %error, "could not schedule due publication extraction retries");
            }
        }
        // Independent workers overlap network transfers while the extraction
        // semaphore below keeps ZIP/image parsing serialized. Rotating each
        // worker's starting source prevents one large provider monopolizing
        // the bounded on-disk queue.
        let mut workers = tokio::task::JoinSet::new();
        for offset in 0..MAX_CONCURRENT_DOWNLOADS {
            let service = self.clone();
            let mut worker_sources = source_ids.clone();
            if !worker_sources.is_empty() {
                let rotation = offset % worker_sources.len();
                worker_sources.rotate_left(rotation);
            }
            workers.spawn(async move {
                loop {
                    let mut progressed = false;
                    for source_id in &worker_sources {
                        progressed |= service.enrich_next_publication(source_id).await;
                    }
                    if !progressed {
                        break;
                    }
                }
            });
        }
        while let Some(result) = workers.join_next().await {
            if let Err(error) = result {
                tracing::warn!(%error, "catalogue download worker stopped unexpectedly");
            }
        }
    }

    async fn refresh_all_sources(&self) {
        let mut endpoints = self.state.endpoints.read().await.clone();
        prioritize_startup_crawls(&mut endpoints);
        let source_ids = endpoints.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        if let Err(error) = self.cache_write_call(move |cache| cache.queue_crawls(&source_ids)).await {
            tracing::warn!(%error, "could not persist the startup crawl queue");
        }
        stream::iter(endpoints)
            .for_each_concurrent(Some(CRAWL_CONCURRENCY), |endpoint| {
                let service = self.clone();
                async move {
                    let source_id = opds_catalog_source_id(&endpoint.id);
                    if let Err(error) = service.crawl_source(&source_id).await {
                        tracing::warn!(source = source_id.as_str(), %error, "catalog crawl failed; retaining the previous generation");
                    }
                }
            })
            .await;
    }

    pub async fn page_response(&self, source_id: &CatalogSourceId, request: CatalogPageRequest) -> Result<CatalogPageResponse, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = canonicalize_opds_catalog_locator(&endpoint, request.locator.as_deref().unwrap_or(&endpoint.url)).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_locator = locator.clone();
        if self.cache_call(move |cache| cache.page(&cache_source, &cache_locator)).await?.is_none() {
            self.cache_page_on_demand(source_id, &endpoint, &locator).await?;
        }
        let source = opds_source(&endpoint);
        let filter = CatalogLanguageFilter::new(&request.supported_languages);
        if !filter.source_is_multilingual(&source) {
            let cache_source = source_id.clone();
            let cache_locator = locator.clone();
            let epoch = request.epoch.clone();
            let known = request.known_publications.clone();
            let mut response =
                self.cache_call(move |cache| cache.page_response(&cache_source, &cache_locator, epoch.as_deref(), &known)).await?.ok_or_else(|| CatalogServiceError("catalog has no active cached generation for this page".to_owned()))?;
            canonicalize_page_layout_locators(&endpoint, &mut response.page)?;
            return Ok(response);
        }

        let cache_source = source_id.clone();
        let cache_locator = locator.clone();
        let mut response = self.cache_call(move |cache| cache.page_response(&cache_source, &cache_locator, None, &[])).await?.ok_or_else(|| CatalogServiceError("catalog has no active cached generation for this page".to_owned()))?;
        let supported = response.payloads.iter().filter(|payload| filter.publication_supported(&payload.publication)).map(|payload| payload.publication.identity.clone()).collect::<HashSet<_>>();
        response.page.publications.retain(|publication| supported.contains(&publication.identity));
        for group in &mut response.page.groups {
            group.publications.retain(|publication| supported.contains(&publication.identity));
        }
        retain_delta_payloads(&mut response.payloads, response.page.publications.iter().chain(response.page.groups.iter().flat_map(|group| group.publications.iter())), request.epoch.as_deref(), &response.epoch, request.known_publications);
        canonicalize_page_layout_locators(&endpoint, &mut response.page)?;
        Ok(response)
    }

    async fn cache_page_on_demand(&self, source_id: &CatalogSourceId, endpoint: &OpdsEndpoint, locator: &str) -> Result<(), CatalogServiceError> {
        let catalog = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.opds.fetch_catalog_fresh(locator))
            .await
            .map_err(|_| CatalogServiceError("catalogue page did not respond before the upstream timeout".to_owned()))?
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        validate_opds_catalog_locator(endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let mut page = opds_page(source_id.clone(), catalog);
        let cache_source = source_id.clone();
        page = self
            .cache_call(move |cache| {
                cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                Ok(page)
            })
            .await?;
        self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
        let cache_source = source_id.clone();
        let root = endpoint.url.clone();
        let cached_page = page.clone();
        self.cache_write_call(move |cache| cache.put_page(&cache_source, &root, &cached_page, None, None)).await?;
        self.remember_browser_covers(source_id, &page).await;
        let service = self.clone();
        let enrichment_source = source_id.clone();
        tokio::spawn(async move { service.enrich_active_source(enrichment_source, true).await });
        Ok(())
    }

    pub async fn search_response(&self, source_id: &CatalogSourceId, request: CatalogSearchRequest) -> Result<CatalogSearchResponse, CatalogServiceError> {
        validate_query(&request.query)?;
        let endpoint = self.public_endpoint(source_id).await?;
        let source = opds_source(&endpoint);
        let filter = CatalogLanguageFilter::new(&request.supported_languages);
        let cache_source = source_id.clone();
        if !self.cache_call(move |cache| cache.has_pages(&cache_source)).await? {
            return Err(CatalogServiceError("catalog has no active cached generation".to_owned()));
        }
        if !filter.source_is_multilingual(&source) {
            let cache_source = source_id.clone();
            let query = request.query.clone();
            let epoch = request.epoch.clone();
            let known = request.known_publications.clone();
            return self.cache_call(move |cache| cache.search_response(&cache_source, &query, epoch.as_deref(), &known)).await?.ok_or_else(|| CatalogServiceError("catalog has not been crawled".to_owned()));
        }
        let cache_source = source_id.clone();
        let query = request.query.clone();
        let mut response = self.cache_call(move |cache| cache.search_response(&cache_source, &query, None, &[])).await?.ok_or_else(|| CatalogServiceError("catalog has not been crawled".to_owned()))?;
        let supported = response.payloads.iter().filter(|payload| filter.publication_supported(&payload.publication)).map(|payload| payload.publication.identity.clone()).collect::<HashSet<_>>();
        response.title_matches.retain(|item| supported.contains(&item.identity));
        response.author_matches.retain(|item| supported.contains(&item.identity));
        response.toc_matches.retain(|item| supported.contains(&item.identity));
        retain_delta_payloads(&mut response.payloads, response.title_matches.iter().chain(&response.author_matches).chain(&response.toc_matches), request.epoch.as_deref(), &response.epoch, request.known_publications);
        Ok(response)
    }

    /// Browser clients request a complete JSON page and cannot retain the
    /// binary delta protocol. Materialize a full page from the active cache so
    /// opening a catalogue never waits for its upstream OPDS host. Only pages
    /// absent from the active crawl generation fall back to an on-demand fetch.
    pub async fn browser_page(&self, source_id: &CatalogSourceId, request: CatalogPageRequest) -> Result<CatalogPage, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = canonicalize_opds_catalog_locator(&endpoint, request.locator.as_deref().unwrap_or(&endpoint.url)).map_err(|error| CatalogServiceError(error.to_string()))?;
        if let Ok(response) = self.page_response(source_id, request.clone()).await {
            let payloads = response.payloads.into_iter().map(|payload| (payload.publication.identity.clone(), (payload.sequence, payload.publication))).collect::<HashMap<_, _>>();
            if let Some(mut page) = materialize_page(response.page, |reference| payloads.get(&reference.identity).filter(|(sequence, _)| *sequence == reference.sequence).map(|(_, publication)| publication.clone())) {
                self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
                self.remember_browser_covers(source_id, &page).await;
                return Ok(page);
            }
        }
        let catalog = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.opds.fetch_catalog_fresh(&locator))
            .await
            .map_err(|_| CatalogServiceError("catalogue page did not respond before the browser timeout".to_owned()))?
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let mut page = filter_browser_page(opds_page(source_id.clone(), catalog), &opds_source(&endpoint), &request.supported_languages);
        self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
        self.remember_browser_covers(source_id, &page).await;
        Ok(page)
    }

    pub async fn browser_search(&self, source_id: &CatalogSourceId, request: CatalogSearchRequest) -> Result<CatalogPage, CatalogServiceError> {
        validate_query(&request.query)?;
        let endpoint = self.public_endpoint(source_id).await?;
        let operation = async {
            let root = self.state.opds.fetch_catalog_fresh(&endpoint.url).await.map_err(|error| CatalogServiceError(error.to_string()))?;
            validate_opds_catalog_locator(&endpoint, &root.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
            let search = root.search.as_ref().ok_or_else(|| CatalogServiceError("catalog does not advertise search".to_owned()))?;
            let search_origin = match search {
                OpdsSearch::UriTemplate { base_url, .. } => base_url,
                OpdsSearch::OpenSearchDescription { url } => url,
            };
            validate_opds_catalog_locator(&endpoint, search_origin).map_err(|error| CatalogServiceError(error.to_string()))?;
            let catalog = self.state.opds.search(&root, &request.query).await.map_err(|error| CatalogServiceError(error.to_string()))?;
            validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
            Ok::<_, CatalogServiceError>(catalog)
        };
        let catalog = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, operation).await.map_err(|_| CatalogServiceError("catalogue search did not respond before the browser timeout".to_owned()))??;
        let mut page = filter_browser_page(opds_page(source_id.clone(), catalog), &opds_source(&endpoint), &request.supported_languages);
        self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
        self.remember_browser_covers(source_id, &page).await;
        Ok(page)
    }

    async fn remember_browser_covers(&self, source_id: &CatalogSourceId, page: &CatalogPage) {
        let publications = page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).collect::<Vec<_>>();
        {
            let mut recent = self.state.recent_browser_covers.lock().await;
            for publication in &publications {
                recent.remember(source_id, publication);
            }
        }
        let urls = publications.into_iter().filter_map(|publication| publication.image_url.clone()).collect::<Vec<_>>();
        self.state.cover_pipeline.enqueue(source_id.clone(), urls.into_iter().filter(|url| !is_synthetic_cover_key(url))).await;
    }

    async fn prepare_publication_cover_fallbacks(&self, source_id: &CatalogSourceId, page: &mut CatalogPage) {
        for publication in page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())) {
            let _ = publication_cover_fallback(source_id, publication);
        }
    }

    async fn enrich_active_source(&self, source_id: CatalogSourceId, retry_failed: bool) {
        if retry_failed {
            let cache_source = source_id.clone();
            if let Err(error) = self.cache_write_call(move |cache| cache.retry_failed_epub_extractions(&cache_source)).await {
                tracing::warn!(source = source_id.as_str(), %error, "could not retry failed publication extractions");
                return;
            }
        }
        while self.enrich_next_publication(&source_id).await {}
    }

    async fn enrich_next_publication(&self, source_id: &CatalogSourceId) -> bool {
        // Bound claimed rows and completed temporary files together. Jobs that
        // cannot enter the disk-backed pipeline remain durably pending.
        let _queued_permit = match self.state.queued_download_slots.acquire().await {
            Ok(permit) => permit,
            Err(error) => {
                tracing::warn!(source = source_id.as_str(), %error, "catalogue download queue closed");
                return false;
            }
        };
        let cache_source = source_id.clone();
        let job = match self.cache_write_call(move |cache| cache.claim_next_epub_extraction(&cache_source)).await {
            Ok(Some(job)) => job,
            Ok(None) => return false,
            Err(error) => {
                tracing::warn!(source = source_id.as_str(), %error, "could not claim a queued publication extraction");
                return false;
            }
        };
        if let Err(error) = self.enrich_claimed_publication(job).await {
            tracing::warn!(source = source_id.as_str(), %error, "could not enrich active catalogue publication from embedded metadata");
        }
        true
    }

    async fn enrich_active_publication(&self, source_id: &CatalogSourceId, identity: CatalogPublicationIdentity, acquisition: OpdsDirectAcquisition, embedded_cover_key: Option<String>) -> Result<(), CatalogServiceError> {
        let _queued_permit = self.state.queued_download_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let acquisition_url = acquisition.url.clone();
        let cache_source = source_id.clone();
        let cache_url = acquisition_url.clone();
        let claimed = self.cache_write_call(move |cache| cache.begin_epub_extraction(&cache_source, &cache_url)).await?;
        if !claimed {
            return Ok(());
        }
        self.enrich_claimed_publication_with(source_id, identity, acquisition, embedded_cover_key).await
    }

    async fn enrich_claimed_publication(&self, job: CatalogEpubExtractionJob) -> Result<(), CatalogServiceError> {
        let book_format = extraction_book_format(job.book_format).ok_or_else(|| CatalogServiceError("queued publication has an unsupported extraction format".to_owned()))?;
        let digest = blake3::hash(job.acquisition_url.as_bytes()).to_hex();
        let acquisition = OpdsDirectAcquisition { url: job.acquisition_url, kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None };
        self.enrich_claimed_publication_with(&job.source_id, job.publication, acquisition, job.cover_key).await
    }

    async fn enrich_claimed_publication_with(&self, source_id: &CatalogSourceId, identity: CatalogPublicationIdentity, acquisition: OpdsDirectAcquisition, embedded_cover_key: Option<String>) -> Result<(), CatalogServiceError> {
        let acquisition_url = acquisition.url.clone();
        let result = async {
            let extract_cover = embedded_cover_key.is_some();
            let book_format = acquisition.book_format;
            let mut downloaded = self.download_catalog_metadata(&acquisition, extract_cover, true).await?;
            let mut downloaded_bytes = downloaded.transferred_bytes();
            let extracted = loop {
                let cache_source = source_id.clone();
                let cache_url = acquisition_url.clone();
                self.cache_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, "extracting")).await?;
                let extraction_permit = self.state.extraction_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
                let path = downloaded.path().to_owned();
                let extracted = extract_catalog_metadata_isolated(&path, book_format, extract_cover).await?;
                if downloaded.is_metadata_only() && extract_cover && extracted.thumbnail.is_none() {
                    // Ranged metadata succeeded but the declared artwork was
                    // absent or unsuitable. Preserve the existing broader
                    // cover fallback by retrying this one publication in full.
                    drop(extraction_permit);
                    drop(downloaded);
                    let cache_source = source_id.clone();
                    let cache_url = acquisition_url.clone();
                    self.cache_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, "downloading")).await?;
                    downloaded = self.download_catalog_metadata(&acquisition, extract_cover, false).await?;
                    downloaded_bytes = downloaded_bytes.saturating_add(downloaded.transferred_bytes());
                    continue;
                }
                drop(extraction_permit);
                break extracted;
            };
            drop(downloaded);
            let cache_source = source_id.clone();
            let cache_url = acquisition_url.clone();
            self.cache_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, "enriching")).await?;

            let cover_is_definitively_absent = match (embedded_cover_key.as_deref(), extracted.thumbnail) {
                (Some(key), Some(thumbnail)) => {
                    self.state.covers.remember_cover_url(source_id, key);
                    if let Err(error) = self.state.covers.cover_with(source_id, key, async move { Ok(thumbnail) }).await {
                        tracing::warn!(source = source_id.as_str(), %error, "could not cache embedded publication cover during metadata enrichment");
                    }
                    false
                }
                (Some(key), None) if is_synthetic_cover_key(key) => true,
                (Some(key), None) => match self.state.covers.cached_cover(key).await {
                    Ok(_) => false,
                    Err(_) => match self.state.covers.cover(source_id, key).await {
                        Ok(_) => false,
                        Err(error) if error.is_unsuitable_cover_dimensions() => true,
                        Err(_) => false,
                    },
                },
                (None, _) => true,
            };
            let mut saved_metadata = extracted.saved_metadata;
            let mut isbns = extracted.isbns;
            if isbns.is_empty() {
                match saved_metadata.as_mut() {
                    Some(metadata) => match self.infer_catalog_epub_isbn(metadata).await {
                        Ok(Some(isbn)) => isbns.push(isbn),
                        Ok(None) => {}
                        Err(error) => tracing::warn!(source = source_id.as_str(), %error, "could not infer catalogue EPUB ISBN from metadata snapshot"),
                    },
                    None => {}
                }
            }
            let mut subjects = extracted.subjects;
            let mut popularity = None;
            if !isbns.is_empty() {
                match self.metadata_matches(&isbns).await {
                    Ok(external) => {
                        for isbn in &isbns {
                            if let Some(enriched) = external.get(isbn) {
                                subjects.extend(enriched.subjects.iter().cloned());
                                if let Some(candidate) = &enriched.popularity {
                                    merge_catalog_popularity(&mut popularity, candidate);
                                }
                            }
                        }
                        subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
                        subjects.dedup();
                    }
                    Err(error) => tracing::warn!(source = source_id.as_str(), %error, "could not enrich catalogue subjects from metadata snapshot"),
                }
            }

            // A crawl may proceed while the network and EPUB parsing work is
            // happening, but activation and the final enriched update for one
            // source must not overlap. Re-read the active publication after
            // taking the lock so the result applies to the newest generation.
            let update_lock = self.source_update_lock(source_id).await;
            let _update = update_lock.lock().await;
            let cache_source = source_id.clone();
            let cache_identity = identity.clone();
            let mut publication = self.cache_call(move |cache| cache.active_publication(&cache_source, &cache_identity)).await?.ok_or_else(|| CatalogServiceError("EPUB publication is no longer active".to_owned()))?;
            if !publication_has_acquisition_url(&publication, &acquisition_url) {
                return Err(CatalogServiceError("EPUB acquisition is no longer active".to_owned()));
            }
            apply_catalog_metadata(&mut publication, extracted.metadata);
            if cover_is_definitively_absent {
                publication.image_url = None;
            }
            let toc = saved_metadata.as_ref().map(|metadata| metadata.toc.as_slice()).unwrap_or_default();
            let cache_source = source_id.clone();
            let cache_publication = publication.clone();
            let cache_subjects = subjects.clone();
            let cache_toc = toc.to_vec();
            let cache_popularity = popularity.clone();
            if !self.cache_write_call(move |cache| cache.enrich_active_publication(&cache_source, &cache_publication, &cache_subjects, &cache_toc, cache_popularity.as_ref())).await? {
                return Err(CatalogServiceError("EPUB publication is no longer active".to_owned()));
            }
            let cache_source = source_id.clone();
            let cache_url = acquisition_url.clone();
            self.cache_write_call(move |cache| cache.finish_epub_extraction(&cache_source, &cache_url, downloaded_bytes, &publication, saved_metadata.as_ref())).await
        }
        .await;
        if let Err(error) = &result {
            let cache_source = source_id.clone();
            let cache_url = acquisition_url.clone();
            let message = error.to_string();
            let (category, retryable) = classify_catalog_failure(&message);
            if let Err(state_error) = self.cache_write_call(move |cache| cache.fail_epub_extraction(&cache_source, &cache_url, &message, category, retryable)).await {
                tracing::warn!(source = source_id.as_str(), %state_error, "could not persist failed publication extraction");
            }
        }
        self.state.extraction_notify.notify_waiters();
        result
    }

    async fn download_catalog_metadata(&self, acquisition: &OpdsDirectAcquisition, include_cover: bool, prefer_range: bool) -> Result<opds::DownloadedOpdsBook, CatalogServiceError> {
        let _download_permit = self.state.background_download_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let operation = async {
            let range_origin = Url::parse(&acquisition.url).ok().map(|url| url.origin().ascii_serialization());
            let range_disabled = match range_origin.as_deref() {
                Some(origin) => self.state.epub_range_origins.lock().await.get(origin).is_some_and(|supported| !supported),
                None => false,
            };
            if prefer_range && acquisition.book_format == OpdsBookFormat::Epub && !range_disabled {
                match self.state.opds.download_epub_metadata_with_limit(acquisition, &self.state.acquisition_directory, metadata_publication_byte_limit(acquisition.book_format), include_cover).await {
                    Ok(downloaded) => {
                        if let Some(origin) = range_origin {
                            self.state.epub_range_origins.lock().await.insert(origin, true);
                        }
                        return Ok(downloaded);
                    }
                    Err(error) => {
                        if matches!(error, opds::OpdsError::RangeUnsupported) {
                            if let Some(origin) = range_origin {
                                self.state.epub_range_origins.lock().await.insert(origin, false);
                            }
                        }
                        tracing::debug!(url = acquisition.url, %error, "ranged EPUB metadata acquisition fell back to a full download");
                    }
                }
            }
            self.state.opds.download_book_with_limit(acquisition, &self.state.acquisition_directory, metadata_publication_byte_limit(acquisition.book_format)).await.map_err(|error| CatalogServiceError(error.to_string()))
        };
        tokio::time::timeout(CATALOG_METADATA_DOWNLOAD_TIMEOUT, operation).await.map_err(|_| CatalogServiceError(format!("publication download exceeded {} seconds", CATALOG_METADATA_DOWNLOAD_TIMEOUT.as_secs())))?
    }

    async fn enrich_saved_metadata_batch(&self, jobs: &[CatalogMetadataBackfillJob]) -> Result<Vec<CatalogMetadataBackfillUpdate>, CatalogServiceError> {
        let mut isbn_groups = jobs
            .iter()
            .map(|job| {
                job.metadata
                    .book
                    .publication
                    .identifiers
                    .iter()
                    .filter(|identifier| identifier.scheme() == &PublicationIdentifierScheme::Isbn)
                    .filter_map(PublicationIdentifier::canonical_value)
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let queries = jobs
            .iter()
            .enumerate()
            .filter(|(index, _)| isbn_groups[*index].is_empty())
            .filter_map(|(index, job)| {
                let metadata = &job.metadata.book;
                (!metadata.title.trim().is_empty()).then(|| EditionIdentityQuery {
                    query_id: index.to_string(),
                    title: metadata.title.clone(),
                    authors: metadata.authors().map(|author| author.name().to_owned()).collect(),
                    publishers: metadata.publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect(),
                    publication_year: library_model::unique_publication_year(metadata.publication.dates.iter().map(|date| date.value())),
                })
            })
            .collect::<Vec<_>>();
        let requests = queries.chunks(METADATA_IDENTITY_BACKFILL_BATCH_SIZE).map(|chunk| EditionIdentityRequest { queries: chunk.to_vec() }).collect::<Vec<_>>();
        let responses = stream::iter(requests.into_iter().map(|request| async move { self.resolve_edition_identities(request).await })).buffer_unordered(METADATA_IDENTITY_BACKFILL_CONCURRENCY).collect::<Vec<_>>().await;
        for response in responses {
            let response = response?;
            for result in response.results {
                if result.status != EditionIdentityStatus::Matched {
                    continue;
                }
                let Some(matched) = result.matched else { continue };
                if let Ok(index) = result.query_id.parse::<usize>() {
                    if let Some(isbns) = isbn_groups.get_mut(index) {
                        isbns.push(matched.canonical_isbn13);
                    }
                }
            }
        }

        let all_isbns = isbn_groups.iter().flatten().cloned().collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
        let (snapshot_date, matches_by_isbn) = self.metadata_matches_by_isbn(&all_isbns).await?;
        let mut updates = Vec::with_capacity(jobs.len());
        for (job, isbns) in jobs.iter().zip(isbn_groups) {
            let records = isbns.iter().filter_map(|isbn| matches_by_isbn.get(isbn)).flatten().collect::<Vec<_>>();
            let matched = catalog_metadata_match(records, &snapshot_date);
            let mut subjects = job.metadata.book.publication.subjects.clone();
            subjects.extend(matched.subjects);
            subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
            subjects.dedup();
            updates.push(CatalogMetadataBackfillUpdate { source_id: job.source_id.clone(), publication: job.publication.clone(), acquisition_url: job.acquisition_url.clone(), subjects, popularity: matched.popularity });
        }
        Ok(updates)
    }

    async fn metadata_matches_by_isbn(&self, isbns: &[String]) -> Result<(String, HashMap<String, Vec<RichWorkMetadataMatch>>), CatalogServiceError> {
        if isbns.is_empty() {
            return Ok(("unknown".to_owned(), HashMap::new()));
        }
        let Some(endpoint) = &self.state.metadata_endpoint else { return Ok(("unknown".to_owned(), HashMap::new())) };
        let mut by_isbn = HashMap::<String, Vec<RichWorkMetadataMatch>>::new();
        let mut snapshot_date = None;
        for batch in isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let mut endpoint = endpoint.clone();
            endpoint.set_path("/v2/classifications");
            let body = metadata_contract::encode_v2_request(&MetadataEnrichmentRequest { isbns: batch.to_vec() }).map_err(|error| CatalogServiceError(error.to_string()))?;
            let response = self
                .state
                .metadata_client
                .post(endpoint)
                .header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)
                .header(reqwest::header::ACCEPT, metadata_contract::MEDIA_TYPE_V2)
                .body(body)
                .send()
                .await
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            if !response.status().is_success() {
                return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
            }
            let bytes = response.bytes().await.map_err(|error| CatalogServiceError(error.to_string()))?;
            if bytes.len() > metadata_contract::MAX_RESPONSE_BYTES {
                return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
            }
            let response = metadata_contract::decode_v2_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))?;
            if snapshot_date.as_ref().is_some_and(|date| date != &response.snapshot.dump_date) {
                return Err(CatalogServiceError("metadata service changed snapshots during one catalogue enrichment request".to_owned()));
            }
            snapshot_date = Some(response.snapshot.dump_date);
            for result in response.results.into_iter().filter(|result| matches!(result.status, LookupStatus::Matched | LookupStatus::Ambiguous) && !result.matches.is_empty()) {
                let matches = result.matches;
                by_isbn.entry(result.requested_isbn).or_default().extend(matches.iter().cloned());
                if let Some(canonical) = result.canonical_isbn13 {
                    by_isbn.entry(canonical).or_default().extend(matches);
                }
            }
        }
        Ok((snapshot_date.unwrap_or_else(|| "unknown".to_owned()), by_isbn))
    }

    async fn infer_catalog_epub_isbn(&self, metadata: &mut CatalogEpubMetadata) -> Result<Option<String>, CatalogServiceError> {
        let authors = metadata.book.authors().map(|author| author.name().to_owned()).collect::<Vec<_>>();
        let publishers = metadata.book.publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect::<Vec<_>>();
        let publication_year = library_model::unique_publication_year(metadata.book.publication.dates.iter().map(|date| date.value()));
        if metadata.book.title.trim().is_empty() {
            return Ok(None);
        }
        let response =
            self.resolve_edition_identities(EditionIdentityRequest { queries: vec![EditionIdentityQuery { query_id: "catalogue-epub".to_owned(), title: metadata.book.title.clone(), authors, publishers, publication_year }] }).await?;
        let Some(result) = response.results.into_iter().find(|result| result.query_id == "catalogue-epub") else { return Ok(None) };
        if result.status != EditionIdentityStatus::Matched {
            return Ok(None);
        }
        let Some(matched) = result.matched else { return Ok(None) };
        let provenance = PublicationIdentifierType::new("openlibrary-edition-match", Some("bokheim:provenance".to_owned())).map_err(|error| CatalogServiceError(error.to_string()))?;
        let identifier = PublicationIdentifier::new(&matched.canonical_isbn13, PublicationIdentifierScheme::Isbn, vec![provenance]).map_err(|error| CatalogServiceError(error.to_string()))?;
        metadata.book.publication.identifiers.push(identifier);
        Ok(Some(matched.canonical_isbn13))
    }

    async fn metadata_matches(&self, isbns: &[String]) -> Result<HashMap<String, CatalogMetadataMatch>, CatalogServiceError> {
        let Some(endpoint) = &self.state.metadata_endpoint else { return Ok(HashMap::new()) };
        let mut matched_records = Vec::new();
        let mut enrichment_keys = isbns.iter().cloned().collect::<HashSet<_>>();
        let mut snapshot_date = None;
        for batch in isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let mut endpoint = endpoint.clone();
            endpoint.set_path("/v2/classifications");
            let body = metadata_contract::encode_v2_request(&MetadataEnrichmentRequest { isbns: batch.to_vec() }).map_err(|error| CatalogServiceError(error.to_string()))?;
            let mut response = self
                .state
                .metadata_client
                .post(endpoint.clone())
                .header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)
                .header(reqwest::header::ACCEPT, metadata_contract::MEDIA_TYPE_V2)
                .body(body)
                .send()
                .await
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            if !response.status().is_success() {
                return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
            }
            if response.content_length().is_some_and(|length| length > metadata_contract::MAX_RESPONSE_BYTES as u64) {
                return Err(CatalogServiceError("metadata service response exceeded its declared size limit".to_owned()));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| CatalogServiceError(error.to_string()))? {
                if bytes.len().saturating_add(chunk.len()) > metadata_contract::MAX_RESPONSE_BYTES {
                    return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
                }
                bytes.extend_from_slice(&chunk);
            }
            let response = metadata_contract::decode_v2_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))?;
            if snapshot_date.as_ref().is_some_and(|date| date != &response.snapshot.dump_date) {
                return Err(CatalogServiceError("metadata service changed snapshots during one catalogue enrichment request".to_owned()));
            }
            snapshot_date = Some(response.snapshot.dump_date);
            for result in response.results.into_iter().filter(|result| matches!(result.status, LookupStatus::Matched | LookupStatus::Ambiguous) && !result.matches.is_empty()) {
                enrichment_keys.insert(result.requested_isbn);
                if let Some(canonical) = result.canonical_isbn13 {
                    enrichment_keys.insert(canonical);
                }
                matched_records.extend(result.matches);
            }
        }
        if matched_records.is_empty() {
            return Ok(HashMap::new());
        }
        let matched = catalog_metadata_match(matched_records.iter().collect(), &snapshot_date.unwrap_or_else(|| "unknown".to_owned()));
        Ok(enrichment_keys.into_iter().map(|isbn| (isbn, matched.clone())).collect())
    }

    pub async fn rich_metadata_enrichment(&self, request: MetadataEnrichmentRequest) -> Result<metadata_contract::RichMetadataEnrichmentResponse, CatalogServiceError> {
        let Some(metadata_endpoint) = &self.state.metadata_endpoint else { return Err(CatalogServiceError("Open Library metadata enrichment is not configured".to_owned())) };
        let mut endpoint = metadata_endpoint.clone();
        endpoint.set_path("/v2/enrichment");
        let body = metadata_contract::encode_v2_request(&request).map_err(|error| CatalogServiceError(error.to_string()))?;
        let mut response = self
            .state
            .metadata_client
            .post(endpoint)
            .header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)
            .header(reqwest::header::ACCEPT, metadata_contract::MEDIA_TYPE_V2)
            .body(body)
            .send()
            .await
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > metadata_contract::MAX_RESPONSE_BYTES as u64) {
            return Err(CatalogServiceError("metadata service response exceeded its declared size limit".to_owned()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| CatalogServiceError(error.to_string()))? {
            if bytes.len().saturating_add(chunk.len()) > metadata_contract::MAX_RESPONSE_BYTES {
                return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
            }
            bytes.extend_from_slice(&chunk);
        }
        metadata_contract::decode_v2_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))
    }

    pub async fn resolve_edition_identities(&self, request: EditionIdentityRequest) -> Result<EditionIdentityResponse, CatalogServiceError> {
        let Some(metadata_endpoint) = &self.state.metadata_endpoint else { return Err(CatalogServiceError("Open Library metadata enrichment is not configured".to_owned())) };
        let mut endpoint = metadata_endpoint.clone();
        endpoint.set_path("/v2/edition-identities");
        let body = metadata_contract::encode_v2_edition_identity_request(&request).map_err(|error| CatalogServiceError(error.to_string()))?;
        let response = self.state.metadata_client.post(endpoint).header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2).body(body).send().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > metadata_contract::MAX_RESPONSE_BYTES as u64) {
            return Err(CatalogServiceError("metadata service response exceeded its declared size limit".to_owned()));
        }
        let bytes = response.bytes().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        if bytes.len() > metadata_contract::MAX_RESPONSE_BYTES {
            return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
        }
        metadata_contract::decode_v2_edition_identity_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogServiceError> {
        self.public_endpoint(source_id).await?;
        let cache_source = source_id.clone();
        let cache_image = image_url.to_owned();
        let active = self.cache_call(move |cache| cache.active_publication_for_cover(&cache_source, &cache_image)).await?;
        let recent = self.state.recent_browser_covers.lock().await.get(source_id, image_url);
        if active.is_none() && recent.is_none() {
            return Err(CatalogServiceError("cover URL is not present in the active catalog generation".to_owned()));
        }
        self.state.covers.remember_cover_url(source_id, image_url);
        let publication_identity = active.as_ref().map(|publication| publication.identity.clone()).or_else(|| recent.as_ref().map(|reference| reference.identity.clone()));
        let epub_acquisition = active.as_ref().and_then(publication_metadata_acquisition).or_else(|| recent.as_ref().and_then(|reference| reference.acquisition.clone()));
        let browser_known = recent.is_some();
        if let Some(acquisition) = epub_acquisition {
            let mut upstream_cover_is_unsuitable = false;
            // A real OPDS URL may already have produced a sufficiently large,
            // cover-shaped cached thumbnail. Synthetic keys always represent
            // an EPUB fallback and therefore skip this probe.
            if !is_synthetic_cover_key(image_url) {
                match self.state.covers.cached_cover(image_url).await {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) if error.is_unsuitable_cover_dimensions() => upstream_cover_is_unsuitable = true,
                    Err(_) if browser_known => match self.state.covers.cover(source_id, image_url).await {
                        Ok(bytes) => return Ok(bytes),
                        Err(error) if error.is_unsuitable_cover_dimensions() => upstream_cover_is_unsuitable = true,
                        Err(_) => {}
                    },
                    Err(_) => {}
                }
            }
            // Promote a queued metadata extraction when a reader needs the
            // EPUB fallback. Whichever task claims the extraction downloads
            // the EPUB once; the other waits for the generated JPEG instead of
            // issuing a competing acquisition request.
            if let Some(identity) = publication_identity.as_ref() {
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                self.cache_write_call(move |cache| cache.queue_epub_extractions(&cache_source, std::slice::from_ref(&cache_url), false)).await?;
                if let Err(error) = self.enrich_active_publication(source_id, identity.clone(), acquisition.clone(), Some(image_url.to_owned())).await {
                    tracing::warn!(source = source_id.as_str(), %error, "on-demand EPUB cover enrichment failed");
                }
                let deadline = tokio::time::Instant::now() + BROWSER_UPSTREAM_TIMEOUT;
                loop {
                    let notified = self.state.extraction_notify.notified();
                    if let Ok(bytes) = self.state.covers.cached_cover(image_url).await {
                        return Ok(bytes);
                    }
                    let cache_source = source_id.clone();
                    let cache_url = acquisition.url.clone();
                    let status = self.cache_call(move |cache| cache.epub_extraction_status(&cache_source, &cache_url)).await?;
                    let now = tokio::time::Instant::now();
                    if !matches!(status.as_deref(), Some("pending" | "running")) || now >= deadline {
                        break;
                    }
                    let _ = tokio::time::timeout(deadline - now, notified).await;
                }
            }
            let _download_permit = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.interactive_download_slots.acquire())
                .await
                .map_err(|_| CatalogServiceError("interactive catalog download capacity timed out".to_owned()))?
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            let downloaded = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.opds.download_book(&acquisition, &self.state.acquisition_directory))
                .await
                .map_err(|_| CatalogServiceError("interactive cover download timed out".to_owned()))?
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            drop(_download_permit);
            let _extraction_permit = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.extraction_slots.acquire())
                .await
                .map_err(|_| CatalogServiceError("interactive cover extraction capacity timed out".to_owned()))?
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            let path = downloaded.path().to_owned();
            let thumbnail = tokio::task::spawn_blocking(move || generate_publication_thumbnail(&path)).await.map_err(|error| CatalogServiceError(error.to_string()))?.map_err(CatalogServiceError)?;
            drop(downloaded);
            if let Some(thumbnail) = thumbnail {
                return self.state.covers.cover_with(source_id, image_url, async move { Ok(thumbnail) }).await.map_err(|error| CatalogServiceError(error.to_string()));
            }
            if let (true, Some(cache_identity)) = (is_synthetic_cover_key(image_url) || upstream_cover_is_unsuitable, publication_identity.clone()) {
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                let cache_image = image_url.to_owned();
                self.cache_write_call(move |cache| cache.clear_active_publication_cover(&cache_source, &cache_identity, &cache_url, &cache_image)).await?;
            }
            return Err(CatalogServiceError("EPUB and catalogue do not contain a suitable cover image".to_owned()));
        }
        if browser_known {
            self.state.covers.cover(source_id, image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
        } else {
            self.state.covers.cached_cover(image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
        }
    }

    pub async fn recommendations(&self, request: &CatalogRecommendationRequest) -> Result<CatalogRecommendationResponse, CatalogServiceError> {
        let request = request.clone();
        self.cache_call(move |cache| cache.recommendations(&request)).await
    }

    pub async fn browse_subjects(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogServiceError> {
        let request = request.clone();
        self.cache_call(move |cache| cache.browse_subjects(&request)).await
    }

    pub async fn browser_sources(&self) -> Vec<CatalogSource> {
        self.state.endpoints.read().await.iter().map(opds_source).collect()
    }

    pub async fn inventory(&self) -> Result<CatalogInventory, CatalogServiceError> {
        self.cache_call(CatalogCache::inventory).await
    }

    /// Downloads only an acquisition that is present on a freshly fetched
    /// page from the registered catalogue origin. This deliberately accepts
    /// structured identity data rather than an arbitrary proxy URL.
    pub async fn browser_acquisition(&self, source_id: &CatalogSourceId, request: &CatalogImportRequest) -> Result<DownloadedOpdsBook, CatalogServiceError> {
        let _permit = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.interactive_download_slots.acquire())
            .await
            .map_err(|_| CatalogServiceError("interactive catalog acquisition capacity timed out".to_owned()))?
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        if &request.source_id != source_id {
            return Err(CatalogServiceError("catalog acquisition source does not match the request path".to_owned()));
        }
        let endpoint = self.public_endpoint(source_id).await?;
        validate_opds_catalog_locator(&endpoint, &request.catalog_locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        let catalog = self.state.opds.fetch_catalog_fresh(&request.catalog_locator).await.map_err(|error| CatalogServiceError(error.to_string()))?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let (identity, acquisition) = validated_opds_import(request, &catalog).map_err(|error| CatalogServiceError(error.to_string()))?;
        identity.validate().map_err(|error| CatalogServiceError(error.to_string()))?;
        tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.opds.download_book(&acquisition, &self.state.acquisition_directory))
            .await
            .map_err(|_| CatalogServiceError("interactive catalog acquisition timed out".to_owned()))?
            .map_err(|error| CatalogServiceError(error.to_string()))
    }

    async fn public_endpoint(&self, source_id: &CatalogSourceId) -> Result<OpdsEndpoint, CatalogServiceError> {
        self.state.endpoints.read().await.iter().find(|endpoint| opds_catalog_source_id(&endpoint.id) == *source_id).cloned().ok_or_else(|| CatalogServiceError("catalog source is not registered".to_owned()))
    }

    async fn reconcile_sources(&self, endpoints: &[OpdsEndpoint]) -> Result<(), CatalogServiceError> {
        let endpoints = endpoints.to_vec();
        self.blocking_call(move |cache| reconcile_cached_sources(&cache, &endpoints).map(|_| ())).await
    }

    fn from_bytes(source: RegistrySource, bytes: Vec<u8>, cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        let registry = parse_registry(bytes.clone())?;
        let endpoints = endpoints_from_registry(registry.clone()).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_path = cache_path.unwrap_or_else(default_cache_path);
        let cache = CatalogCache::open(cache_path).map_err(cache_error)?;
        reconcile_cached_sources(&cache, &endpoints)?;
        let covers = CatalogCoverCache::new_public_only(cover_cache_directory).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cover_pipeline = CatalogCoverPipeline::new(covers.clone(), COVER_DOWNLOAD_WORKERS, COVER_THUMBNAIL_WORKERS).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(Self {
            state: Arc::new(CatalogServiceState {
                started_at: Instant::now(),
                source,
                snapshot: RwLock::new(RegistrySnapshot { observed_bytes: bytes, registry }),
                endpoints: RwLock::new(endpoints),
                opds: OpdsClient::new_public_only().map_err(|error| CatalogServiceError(error.to_string()))?,
                cache,
                covers,
                cover_pipeline,
                cache_write_lock: Mutex::new(()),
                crawl_locks: Mutex::new(HashMap::new()),
                crawl_progress: Mutex::new(HashMap::new()),
                source_update_locks: Mutex::new(HashMap::new()),
                recent_browser_covers: Mutex::new(RecentBrowserCovers::default()),
                epub_range_origins: Mutex::new(HashMap::new()),
                background_download_slots: Semaphore::new(MAX_CONCURRENT_DOWNLOADS),
                interactive_download_slots: Semaphore::new(MAX_INTERACTIVE_DOWNLOADS),
                queued_download_slots: Semaphore::new(MAX_QUEUED_DOWNLOADS),
                extraction_slots: Semaphore::new(MAX_CONCURRENT_EXTRACTIONS),
                extraction_notify: Notify::new(),
                acquisition_directory: std::env::temp_dir().join("bokheim-catalog-acquisitions"),
                metadata_endpoint: configured_metadata_endpoint()?,
                metadata_client: reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(15)).build().map_err(|error| CatalogServiceError(error.to_string()))?,
                max_crawl_pages: configured_max_crawl_pages()?,
            }),
        })
    }
}

fn classify_catalog_failure(message: &str) -> (&'static str, bool) {
    let message = message.to_ascii_lowercase();
    if message.contains("timed out") || message.contains("timeout") || message.contains("download exceeded") {
        ("network_timeout", true)
    } else if message.contains("dns") || message.contains("resolve") {
        ("network_dns", true)
    } else if message.contains("tls") || message.contains("certificate") {
        ("network_tls", true)
    } else if message.contains("429") || message.contains("too many requests") {
        ("http_rate_limited", true)
    } else if message.contains("404") || message.contains("410") {
        ("http_unavailable", false)
    } else if message.contains("401") || message.contains("403") || message.contains("forbidden") || message.contains("unauthorized") {
        ("http_access_denied", false)
    } else if message.contains("http") || message.contains("request failed") || message.contains("connection") {
        ("network_or_http", true)
    } else if message.contains("too large") || message.contains("size limit") || message.contains("exceeded") {
        ("safety_limit", false)
    } else if message.contains("unsupported") {
        ("unsupported_format", false)
    } else if message.contains("no longer active") {
        ("stale_publication", false)
    } else if message.contains("zip") || message.contains("epub") || message.contains("container") || message.contains("truncated") {
        ("invalid_publication", false)
    } else if message.contains("metadata") || message.contains("opf") || message.contains("xml") {
        ("metadata_parse", false)
    } else {
        ("internal", true)
    }
}

fn prioritize_startup_crawls(endpoints: &mut [OpdsEndpoint]) {
    endpoints.sort_by_key(|endpoint| endpoint.id.as_str() != PRIORITY_CRAWL_SOURCE);
}

fn reconcile_cached_sources(cache: &CatalogCache, endpoints: &[OpdsEndpoint]) -> Result<Vec<CatalogSourceId>, CatalogServiceError> {
    let retained = endpoints.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<HashSet<_>>();
    for endpoint in endpoints {
        cache.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).map_err(cache_error)?;
    }
    let mut removed = Vec::new();
    for source in cache.inventory().map_err(cache_error)?.sources {
        if !retained.contains(&source.source_id) {
            cache.remove_source(&source.source_id).map_err(cache_error)?;
            removed.push(source.source_id);
        }
    }
    Ok(removed)
}

fn publication_cover_fallback(source_id: &CatalogSourceId, publication: &mut CatalogPublication) -> Option<(String, OpdsDirectAcquisition)> {
    let request = publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if extraction_book_format(request.book_format).is_some() => Some(request),
        _ => None,
    })?;
    let book_format = extraction_book_format(request.book_format)?;
    let digest = blake3::hash(format!("{}\n{}", source_id.as_str(), request.download_url).as_bytes()).to_hex();
    let key = publication.image_url.clone().unwrap_or_else(|| format!("{EPUB_COVER_KEY_PREFIX}{digest}"));
    publication.image_url = Some(key.clone());
    Some((key, OpdsDirectAcquisition { url: request.download_url.clone(), kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None }))
}

fn publication_metadata_acquisition(publication: &CatalogPublication) -> Option<OpdsDirectAcquisition> {
    let request = publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if extraction_book_format(request.book_format).is_some() => Some(request),
        _ => None,
    })?;
    let book_format = extraction_book_format(request.book_format)?;
    Some(OpdsDirectAcquisition {
        url: request.download_url.clone(),
        kind: OpdsAcquisitionKind::OpenAccess,
        book_format,
        suggested_file_name: format!("{}.{}", blake3::hash(request.download_url.as_bytes()).to_hex(), book_format.extension()),
        price: None,
    })
}

fn extraction_book_format(format: CatalogBookFormat) -> Option<OpdsBookFormat> {
    match format {
        CatalogBookFormat::Epub => Some(OpdsBookFormat::Epub),
        CatalogBookFormat::Pdf => Some(OpdsBookFormat::Pdf),
        CatalogBookFormat::Mobi => Some(OpdsBookFormat::Mobi),
        _ => None,
    }
}

fn metadata_publication_byte_limit(format: OpdsBookFormat) -> u64 {
    match format {
        OpdsBookFormat::Epub => MAX_EPUB_METADATA_PUBLICATION_BYTES,
        OpdsBookFormat::Mobi => MAX_MOBI_METADATA_PUBLICATION_BYTES,
        OpdsBookFormat::Pdf => MAX_PDF_METADATA_PUBLICATION_BYTES,
        _ => 0,
    }
}

fn is_synthetic_cover_key(value: &str) -> bool {
    value.starts_with(EPUB_COVER_KEY_PREFIX)
}

fn generate_publication_thumbnail(path: &Path) -> Result<Option<Vec<u8>>, String> {
    thumbnail::generate_thumbnail(path).map_err(|error| error.to_string())
}

#[derive(Deserialize, Serialize)]
struct ExtractedCatalogData {
    metadata: ExtractedCatalogMetadata,
    isbns: Vec<String>,
    subjects: Vec<PublicationSubject>,
    saved_metadata: Option<CatalogEpubMetadata>,
    thumbnail: Option<Vec<u8>>,
}

#[derive(Default, Deserialize, Serialize)]
struct ExtractedCatalogMetadata {
    title: Option<String>,
    authors: Vec<CatalogAuthorCredit>,
    languages: Vec<String>,
    description: Option<String>,
}

fn extract_catalog_metadata(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    match book_format {
        OpdsBookFormat::Epub => extract_epub_catalog_metadata(path, extract_cover),
        OpdsBookFormat::Mobi => extract_mobi_catalog_metadata(path, extract_cover),
        OpdsBookFormat::Pdf => extract_pdf_catalog_metadata(path, extract_cover),
        _ => Err(format!("catalog metadata extraction is unsupported for {}", book_format.extension())),
    }
}

#[cfg(test)]
async fn extract_catalog_metadata_isolated(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, CatalogServiceError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || extract_catalog_metadata(&path, book_format, extract_cover)).await.map_err(|error| CatalogServiceError(error.to_string()))?.map_err(CatalogServiceError)
}

#[cfg(not(test))]
async fn extract_catalog_metadata_isolated(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, CatalogServiceError> {
    let executable = std::env::current_exe().map_err(|error| CatalogServiceError(error.to_string()))?;
    let output = tempfile::NamedTempFile::new().map_err(|error| CatalogServiceError(error.to_string()))?;
    let output_path = output.path().to_owned();
    let mut command = tokio::process::Command::new("/usr/bin/prlimit");
    command
        .arg(format!("--as={EXTRACTION_WORKER_MEMORY_BYTES}"))
        .arg(format!("--cpu={EXTRACTION_WORKER_CPU_SECONDS}"))
        .arg("--")
        .arg(executable)
        .arg("--catalog-extraction-worker")
        .arg(path)
        .arg(book_format.extension())
        .arg(if extract_cover { "cover" } else { "metadata" })
        .arg(&output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(EXTRACTION_WORKER_TIMEOUT, command.status())
        .await
        .map_err(|_| CatalogServiceError("publication extraction worker exceeded its time limit".to_owned()))?
        .map_err(|error| CatalogServiceError(format!("could not start publication extraction worker: {error}")))?;
    if !status.success() {
        return Err(CatalogServiceError(format!("publication extraction worker exceeded a resource limit or failed with {status}")));
    }
    let length = tokio::fs::metadata(&output_path).await.map_err(|error| CatalogServiceError(error.to_string()))?.len();
    if length > MAX_EXTRACTION_RESULT_BYTES as u64 {
        return Err(CatalogServiceError(format!("publication extraction result exceeded the {MAX_EXTRACTION_RESULT_BYTES}-byte size limit")));
    }
    let bytes = tokio::fs::read(&output_path).await.map_err(|error| CatalogServiceError(error.to_string()))?;
    wire::decode(&bytes, MAX_EXTRACTION_RESULT_BYTES).map_err(|error| CatalogServiceError(error.to_string()))
}

/// Internal subprocess entry point. Expensive and native format parsers run
/// here under an address-space and CPU limit so one malformed publication can
/// never make the catalogue HTTP process unavailable.
pub fn run_catalog_extraction_worker(input: &Path, format: &str, mode: &str, output: &Path) -> Result<(), String> {
    let book_format = match format {
        "epub" => OpdsBookFormat::Epub,
        "mobi" => OpdsBookFormat::Mobi,
        "pdf" => OpdsBookFormat::Pdf,
        _ => return Err(format!("unsupported extraction worker format: {format}")),
    };
    let extract_cover = match mode {
        "cover" => true,
        "metadata" => false,
        _ => return Err(format!("unsupported extraction worker mode: {mode}")),
    };
    let input_length = std::fs::metadata(input).map_err(|error| error.to_string())?.len();
    let input_limit = metadata_publication_byte_limit(book_format);
    if input_length > input_limit {
        return Err(format!("publication exceeds the {input_limit}-byte extraction size limit"));
    }
    let extracted = extract_catalog_metadata(input, book_format, extract_cover)?;
    let bytes = wire::encode(&extracted).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_EXTRACTION_RESULT_BYTES {
        return Err(format!("publication extraction result exceeded the {MAX_EXTRACTION_RESULT_BYTES}-byte size limit"));
    }
    std::fs::write(output, bytes).map_err(|error| error.to_string())
}

fn extract_epub_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let inspected = book_metadata::inspect_epub(path).map_err(|error| error.to_string())?;
    let book = inspected.metadata;
    let title = bounded_embedded_text(&book.title, MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors = book.authors().map(|author| CatalogAuthorCredit { name: author.name().to_owned(), sort_name: author.sort_name().map(str::to_owned), identifiers: Vec::new(), provider_link: None }).collect();
    let languages = book.publication.languages.iter().map(|language| language.as_str().to_owned()).collect();
    let description = bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true);
    let isbns = book.publication.identifiers.iter().filter(|identifier| identifier.scheme() == &PublicationIdentifierScheme::Isbn).filter_map(|identifier| identifier.canonical_value()).collect::<HashSet<_>>().into_iter().collect();
    let subjects = book.publication.subjects.clone();
    let metadata = ExtractedCatalogMetadata { title, authors, languages, description };
    let saved_metadata = Some(CatalogEpubMetadata { book, toc: inspected.toc });
    // Cover corruption must not discard otherwise valid bibliographic data.
    let thumbnail = if extract_cover { generate_publication_thumbnail(path).ok().flatten() } else { None };
    Ok(ExtractedCatalogData { metadata, isbns, subjects, saved_metadata, thumbnail })
}

fn extract_mobi_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let mobi = mobi::MobiMetadata::from_path(path).map_err(|error| error.to_string())?;
    let (metadata, isbns, subjects) = mobi_catalog_metadata(&mobi);
    let thumbnail = if extract_cover { generate_publication_thumbnail(path).ok().flatten() } else { None };
    Ok(ExtractedCatalogData { metadata, isbns, subjects, saved_metadata: None, thumbnail })
}

fn mobi_catalog_metadata(mobi: &mobi::MobiMetadata) -> (ExtractedCatalogMetadata, Vec<String>, Vec<PublicationSubject>) {
    let title = bounded_embedded_text(&mobi.title(), MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors =
        mobi.author().and_then(|author| bounded_embedded_text(&author, MAX_EMBEDDED_METADATA_TEXT_BYTES, false)).map(|name| CatalogAuthorCredit { name, sort_name: None, identifiers: Vec::new(), provider_link: None }).into_iter().collect();
    let description = mobi.description().and_then(|description| bounded_embedded_text(&description, MAX_EMBEDDED_DESCRIPTION_BYTES, true));
    let isbns = mobi.isbn().and_then(|isbn| canonical_isbn(&isbn)).into_iter().collect();
    let subjects = mobi
        .subjects()
        .unwrap_or_default()
        .into_iter()
        .take(256)
        .filter_map(|subject| {
            let subject = bounded_embedded_text(&subject, MAX_EMBEDDED_METADATA_TEXT_BYTES, false)?;
            PublicationSubject::new(None, subject, "mobi:subject", None, None).ok()
        })
        .collect();
    (ExtractedCatalogMetadata { title, authors, languages: Vec::new(), description }, isbns, subjects)
}

fn extract_pdf_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let book = book_metadata::inspect_pdf(path).map_err(|error| error.to_string())?.metadata;
    let title = bounded_embedded_text(&book.title, MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors = book.authors().map(|author| CatalogAuthorCredit { name: author.name().to_owned(), sort_name: author.sort_name().map(str::to_owned), identifiers: Vec::new(), provider_link: None }).collect();
    let languages = book.publication.languages.iter().map(|language| language.as_str().to_owned()).collect();
    let description = bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true);
    let isbns = book.publication.identifiers.iter().filter(|identifier| identifier.scheme() == &PublicationIdentifierScheme::Isbn).filter_map(PublicationIdentifier::canonical_value).collect::<HashSet<_>>().into_iter().collect();
    let subjects = book.publication.subjects.clone();
    // PDF rasterization is not metadata extraction. It has a much larger and
    // less predictable native working set, so covers are handled only by the
    // independently bounded cover pipeline.
    let _ = extract_cover;
    let thumbnail = None;
    let saved_metadata = Some(CatalogEpubMetadata { book, toc: Vec::new() });
    Ok(ExtractedCatalogData { metadata: ExtractedCatalogMetadata { title, authors, languages, description }, isbns, subjects, saved_metadata, thumbnail })
}

fn bounded_embedded_text(value: &str, max_bytes: usize, multiline: bool) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || value.chars().any(|character| character.is_control() && !(multiline && matches!(character, '\n' | '\r' | '\t'))) {
        return None;
    }
    Some(if multiline { value.to_owned() } else { value.split_whitespace().collect::<Vec<_>>().join(" ") })
}

fn publication_has_acquisition_url(publication: &CatalogPublication, acquisition_url: &str) -> bool {
    publication.actions.iter().any(|action| matches!(action, CatalogPublicationAction::Import { request } if extraction_book_format(request.book_format).is_some() && request.download_url == acquisition_url))
}

fn apply_catalog_metadata(publication: &mut CatalogPublication, metadata: ExtractedCatalogMetadata) {
    if let Some(title) = metadata.title {
        publication.title = title;
    }
    if !metadata.authors.is_empty() {
        publication.authors = metadata.authors;
    }
    if !metadata.languages.is_empty() {
        publication.languages = metadata.languages;
    }
    if publication.description.is_none() {
        if let Some(text) = metadata.description {
            publication.description = Some(CatalogDescription::PlainText { text });
        }
    }
}

fn merge_catalog_popularity(current: &mut Option<CatalogPublicationPopularity>, candidate: &CatalogPublicationPopularity) {
    let candidate_key = (candidate.reading_log_count(), candidate.rating_count);
    if current.as_ref().is_none_or(|current| candidate_key > (current.reading_log_count(), current.rating_count)) {
        *current = Some(candidate.clone());
    }
}

fn catalog_metadata_match(records: Vec<&RichWorkMetadataMatch>, snapshot_date: &str) -> CatalogMetadataMatch {
    // The metadata service already applies the library client's unified
    // classification-consensus policy before serializing every match. Treat
    // those classifications as authoritative here instead of constructing a
    // second, several-hundred-megabyte taxonomy runtime in the catalogue
    // process merely to filter the same evidence again.
    let merged = metadata_contract::merge_rich_work_matches(records, |_, _| Vec::new());
    let subjects = merged
        .classifications
        .iter()
        .filter_map(|classification| {
            let authority = match classification.scheme {
                ClassificationScheme::LibraryOfCongress => "lcc",
                ClassificationScheme::DeweyDecimal => "ddc",
            };
            let provenance = match classification.source {
                ClassificationSource::ExactEdition => "exact-edition",
                ClassificationSource::Work => "work",
            };
            let source = format!("openlibrary:{snapshot_date}:{provenance}");
            PublicationSubject::new(None, &classification.notation, source, Some(authority.to_owned()), Some(classification.notation.clone())).ok()
        })
        .collect();
    let popularity = merged.popularity.map(|popularity| CatalogPublicationPopularity {
        want_to_read_count: popularity.want_to_read_count,
        currently_reading_count: popularity.currently_reading_count,
        already_read_count: popularity.already_read_count,
        rating_count: popularity.rating_count,
        rating_sum: popularity.rating_sum,
    });
    CatalogMetadataMatch { subjects, popularity }
}

fn page_publication_count(page: &CatalogPage) -> u64 {
    page.publications.len().saturating_add(page.groups.iter().map(|group| group.publications.len()).sum::<usize>()) as u64
}

fn page_cover_urls(page: &CatalogPage) -> Vec<String> {
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter_map(|publication| publication.image_url.clone()).filter(|url| !is_synthetic_cover_key(url)).collect()
}

fn internal_page_links(root: &Url, page: &CatalogPage) -> Result<Vec<String>, CatalogServiceError> {
    let links = page.navigation.iter().chain(page.groups.iter().flat_map(|group| group.navigation.iter())).filter_map(|link| match &link.target {
        CatalogLinkTarget::Page { locator } => Some(locator.as_str()),
        CatalogLinkTarget::External { .. } => None,
    });
    let mut internal = Vec::new();
    for locator in links
        .chain(page.groups.iter().filter_map(|group| group.page_locator.as_deref()))
        .chain([page.pagination.first.as_deref(), page.pagination.previous.as_deref(), page.pagination.next.as_deref(), page.pagination.last.as_deref()].into_iter().flatten())
    {
        if same_origin(root, locator)? {
            internal.push(locator.to_owned());
        }
    }
    Ok(internal)
}

fn canonicalize_page_layout_locators(endpoint: &OpdsEndpoint, page: &mut catalog_contract::CatalogPageLayout) -> Result<(), CatalogServiceError> {
    page.locator = canonicalize_opds_catalog_locator(endpoint, &page.locator).map_err(|error| CatalogServiceError(error.to_string()))?;
    for link in page.navigation.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.navigation.iter_mut())).chain(page.facets.iter_mut().flat_map(|facet| facet.links.iter_mut())) {
        if let CatalogLinkTarget::Page { locator } = &mut link.target {
            *locator = canonicalize_opds_catalog_locator(endpoint, locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        }
    }
    for locator in page.groups.iter_mut().filter_map(|group| group.page_locator.as_mut()) {
        *locator = canonicalize_opds_catalog_locator(endpoint, locator).map_err(|error| CatalogServiceError(error.to_string()))?;
    }
    for locator in [&mut page.pagination.first, &mut page.pagination.previous, &mut page.pagination.next, &mut page.pagination.last].into_iter().flatten() {
        *locator = canonicalize_opds_catalog_locator(endpoint, locator).map_err(|error| CatalogServiceError(error.to_string()))?;
    }
    Ok(())
}

fn same_origin(root: &Url, candidate: &str) -> Result<bool, CatalogServiceError> {
    let candidate = Url::parse(candidate).map_err(|error| CatalogServiceError(error.to_string()))?;
    Ok(root.scheme() == candidate.scheme() && root.host_str() == candidate.host_str() && root.port_or_known_default() == candidate.port_or_known_default())
}

fn validate_query(query: &str) -> Result<(), CatalogServiceError> {
    if query.trim().is_empty() || query.len() > 512 || query.chars().any(char::is_control) {
        return Err(CatalogServiceError("catalog search query must be non-empty bounded text without control characters".to_owned()));
    }
    Ok(())
}

fn retain_delta_payloads<'a>(
    payloads: &mut Vec<CatalogPublicationPayload>, returned: impl IntoIterator<Item = &'a CatalogPublicationReference>, request_epoch: Option<&str>, response_epoch: &str, known_publications: Vec<CatalogPublicationReference>,
) {
    let returned = returned.into_iter().map(|publication| publication.identity.clone()).collect::<HashSet<_>>();
    let known = if request_epoch == Some(response_epoch) { known_publications.into_iter().map(|publication| (publication.identity, publication.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
    payloads.retain(|payload| returned.contains(&payload.publication.identity) && known.get(&payload.publication.identity) != Some(&payload.sequence));
}

fn cache_error(error: impl fmt::Display) -> CatalogServiceError {
    CatalogServiceError(error.to_string())
}

fn default_cache_path() -> PathBuf {
    std::env::temp_dir().join("bokheim-opds-cache.db")
}

fn default_cover_cache_directory() -> PathBuf {
    std::env::temp_dir().join(format!("bokheim-opds-covers-{}", std::process::id()))
}

impl fmt::Display for CatalogServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogServiceError {}

pub fn app(service: CatalogService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/opds/catalogs", get(catalogs))
        .route("/api/opds/sources/:source_id/page", post(catalog_page))
        .route("/api/opds/sources/:source_id/search", post(catalog_search))
        .route("/api/opds/recommendations", post(catalog_recommendations))
        .route("/api/opds/subjects", post(catalog_subjects))
        .route("/api/opds/sources/:source_id/cover", get(catalog_cover))
        .route("/api/web/catalogs", get(browser_catalogs))
        .route("/api/web/stats", get(browser_catalog_stats))
        .route("/api/metadata/enrichment", post(browser_metadata_enrichment))
        .route("/api/metadata/enrichment/v2", post(browser_metadata_enrichment_v2))
        .route("/api/web/sources/:source_id/page", post(browser_catalog_page))
        .route("/api/web/sources/:source_id/search", post(browser_catalog_search))
        .route("/api/web/recommendations", post(browser_catalog_recommendations))
        .route("/api/web/subjects", post(browser_catalog_subjects))
        .route("/api/web/sources/:source_id/acquire", post(browser_catalog_acquire))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST, Method::OPTIONS]).allow_headers([header::ACCEPT, header::CONTENT_TYPE, header::IF_NONE_MATCH]).expose_headers([
            header::CONTENT_DISPOSITION,
            header::CONTENT_LENGTH,
            header::CONTENT_TYPE,
            header::ETAG,
        ]))
        .with_state(service)
}

/// Loopback-only operational surface. The catalogue binary serves this on a
/// separate listener so public reverse proxies cannot accidentally expose it.
pub fn operations_app(service: CatalogService) -> Router {
    Router::new().route("/internal/operations", get(catalog_operations)).with_state(service)
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn catalogs(State(service): State<CatalogService>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let registry = service.registry().await;
    let etag = format!("\"opds-{}\"", registry.revision);
    if headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        add_common_headers(&mut response, REGISTRY_CACHE_CONTROL);
        response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
        return Ok(response);
    }
    let mut response = wire_response(&registry, REGISTRY_CACHE_CONTROL)?;
    response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
    Ok(response)
}

async fn catalog_page(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogPageRequest = decode_wire_request(&headers, &body)?;
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let response = service.page_response(&source_id, request).await.map_err(ApiError::unavailable)?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

async fn catalog_search(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogSearchRequest = decode_wire_request(&headers, &body)?;
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let response = service.search_response(&source_id, request).await.map_err(ApiError::unavailable)?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

async fn catalog_recommendations(State(service): State<CatalogService>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogRecommendationRequest = decode_wire_request(&headers, &body)?;
    let response = service.recommendations(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

async fn catalog_subjects(State(service): State<CatalogService>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogSubjectBrowseRequest = decode_wire_request(&headers, &body)?;
    let response = service.browse_subjects(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

async fn browser_catalogs(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    let _ = service.registry().await;
    json_response(&service.browser_sources().await, REGISTRY_CACHE_CONTROL)
}

async fn browser_catalog_stats(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    catalog_operations(State(service)).await
}

async fn browser_metadata_enrichment(State(service): State<CatalogService>, Json(request): Json<MetadataEnrichmentRequest>) -> Result<Response, ApiError> {
    let response = service.rich_metadata_enrichment(request).await.map_err(ApiError::unavailable)?;
    json_response(&legacy_metadata_enrichment(response), "no-store")
}

async fn browser_metadata_enrichment_v2(State(service): State<CatalogService>, Json(request): Json<MetadataEnrichmentRequest>) -> Result<Response, ApiError> {
    let response = service.rich_metadata_enrichment(request).await.map_err(ApiError::unavailable)?;
    json_response(&response, "no-store")
}

fn legacy_metadata_enrichment(response: metadata_contract::RichMetadataEnrichmentResponse) -> metadata_contract::MetadataEnrichmentResponse {
    metadata_contract::MetadataEnrichmentResponse {
        snapshot: response.snapshot,
        results: response
            .results
            .into_iter()
            .map(|result| metadata_contract::IsbnMetadataResult {
                requested_isbn: result.requested_isbn,
                canonical_isbn13: result.canonical_isbn13,
                status: result.status,
                matches: result
                    .matches
                    .into_iter()
                    .map(|matched| metadata_contract::WorkMetadataMatch {
                        open_library_work_id: matched.open_library_work_id,
                        exact_edition_ids: matched.exact_edition_ids,
                        classifications: matched.classifications,
                        authors: matched.authors,
                    })
                    .collect(),
            })
            .collect(),
    }
}

async fn catalog_operations(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    let inventory = catalog_inventory_response(&service).await?;
    let failures = service.cache_call(CatalogCache::epub_failure_diagnostics).await.map_err(ApiError::unavailable)?;
    let extraction_failures = failures
        .into_iter()
        .map(|failure| EpubFailureDiagnosticResponse {
            category: failure.category,
            retryable: failure.retryable,
            count: failure.count,
            first_failed_at: failure.first_failed_at,
            last_failed_at: failure.last_failed_at,
            next_retry_at: failure.next_retry_at,
            examples: failure
                .examples
                .into_iter()
                .map(|example| EpubFailureExampleResponse {
                    source_id: example.source_id.as_str().to_owned(),
                    acquisition_host: example.acquisition_host,
                    publication_id: example.publication_id,
                    attempts: example.attempts,
                    last_failed_at: example.last_failed_at,
                })
                .collect(),
        })
        .collect();
    json_response(&CatalogOperationsResponse { inventory, extraction_failures }, "no-store")
}

async fn catalog_inventory_response(service: &CatalogService) -> Result<CatalogInventoryResponse, ApiError> {
    let registry = service.registry().await;
    let uptime_seconds = service.state.started_at.elapsed().as_secs();
    let metadata_enrichment_configured = service.state.metadata_endpoint.is_some();
    let inventory = service.inventory().await.map_err(ApiError::unavailable)?;
    let crawl_progress = service.state.crawl_progress.lock().await.clone();
    let response = CatalogInventoryResponse {
        status: "available",
        uptime_seconds,
        registry_revision: registry.revision,
        metadata_enrichment_configured,
        active_page_count: inventory.active_page_count,
        active_publication_count: inventory.active_publication_count,
        active_epub_count: inventory.active_epub_count,
        epub_extractions: epub_extraction_inventory_response(inventory.epub_extractions),
        metadata_backfill: MetadataBackfillInventoryResponse {
            pending: inventory.metadata_backfill.pending,
            running: inventory.metadata_backfill.running,
            succeeded: inventory.metadata_backfill.succeeded,
            last_error: inventory.metadata_backfill.last_error,
        },
        sources: inventory
            .sources
            .into_iter()
            .map(|source| {
                let progress = (source.crawl_status == "running").then(|| crawl_progress.get(&source.source_id)).flatten();
                CatalogSourceInventoryResponse {
                    source_id: source.source_id.as_str().to_owned(),
                    crawl_status: source.crawl_status,
                    last_crawl_started_at: source.last_crawl_started_at,
                    last_crawl_succeeded_at: source.last_crawl_succeeded_at,
                    last_crawl_error_category: source.last_crawl_error.as_deref().map(|error| classify_catalog_failure(error).0),
                    crawl_frontier_count: source.crawl_frontier_count,
                    crawl_pages_fetched: progress.map_or(0, |progress| progress.pages_fetched),
                    crawl_publications_seen: progress.map_or(0, |progress| progress.publications_seen),
                    crawl_elapsed_seconds: progress.map_or(0, |progress| progress.started_at.elapsed().as_secs()),
                    active_page_count: source.active_page_count,
                    active_publication_count: source.active_publication_count,
                    active_epub_count: source.active_epub_count,
                    epub_extractions: epub_extraction_inventory_response(source.epub_extractions),
                }
            })
            .collect(),
    };
    Ok(response)
}

fn epub_extraction_inventory_response(inventory: catalog_core::CatalogEpubExtractionInventory) -> EpubExtractionInventoryResponse {
    EpubExtractionInventoryResponse {
        pending: inventory.pending,
        running: inventory.running,
        downloading: inventory.downloading,
        extracting: inventory.extracting,
        enriching: inventory.enriching,
        succeeded: inventory.succeeded,
        failed: inventory.failed,
        attempts: inventory.attempts,
        downloaded_bytes: inventory.downloaded_bytes,
    }
}

async fn browser_catalog_page(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Json(mut request): Json<CatalogPageRequest>) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    request.epoch = None;
    request.known_publications.clear();
    let page = service.browser_page(&source_id, request).await.map_err(ApiError::unavailable)?;
    json_response(&page, DATA_CACHE_CONTROL)
}

async fn browser_catalog_search(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Json(mut request): Json<CatalogSearchRequest>) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    request.epoch = None;
    request.known_publications.clear();
    let page = service.browser_search(&source_id, request).await.map_err(ApiError::unavailable)?;
    json_response(&page, DATA_CACHE_CONTROL)
}

async fn browser_catalog_recommendations(State(service): State<CatalogService>, Json(request): Json<CatalogRecommendationRequest>) -> Result<Response, ApiError> {
    let response = service.recommendations(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    json_response(&response, DATA_CACHE_CONTROL)
}

async fn browser_catalog_subjects(State(service): State<CatalogService>, Json(request): Json<CatalogSubjectBrowseRequest>) -> Result<Response, ApiError> {
    let response = service.browse_subjects(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    json_response(&response, DATA_CACHE_CONTROL)
}

async fn browser_catalog_acquire(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Json(request): Json<CatalogImportRequest>) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let downloaded = service.browser_acquisition(&source_id, &request).await.map_err(ApiError::unavailable)?;
    let length = tokio::fs::metadata(downloaded.path()).await.map_err(ApiError::internal)?.len();
    let file = tokio::fs::File::open(downloaded.path()).await.map_err(ApiError::internal)?;
    let stream = futures_util::stream::try_unfold((file, downloaded), |(mut file, guard)| async move {
        let mut buffer = vec![0_u8; 64 * 1024];
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            Ok::<_, std::io::Error>(None)
        } else {
            buffer.truncate(read);
            Ok(Some((Bytes::from(buffer), (file, guard))))
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(book_media_type(request.book_format)));
    response.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_str(&length.to_string()).map_err(ApiError::internal)?);
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(response)
}

fn filter_browser_page(mut page: CatalogPage, source: &CatalogSource, supported_languages: &[String]) -> CatalogPage {
    let filter = CatalogLanguageFilter::new(supported_languages);
    if filter.source_is_multilingual(source) {
        page.publications.retain(|publication| filter.publication_supported(publication));
        for group in &mut page.groups {
            group.publications.retain(|publication| filter.publication_supported(publication));
        }
    }
    page
}

fn book_media_type(format: CatalogBookFormat) -> &'static str {
    match format {
        CatalogBookFormat::Epub => "application/epub+zip",
        CatalogBookFormat::Pdf => "application/pdf",
        CatalogBookFormat::Mobi => "application/x-mobipocket-ebook",
        CatalogBookFormat::Fb2 => "application/fb2+zip",
        CatalogBookFormat::Cbz => "application/vnd.comicbook+zip",
        CatalogBookFormat::WebPub => "application/webpub+zip",
        CatalogBookFormat::M4b => "audio/mp4",
    }
}

async fn catalog_cover(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Query(query): Query<CoverQuery>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let bytes = service.catalog_cover(&source_id, &query.image_url).await.map_err(ApiError::unavailable)?;
    let etag = format!("\"{}\"", blake3::hash(&bytes).to_hex());
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(COVER_CACHE_CONTROL));
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
    if request_headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }
    Ok((headers, bytes).into_response())
}

fn require_wire_accept(headers: &HeaderMap) -> Result<(), ApiError> {
    if !headers.get_all(header::ACCEPT).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(transport_wire::is_current_media_type) {
        return Err(ApiError { status: StatusCode::NOT_ACCEPTABLE, message: "request requires the current Bokheim binary media type".to_owned() });
    }
    Ok(())
}

fn decode_wire_request<T: serde::de::DeserializeOwned>(headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
    let media_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
    if !media_type.is_some_and(transport_wire::is_current_media_type) {
        return Err(ApiError::bad_request("request requires the current Bokheim binary content type"));
    }
    if headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).is_some_and(|value| !value.eq_ignore_ascii_case("identity")) {
        return Err(ApiError::bad_request("compressed request bodies are not supported"));
    }
    transport_wire::decode(body, transport_wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| ApiError::bad_request(error.to_string()))
}

fn wire_response(value: &impl serde::Serialize, cache_control: &'static str) -> Result<Response, ApiError> {
    let bytes = transport_wire::encode(value).map_err(ApiError::internal)?;
    let mut response = bytes.into_response();
    add_common_headers(&mut response, cache_control);
    Ok(response)
}

fn json_response(value: &impl serde::Serialize, cache_control: &'static str) -> Result<Response, ApiError> {
    let mut response = Json(value).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(response)
}

fn add_common_headers(response: &mut Response, cache_control: &'static str) {
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(transport_wire::MEDIA_TYPE));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response.headers_mut().insert(header::VARY, HeaderValue::from_static("Accept"));
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: message.into() }
    }

    fn unavailable(error: CatalogServiceError) -> Self {
        Self { status: StatusCode::SERVICE_UNAVAILABLE, message: error.to_string() }
    }

    fn internal(error: impl fmt::Display) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: error.to_string() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")], self.message).into_response()
    }
}

async fn read_registry_file(path: &Path) -> Result<Vec<u8>, CatalogServiceError> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| CatalogServiceError(format!("could not inspect catalog registry {}: {error}", path.display())))?;
    if metadata.len() > MAX_REGISTRY_BYTES as u64 {
        return Err(CatalogServiceError(format!("catalog registry {} exceeds {MAX_REGISTRY_BYTES} bytes", path.display())));
    }
    tokio::fs::read(path).await.map_err(|error| CatalogServiceError(format!("could not read catalog registry {}: {error}", path.display())))
}

fn parse_registry(bytes: Vec<u8>) -> Result<CatalogRegistry, CatalogServiceError> {
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(CatalogServiceError(format!("catalog registry exceeds {MAX_REGISTRY_BYTES} bytes")));
    }
    let registry: CatalogRegistry = serde_json::from_slice(&bytes).map_err(|error| CatalogServiceError(format!("catalog registry is not valid JSON: {error}")))?;
    validate_curated_registry(&registry).map_err(|error| CatalogServiceError(error.to_string()))?;
    Ok(registry)
}

fn configured_metadata_endpoint() -> Result<Option<Url>, CatalogServiceError> {
    let Some(value) = std::env::var_os("BOKHEIM_METADATA_URL") else { return Ok(None) };
    let mut url = Url::parse(&value.to_string_lossy()).map_err(|error| CatalogServiceError(format!("invalid BOKHEIM_METADATA_URL: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.cannot_be_a_base() {
        return Err(CatalogServiceError("BOKHEIM_METADATA_URL must be an HTTP(S) base URL".to_owned()));
    }
    url.set_path("/v2/enrichment");
    url.set_query(None);
    url.set_fragment(None);
    Ok(Some(url))
}

fn configured_max_crawl_pages() -> Result<usize, CatalogServiceError> {
    let Some(value) = std::env::var_os("BOKHEIM_CATALOG_MAX_CRAWL_PAGES") else { return Ok(DEFAULT_MAX_CRAWL_PAGES) };
    let value = value.to_string_lossy();
    let pages = value.parse::<usize>().map_err(|_| CatalogServiceError("BOKHEIM_CATALOG_MAX_CRAWL_PAGES must be a positive integer".to_owned()))?;
    if pages == 0 || pages > HARD_MAX_CRAWL_PAGES {
        return Err(CatalogServiceError(format!("BOKHEIM_CATALOG_MAX_CRAWL_PAGES must be between 1 and {HARD_MAX_CRAWL_PAGES}")));
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::get;
    use image::{GenericImageView, ImageOutputFormat};
    use lopdf::dictionary;
    use opds::{registry_from_endpoints, OpdsEndpointId};
    use std::io::{Cursor, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tower::ServiceExt;
    use zip::write::SimpleFileOptions;

    fn endpoint(url: String) -> OpdsEndpoint {
        OpdsEndpoint { id: OpdsEndpointId::parse("test").unwrap(), title: "Test".to_owned(), description: "Test catalog".to_owned(), languages: vec!["Multilingual".to_owned()], url }
    }

    fn service(endpoint: OpdsEndpoint, directory: &Path) -> CatalogService {
        let registry = registry_from_endpoints(1, std::slice::from_ref(&endpoint));
        let cache = CatalogCache::open(directory.join("catalog-cache.db")).unwrap();
        cache.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).unwrap();
        let covers = CatalogCoverCache::new(directory.join("covers")).unwrap();
        let cover_pipeline = CatalogCoverPipeline::new(covers.clone(), 1, 1).unwrap();
        CatalogService {
            state: Arc::new(CatalogServiceState {
                source: RegistrySource::Bundled,
                snapshot: RwLock::new(RegistrySnapshot { observed_bytes: serde_json::to_vec(&registry).unwrap(), registry }),
                endpoints: RwLock::new(vec![endpoint]),
                opds: OpdsClient::new().unwrap(),
                cache,
                covers,
                cover_pipeline,
                cache_write_lock: Mutex::new(()),
                crawl_locks: Mutex::new(HashMap::new()),
                crawl_progress: Mutex::new(HashMap::new()),
                source_update_locks: Mutex::new(HashMap::new()),
                recent_browser_covers: Mutex::new(RecentBrowserCovers::default()),
                epub_range_origins: Mutex::new(HashMap::new()),
                background_download_slots: Semaphore::new(MAX_CONCURRENT_DOWNLOADS),
                interactive_download_slots: Semaphore::new(MAX_INTERACTIVE_DOWNLOADS),
                queued_download_slots: Semaphore::new(MAX_QUEUED_DOWNLOADS),
                extraction_slots: Semaphore::new(MAX_CONCURRENT_EXTRACTIONS),
                extraction_notify: Notify::new(),
                acquisition_directory: directory.join("acquisitions"),
                metadata_endpoint: None,
                metadata_client: reqwest::Client::new(),
                max_crawl_pages: DEFAULT_MAX_CRAWL_PAGES,
                started_at: Instant::now(),
            }),
        }
    }

    async fn wait_for_successful_extractions(service: &CatalogService, expected: u64) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if service.state.cache.inventory().unwrap().epub_extractions.succeeded >= expected {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background EPUB extraction did not finish");
    }

    #[tokio::test]
    async fn first_crawl_publishes_root_before_descendant_traversal_finishes() {
        let child_started = Arc::new(AtomicBool::new(false));
        let release_child = Arc::new(tokio::sync::Notify::new());
        let upstream =
            Router::new().route("/root", get(|| async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Root"},"navigation":[{"href":"/child","type":"application/opds+json","title":"Child"}]}"#) })).route(
                "/child",
                get({
                    let child_started = child_started.clone();
                    let release_child = release_child.clone();
                    move || {
                        let child_started = child_started.clone();
                        let release_child = release_child.clone();
                        async move {
                            child_started.store(true, Ordering::SeqCst);
                            release_child.notified().await;
                            ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Child"},"navigation":[{"title":"Website","href":"/about","type":"text/html"}]}"#)
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let crawl_service = service.clone();
        let crawl_source = source_id.clone();
        let crawl_task = tokio::spawn(async move { crawl_service.crawl_source(&crawl_source).await });

        tokio::time::timeout(Duration::from_secs(1), async {
            while !child_started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let root = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: None, known_publications: Vec::new() }).await.unwrap();
        assert_eq!(root.page.title, "Root");
        assert_eq!(service.state.cache.inventory().unwrap().sources[0].crawl_status, "running");

        release_child.notify_waiters();
        crawl_task.await.unwrap().unwrap();
        let inventory = service.state.cache.inventory().unwrap();
        assert_eq!(inventory.active_page_count, 2);
        assert_eq!(inventory.sources[0].crawl_status, "succeeded");
        upstream_task.abort();
    }

    fn epub_with_embedded_cover() -> Vec<u8> {
        epub_with_embedded_cover_dimensions(360, 540)
    }

    fn epub_with_embedded_cover_dimensions(width: u32, height: u32) -> Vec<u8> {
        let mut cover = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(width, height).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        archive.start_file("mimetype", options.compression_method(zip::CompressionMethod::Stored)).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", options).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("OEBPS/content.opf", options).unwrap();
        archive
            .write_all(br##"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Embedded Title</dc:title><dc:identifier id="isbn">9780821338278</dc:identifier><meta refines="#isbn" property="identifier-type">ISBN</meta><dc:creator file-as="Author, Ada">Ada Author</dc:creator><dc:publisher>Example Press</dc:publisher><dc:date>2024-05-06</dc:date><dc:language>sv</dc:language><dc:description>Embedded description</dc:description><dc:subject>History / United States / 20th Century</dc:subject></metadata><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"##)
            .unwrap();
        archive.start_file("OEBPS/cover.png", options).unwrap();
        archive.write_all(cover.get_ref()).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", options).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>book</body></html>"#).unwrap();
        archive.start_file("OEBPS/nav.xhtml", options).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav epub:type="toc" xmlns:epub="http://www.idpf.org/2007/ops"><ol><li><a href="chapter.xhtml">The Hidden Chapter</a></li></ol></nav></body></html>"#).unwrap();
        archive.finish().unwrap().into_inner()
    }

    fn pdf_with_description() -> Vec<u8> {
        let mut document = lopdf::Document::with_version("1.7");
        let info = document.add_object(lopdf::dictionary! {
            "Title" => lopdf::Object::string_literal("Embedded PDF Title"),
            "Author" => lopdf::Object::string_literal("Ada PDF Author"),
            "Subject" => lopdf::Object::string_literal("A bounded description stored in the PDF information dictionary."),
        });
        let pages = document.new_object_id();
        let content = document.add_object(lopdf::Stream::new(lopdf::dictionary! {}, Vec::new()));
        let page = document.add_object(lopdf::dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 400.into(), 600.into()],
        });
        document.objects.insert(
            pages,
            lopdf::Object::Dictionary(lopdf::dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let root = document.add_object(lopdf::dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", root);
        document.trailer.set("Info", info);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn extracts_subject_and_canonical_isbn_from_epub_once() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("book.epub");
        std::fs::write(&path, epub_with_embedded_cover()).unwrap();
        let extracted = extract_epub_catalog_metadata(&path, false).unwrap();
        assert_eq!(extracted.isbns, ["9780821338278"]);
        assert_eq!(extracted.subjects[0].name(), "History / United States / 20th Century");
        let saved = extracted.saved_metadata.unwrap();
        assert_eq!(saved.book.publication.publishers[0].name().as_str(), "Example Press");
        assert_eq!(saved.book.publication.dates[0].value(), "2024-05-06");
        assert_eq!(saved.toc[0].title, "The Hidden Chapter");
        assert!(extracted.thumbnail.is_none());
    }

    #[test]
    fn extraction_worker_serializes_a_bounded_result_for_the_parent() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("book.epub");
        let output = temporary.path().join("result.bin");
        std::fs::write(&input, epub_with_embedded_cover()).unwrap();

        run_catalog_extraction_worker(&input, "epub", "metadata", &output).unwrap();

        let bytes = std::fs::read(output).unwrap();
        assert!(bytes.len() <= MAX_EXTRACTION_RESULT_BYTES);
        let extracted: ExtractedCatalogData = wire::decode(&bytes, MAX_EXTRACTION_RESULT_BYTES).unwrap();
        assert_eq!(extracted.isbns, ["9780821338278"]);
        assert!(extracted.thumbnail.is_none());
    }

    #[test]
    fn extracts_pdf_description_without_rasterizing_a_cover() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("book.pdf");
        std::fs::write(&path, pdf_with_description()).unwrap();

        let extracted = extract_catalog_metadata(&path, OpdsBookFormat::Pdf, true).unwrap();

        assert_eq!(extracted.metadata.title.as_deref(), Some("Embedded PDF Title"));
        assert_eq!(extracted.metadata.authors[0].name, "Ada PDF Author");
        assert_eq!(extracted.metadata.description.as_deref(), Some("A bounded description stored in the PDF information dictionary."));
        assert!(extracted.thumbnail.is_none());
    }

    #[test]
    fn extraction_worker_rejects_oversized_input_before_parsing() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("oversized.pdf");
        let output = temporary.path().join("result.bin");
        let file = std::fs::File::create(&input).unwrap();
        file.set_len(MAX_PDF_METADATA_PUBLICATION_BYTES + 1).unwrap();

        let error = run_catalog_extraction_worker(&input, "pdf", "metadata", &output).unwrap_err();

        assert!(error.contains("extraction size limit"));
        assert!(!output.exists());
    }

    #[test]
    fn extracts_mobi_description_isbn_and_subjects_from_exth_metadata() {
        use mobi::headers::ExthRecord;

        let mut mobi = mobi::MobiMetadata::default();
        mobi.name = b"Embedded MOBI Title".to_vec();
        mobi.exth.records.insert(ExthRecord::Author, vec![b"Ada MOBI Author".to_vec()]);
        mobi.exth.records.insert(ExthRecord::Description, vec![b"A useful MOBI description.".to_vec()]);
        mobi.exth.records.insert(ExthRecord::Isbn, vec![b"978-0-8213-3827-8".to_vec()]);
        mobi.exth.records.insert(ExthRecord::Subject, vec![b"History".to_vec(), b"Sweden".to_vec()]);

        let (metadata, isbns, subjects) = mobi_catalog_metadata(&mobi);

        assert_eq!(metadata.title.as_deref(), Some("Embedded MOBI Title"));
        assert_eq!(metadata.authors[0].name, "Ada MOBI Author");
        assert_eq!(metadata.description.as_deref(), Some("A useful MOBI description."));
        assert_eq!(isbns, ["9780821338278"]);
        assert_eq!(subjects.iter().map(PublicationSubject::name).collect::<Vec<_>>(), ["History", "Sweden"]);
    }

    #[test]
    fn schedules_epub_mobi_and_pdf_for_the_same_extraction_pipeline() {
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        for (catalog_format, opds_format) in [(CatalogBookFormat::Epub, OpdsBookFormat::Epub), (CatalogBookFormat::Mobi, OpdsBookFormat::Mobi), (CatalogBookFormat::Pdf, OpdsBookFormat::Pdf)] {
            let identity = CatalogPublicationIdentity { edition_id: format!("{catalog_format:?}"), work_id: None };
            let mut publication = CatalogPublication {
                identity: identity.clone(),
                title: "Book".to_owned(),
                authors: Vec::new(),
                languages: Vec::new(),
                description: None,
                image_url: None,
                actions: vec![CatalogPublicationAction::Import {
                    request: CatalogImportRequest {
                        source_id: source_id.clone(),
                        catalog_locator: "https://example.com/opds".to_owned(),
                        publication: identity,
                        download_url: format!("https://example.com/book.{}", opds_format.extension()),
                        book_format: catalog_format,
                    },
                }],
            };
            assert_eq!(publication_metadata_acquisition(&publication).unwrap().book_format, opds_format);
            assert_eq!(publication_cover_fallback(&source_id, &mut publication).unwrap().1.book_format, opds_format);
            assert!(publication.image_url.as_deref().is_some_and(is_synthetic_cover_key));
        }
    }

    #[test]
    fn startup_reconciliation_removes_sources_no_longer_in_the_registry() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let stale = CatalogSourceId::parse("opds-stale").unwrap();
        cache.ensure_source(&stale, "https://stale.example/opds").unwrap();
        let configured = endpoint("https://example.com/opds".to_owned());

        assert_eq!(reconcile_cached_sources(&cache, std::slice::from_ref(&configured)).unwrap(), vec![stale]);
        let sources = cache.inventory().unwrap().sources;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, opds_catalog_source_id(&configured.id));
    }

    #[test]
    fn litteraturbanken_is_first_in_the_startup_crawl_queue() {
        let mut endpoints = vec![
            endpoint("https://example.com/first".to_owned()),
            OpdsEndpoint {
                id: OpdsEndpointId::parse(PRIORITY_CRAWL_SOURCE).unwrap(),
                title: "Litteraturbanken".to_owned(),
                description: "Swedish literature".to_owned(),
                languages: vec!["Swedish".to_owned()],
                url: "https://litteraturbanken.se/epub".to_owned(),
            },
        ];
        prioritize_startup_crawls(&mut endpoints);
        assert_eq!(endpoints[0].id.as_str(), PRIORITY_CRAWL_SOURCE);
    }

    #[tokio::test]
    async fn missing_embedded_subjects_use_the_client_ambiguous_match_policy() {
        let metadata = Router::new().route(
            "/v2/classifications",
            post(|body: Bytes| async move {
                let request = metadata_contract::decode_v2_request(&body, metadata_contract::MAX_REQUEST_BYTES).unwrap();
                let response = metadata_contract::RichMetadataEnrichmentResponse {
                    snapshot: metadata_contract::SnapshotDescription { dump_date: "2026-08".to_owned(), imported_at_ms: 1 },
                    results: vec![metadata_contract::RichIsbnMetadataResult {
                        requested_isbn: request.isbns[0].clone(),
                        canonical_isbn13: Some("9780821338278".to_owned()),
                        status: LookupStatus::Ambiguous,
                        matches: vec![
                            metadata_contract::RichWorkMetadataMatch {
                                open_library_work_id: Some("OL1W".to_owned()),
                                exact_edition_ids: vec!["OL1M".to_owned()],
                                classifications: vec![metadata_contract::Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.9".to_owned(), source: ClassificationSource::ExactEdition }],
                                authors: Vec::new(),
                                places: Vec::new(),
                                people: Vec::new(),
                                popularity: Some(metadata_contract::WorkPopularity { want_to_read_count: 2, currently_reading_count: 0, already_read_count: 1, rating_count: 1, rating_sum: 3 }),
                            },
                            metadata_contract::RichWorkMetadataMatch {
                                open_library_work_id: Some("OL2W".to_owned()),
                                exact_edition_ids: vec!["OL2M".to_owned()],
                                classifications: vec![metadata_contract::Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.9".to_owned(), source: ClassificationSource::Work }],
                                authors: Vec::new(),
                                places: Vec::new(),
                                people: Vec::new(),
                                popularity: Some(metadata_contract::WorkPopularity { want_to_read_count: 12, currently_reading_count: 2, already_read_count: 5, rating_count: 3, rating_sum: 13 }),
                            },
                        ],
                    }],
                    places: Vec::new(),
                    people: Vec::new(),
                };
                ([(header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)], metadata_contract::encode_v2_response(&response).unwrap())
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, metadata).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let mut service = service(endpoint("http://127.0.0.1:9/root".to_owned()), temporary.path());
        Arc::get_mut(&mut service.state).unwrap().metadata_endpoint = Some(Url::parse(&format!("http://{address}/v2/classifications")).unwrap());

        let enriched = service.metadata_matches(&["9780821338278".to_owned()]).await.unwrap();
        let subjects = &enriched["9780821338278"].subjects;
        assert_eq!(subjects[0].source(), "openlibrary:2026-08:exact-edition");
        let projected = library_model::project_subjects(subjects);
        assert_eq!(projected[0].system_id(), "lcc");
        assert_eq!(projected[0].subject_path(), "LCC / Science / Mathematics / Calculating machines / Electronic computers. Computer science / QA76.9");
        assert_eq!(enriched["9780821338278"].popularity.as_ref().unwrap().reading_log_count(), 19);
        task.abort();
    }

    #[tokio::test]
    async fn crawl_enriches_publications_from_temporary_epubs_and_reuses_the_downloaded_cover() {
        let epub_requests = Arc::new(AtomicUsize::new(0));
        let request_counter = epub_requests.clone();
        let epub = Arc::new(epub_with_embedded_cover());
        let response_epub = epub.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/opds+json"), (header::ETAG, "\"metadata-v1\"")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"Feed Title","author":[{"name":"Feed Author"}],"language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                        .into_response()
                }),
            )
            .route(
                "/one.epub",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    let bytes = response_epub.clone();
                    async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let catalog_service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();

        catalog_service.crawl_source(&source_id).await.unwrap();
        wait_for_successful_extractions(&catalog_service, 1).await;
        let response = catalog_service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
        let publication = &response.payloads[0].publication;
        assert_eq!(publication.title, "Embedded Title");
        assert_eq!(publication.authors.iter().map(|author| (author.name.as_str(), author.sort_name.as_deref())).collect::<Vec<_>>(), vec![("Ada Author", Some("Author, Ada"))]);
        assert_eq!(publication.languages, ["sv"]);
        assert_eq!(publication.description, Some(CatalogDescription::PlainText { text: "Embedded description".to_owned() }));
        let recommendations = catalog_service
            .recommendations(&CatalogRecommendationRequest {
                subjects: vec![catalog_contract::CatalogSubjectPreference { system_id: "bisac".to_owned(), subject_path: "History / United States".to_owned(), weight: 4 }],
                supported_languages: vec!["sv".to_owned()],
                excluded_publications: Vec::new(),
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(recommendations.recommendations[0].publication.identity.edition_id, "one");
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "non-range test origin receives one range probe and one full download");
        assert_eq!(std::fs::read_dir(temporary.path().join("acquisitions")).unwrap().count(), 0);
        let extraction = &catalog_service.state.cache.inventory().unwrap().sources[0].epub_extractions;
        assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 0, 1, 0));
        assert_eq!(extraction.attempts, 1);
        assert_eq!(extraction.downloaded_bytes, epub.len() as u64);
        let stored = catalog_service.state.cache.epub_metadata(&source_id, &format!("http://{address}/one.epub")).unwrap().unwrap();
        assert_eq!(stored.book.publication.publishers[0].name().as_str(), "Example Press");
        assert_eq!(stored.book.publication.dates[0].value(), "2024-05-06");
        assert_eq!(stored.toc[0].title, "The Hidden Chapter");
        let toc_search = catalog_service.search_response(&source_id, CatalogSearchRequest { query: "hidden chapter".to_owned(), supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
        assert_eq!(toc_search.toc_matches[0].identity.edition_id, "one");

        let image_url = publication.image_url.as_deref().unwrap();
        assert_eq!(image::guess_format(&catalog_service.catalog_cover(&source_id, image_url).await.unwrap()).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "metadata and cover extraction should share the full fallback download");

        catalog_service.crawl_source(&source_id).await.unwrap();
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "a modified feed should reuse persisted enrichment for the same EPUB URL");
        let recrawled = catalog_service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
        assert_eq!(recrawled.payloads[0].publication.title, "Embedded Title");

        drop(catalog_service);
        let restarted = service(endpoint(format!("http://{address}/root")), temporary.path());
        restarted.crawl_source(&source_id).await.unwrap();
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "persistent extraction state should prevent a redownload after restart");
        assert_eq!(restarted.state.cache.inventory().unwrap().epub_extractions.succeeded, 1);
        task.abort();
    }

    #[tokio::test]
    async fn independent_worker_resumes_a_persisted_pending_extraction() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let epub = Arc::new(epub_with_embedded_cover());
        let response_epub = epub.clone();
        let upstream = Router::new().route(
            "/one.epub",
            get(move || {
                request_counter.fetch_add(1, Ordering::SeqCst);
                let bytes = response_epub.clone();
                async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let root = format!("http://{address}/root");
        let acquisition_url = format!("http://{address}/one.epub");
        let service = service(endpoint(root.clone()), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let identity = CatalogPublicationIdentity { edition_id: "one".to_owned(), work_id: None };
        let publication = CatalogPublication {
            identity: identity.clone(),
            title: "Feed Title".to_owned(),
            authors: Vec::new(),
            languages: vec!["en".to_owned()],
            description: None,
            image_url: None,
            actions: vec![CatalogPublicationAction::Import {
                request: CatalogImportRequest { source_id: source_id.clone(), catalog_locator: root.clone(), publication: identity, download_url: acquisition_url.clone(), book_format: CatalogBookFormat::Epub },
            }],
        };
        let page = CatalogPage {
            source_id: source_id.clone(),
            freshness: catalog_contract::CatalogFreshness::Current,
            title: "Books".to_owned(),
            locator: root.clone(),
            navigation: Vec::new(),
            publications: vec![publication],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: catalog_contract::CatalogPagination::default(),
            search: None,
        };
        service.state.cache.put_page(&source_id, &root, &page, None, None).unwrap();
        service.resume_pending_epub_extractions().await;

        let extraction = &service.state.cache.inventory().unwrap().epub_extractions;
        assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 0, 1, 0));
        assert_eq!(requests.load(Ordering::SeqCst), 2, "non-range test origin receives one range probe and one full download");
        task.abort();
    }

    #[tokio::test]
    async fn crawl_activates_before_extraction_and_refresh_returns_an_incremental_delta() {
        let release = Arc::new(tokio::sync::Notify::new());
        let download_started = Arc::new(AtomicBool::new(false));
        let epub = Arc::new(epub_with_embedded_cover());
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"Feed Title","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/one.epub", get({
                let release = release.clone();
                let download_started = download_started.clone();
                move || {
                    let release = release.clone();
                    let download_started = download_started.clone();
                    let epub = epub.clone();
                    async move {
                        let first_request = !download_started.swap(true, Ordering::SeqCst);
                        if first_request {
                            release.notified().await;
                        }
                        ([(header::CONTENT_TYPE, "application/epub+zip")], epub.as_ref().clone())
                    }
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();

        tokio::time::timeout(Duration::from_secs(1), service.crawl_source(&source_id)).await.expect("crawl waited for EPUB extraction").unwrap();
        let base = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: None, known_publications: Vec::new() }).await.unwrap();
        assert_eq!(base.payloads[0].publication.title, "Feed Title");
        assert_eq!(service.state.cache.inventory().unwrap().sources[0].crawl_status, "succeeded");
        tokio::time::timeout(Duration::from_secs(1), async {
            while !download_started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        release.notify_waiters();
        wait_for_successful_extractions(&service, 1).await;
        let delta = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: Some(base.epoch.clone()), known_publications: base.page.publications.clone() }).await.unwrap();
        assert_eq!(delta.payloads.len(), 1);
        assert_eq!(delta.payloads[0].publication.title, "Embedded Title");
        assert!(delta.payloads[0].sequence > base.payloads[0].sequence);
        task.abort();
    }

    #[tokio::test]
    async fn coverless_epub_is_downloaded_once_then_discarded_after_thumbnail_extraction() {
        let epub_requests = Arc::new(AtomicUsize::new(0));
        let request_counter = epub_requests.clone();
        let epub = Arc::new(epub_with_embedded_cover());
        let response_epub = epub.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]},{"metadata":{"identifier":"pdf","title":"PDF","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/book.pdf","type":"application/pdf"}]}]}"#)
                }),
            )
            .route(
                "/one.epub",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    let bytes = response_epub.clone();
                    async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();

        let page = service.browser_page(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
        let image_url = page.publications[0].image_url.as_deref().unwrap();
        assert!(is_synthetic_cover_key(image_url));
        assert!(page.publications[1].image_url.as_deref().is_some_and(is_synthetic_cover_key), "PDF publications should use the shared extraction fallback");
        assert_eq!(epub_requests.load(Ordering::SeqCst), 0);

        let first = service.catalog_cover(&source_id, image_url).await.unwrap();
        assert_eq!(image::guess_format(&first).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "non-range test origin receives one range probe and one full download");
        assert_eq!(std::fs::read_dir(temporary.path().join("acquisitions")).unwrap().count(), 0);

        let second = service.catalog_cover(&source_id, image_url).await.unwrap();
        assert_eq!(second, first);
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2);
        task.abort();
    }

    #[tokio::test]
    async fn crawl_is_atomic_and_search_never_contacts_upstream() {
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let upstream = Router::new().route("/root", get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async {
                ([
                    (header::CONTENT_TYPE, "application/opds+json"),
                    (header::ETAG, "\"v1\""),
                ], r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"Harry Potter","author":[{"name":"J. K. Rowling"}],"language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#)
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let mut endpoint = endpoint(format!("http://{address}/root"));
        endpoint.languages = vec!["English".to_owned()];
        let service = service(endpoint, temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        service.crawl_source(&source_id).await.unwrap();
        let before_search = requests.load(Ordering::SeqCst);
        let response = service.search_response(&source_id, CatalogSearchRequest { query: "harry".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
        assert_eq!(response.title_matches.len(), 1);
        assert!(response.author_matches.is_empty());
        assert_eq!(response.payloads.len(), 1);
        let second =
            service.search_response(&source_id, CatalogSearchRequest { query: "harry".to_owned(), supported_languages: vec!["en".to_owned()], epoch: Some(response.epoch), known_publications: response.title_matches }).await.unwrap();
        assert!(second.payloads.is_empty());
        assert_eq!(requests.load(Ordering::SeqCst), before_search);
        task.abort();
    }

    #[tokio::test]
    async fn page_response_filters_languages_before_sending_unknown_payloads() {
        let upstream = Router::new().route("/root", get(|| async {
            ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]},{"metadata":{"identifier":"deux","title":"Deux","language":"fr"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/deux.epub","type":"application/epub+zip"}]}]}"#)
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        service.crawl_source(&source_id).await.unwrap();
        let first = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();
        assert_eq!(first.page.publications.len(), 1);
        assert_eq!(first.payloads.len(), 1);
        assert_eq!(first.payloads[0].publication.identity.edition_id, "one");
        let known = first.page.publications.clone();
        let second = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: Some(first.epoch), known_publications: known }).await.unwrap();
        assert!(second.payloads.is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn successful_crawl_removes_pages_no_longer_reachable_from_root() {
        let include_old_page = Arc::new(AtomicBool::new(true));
        let root_state = include_old_page.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(move || {
                    let body = if root_state.load(Ordering::SeqCst) {
                        r#"{"metadata":{"title":"Books"},"navigation":[{"title":"Old","href":"/old","type":"application/opds+json"}],"publications":[{"metadata":{"identifier":"current","title":"Current Book","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/current.epub","type":"application/epub+zip"}]}]}"#
                    } else {
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"current","title":"Current Book","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/current.epub","type":"application/epub+zip"}]}]}"#
                    };
                    async move { ([(header::CONTENT_TYPE, "application/opds+json")], body) }
                }),
            )
            .route(
                "/old",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Old"},"publications":[{"metadata":{"identifier":"old","title":"Removed Book","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/old.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();

        service.crawl_source(&source_id).await.unwrap();
        assert_eq!(service.search_response(&source_id, CatalogSearchRequest { query: "removed".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() },).await.unwrap().title_matches.len(), 1);

        include_old_page.store(false, Ordering::SeqCst);
        service.crawl_source(&source_id).await.unwrap();
        assert!(service.search_response(&source_id, CatalogSearchRequest { query: "removed".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() },).await.unwrap().title_matches.is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn crawl_delegates_cover_download_and_jpeg_thumbnail_once_per_url() {
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(320, 480).write_to(&mut source, ImageOutputFormat::Png).unwrap();
        let source = Arc::new(source.into_inner());
        let cover_requests = Arc::new(AtomicUsize::new(0));
        let response_source = source.clone();
        let response_requests = cover_requests.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/cover.png",
                get(move || {
                    response_requests.fetch_add(1, Ordering::SeqCst);
                    let bytes = response_source.clone();
                    async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let image_url = format!("http://{address}/cover.png");

        service.crawl_source(&source_id).await.unwrap();
        let jpeg = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(bytes) = service.catalog_cover(&source_id, &image_url).await {
                    break bytes;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(image::guess_format(&jpeg).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!(cover_requests.load(Ordering::SeqCst), 1);

        service.crawl_source(&source_id).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(cover_requests.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn tiny_catalog_cover_falls_back_to_cover_from_metadata_epub_download() {
        let mut tiny_cover = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 48).write_to(&mut tiny_cover, ImageOutputFormat::Png).unwrap();
        let tiny_cover = Arc::new(tiny_cover.into_inner());
        let epub = Arc::new(epub_with_embedded_cover());
        let cover_requests = Arc::new(AtomicUsize::new(0));
        let epub_requests = Arc::new(AtomicUsize::new(0));
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/cover.png",
                get({
                    let requests = cover_requests.clone();
                    let bytes = tiny_cover.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let bytes = bytes.clone();
                        async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
                    }
                }),
            )
            .route(
                "/one.epub",
                get({
                    let requests = epub_requests.clone();
                    let bytes = epub.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let bytes = bytes.clone();
                        async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let image_url = format!("http://{address}/cover.png");

        service.crawl_source(&source_id).await.unwrap();
        wait_for_successful_extractions(&service, 1).await;
        let jpeg = service.catalog_cover(&source_id, &image_url).await.unwrap();
        let dimensions = image::load_from_memory(&jpeg).unwrap().dimensions();

        assert_eq!(dimensions, (600, 900));
        assert_eq!(cover_requests.load(Ordering::SeqCst), 1);
        assert_eq!(epub_requests.load(Ordering::SeqCst), 2, "metadata and cover fallback must share one full download after the range probe");
        task.abort();
    }

    #[tokio::test]
    async fn tiny_catalog_and_epub_covers_make_publication_coverless() {
        let mut tiny_cover = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 48).write_to(&mut tiny_cover, ImageOutputFormat::Png).unwrap();
        let tiny_cover = Arc::new(tiny_cover.into_inner());
        let epub = Arc::new(epub_with_embedded_cover_dimensions(48, 72));
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/cover.png", get(move || {
                let bytes = tiny_cover.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
            }))
            .route("/one.epub", get(move || {
                let bytes = epub.clone();
                async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();

        service.crawl_source(&source_id).await.unwrap();
        wait_for_successful_extractions(&service, 1).await;
        let response = service.page_response(&source_id, CatalogPageRequest { locator: None, supported_languages: vec!["sv".to_owned()], epoch: None, known_publications: Vec::new() }).await.unwrap();

        assert_eq!(response.payloads[0].publication.image_url, None);
        task.abort();
    }

    #[tokio::test]
    async fn public_registry_is_uncompressed_binary_and_needs_no_authentication() {
        let service = CatalogService::bundled().unwrap();
        let response = app(service).oneshot(Request::builder().uri("/api/opds/catalogs").header(header::ACCEPT, transport_wire::MEDIA_TYPE).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
        let bytes = to_bytes(response.into_body(), transport_wire::MAX_DECODED_RESPONSE_BYTES).await.unwrap();
        let registry: CatalogRegistry = transport_wire::decode(&bytes, transport_wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
        assert!(!registry.catalogs.is_empty());
    }

    #[tokio::test]
    async fn browser_api_supports_cors_json_browsing_and_validated_acquisitions() {
        let mut cover = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(320, 480).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
        let response_cover = Arc::new(cover.into_inner());
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}],"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/search",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Search"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/cover.png", get(move || {
                let bytes = response_cover.clone();
                async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
            }))
            .route("/one.epub", get(|| async { ([(header::CONTENT_TYPE, "application/epub+zip")], b"epub bytes".as_slice()) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let router = app(service.clone());

        let preflight = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/web/sources/opds-test/search")
                    .header(header::ORIGIN, "https://app.bokheim.se")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(preflight.status().is_success());
        assert_eq!(preflight.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");

        let page_request = CatalogPageRequest { locator: None, supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() };
        let page = router
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/api/web/sources/opds-test/page").header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&page_request).unwrap())).unwrap())
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let page: CatalogPage = serde_json::from_slice(&to_bytes(page.into_body(), 1024 * 1024).await.unwrap()).unwrap();
        assert_eq!(page.publications[0].title, "One");
        let cover = service.catalog_cover(&source_id, page.publications[0].image_url.as_deref().unwrap()).await.unwrap();
        assert_eq!(image::guess_format(&cover).unwrap(), image::ImageFormat::Jpeg);

        let search_request = CatalogSearchRequest { query: "one".to_owned(), supported_languages: vec!["en".to_owned()], epoch: None, known_publications: Vec::new() };
        let search = router
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/api/web/sources/opds-test/search").header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&search_request).unwrap())).unwrap())
            .await
            .unwrap();
        assert_eq!(search.status(), StatusCode::OK);
        let search: CatalogPage = serde_json::from_slice(&to_bytes(search.into_body(), 1024 * 1024).await.unwrap()).unwrap();
        assert_eq!(search.publications[0].title, "One");

        let request = page.publications[0]
            .actions
            .iter()
            .find_map(|action| match action {
                catalog_contract::CatalogPublicationAction::Import { request } => Some(request.clone()),
                _ => None,
            })
            .unwrap();
        let acquisition =
            router.oneshot(Request::builder().method(Method::POST).uri("/api/web/sources/opds-test/acquire").header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_vec(&request).unwrap())).unwrap()).await.unwrap();
        assert_eq!(acquisition.status(), StatusCode::OK);
        assert_eq!(acquisition.headers().get(header::CONTENT_TYPE).unwrap(), "application/epub+zip");
        assert_eq!(&to_bytes(acquisition.into_body(), 1024).await.unwrap()[..], b"epub bytes");
        task.abort();
    }

    /// Background metadata downloads must not consume either interactive
    /// acquisition or cover capacity.
    #[tokio::test]
    async fn browser_acquisition_waits_for_the_background_extraction_worker() {
        let mut cover = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(320, 480).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
        let cover = Arc::new(cover.into_inner());
        let response_cover = cover.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One","language":"en"},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route("/one.epub", get(|| async { ([(header::CONTENT_TYPE, "application/epub+zip")], b"epub bytes".as_slice()) }))
            .route(
                "/cover.png",
                get(move || {
                    let bytes = response_cover.clone();
                    async move { ([(header::CONTENT_TYPE, "image/png")], bytes.as_ref().clone()) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        let source_id = CatalogSourceId::parse("opds-test").unwrap();

        let page = service.browser_page(&source_id, CatalogPageRequest { locator: None, supported_languages: Vec::new(), epoch: None, known_publications: Vec::new() }).await.unwrap();
        let request = page.publications[0]
            .actions
            .iter()
            .find_map(|action| match action {
                CatalogPublicationAction::Import { request } => Some(request.clone()),
                _ => None,
            })
            .unwrap();

        let enrichment = service.state.background_download_slots.acquire_many(MAX_CONCURRENT_DOWNLOADS as u32).await.unwrap();
        let downloaded = tokio::time::timeout(Duration::from_secs(5), service.browser_acquisition(&source_id, &request))
            .await
            .expect("browser acquisition must not block indefinitely behind background enrichment")
            .expect("browser acquisition must not be refused while background enrichment holds the permit");
        assert_eq!(std::fs::read(downloaded.path()).unwrap(), b"epub bytes");
        let image_url = page.publications[0].image_url.as_deref().unwrap();
        let cover = tokio::time::timeout(Duration::from_secs(5), service.catalog_cover(&source_id, image_url))
            .await
            .expect("cover lookup must not block behind background metadata downloads")
            .expect("cover lookup must retain independent interactive capacity");
        assert_eq!(image::guess_format(&cover).unwrap(), image::ImageFormat::Jpeg);
        drop(enrichment);
        task.abort();
    }

    #[tokio::test]
    async fn startup_refresh_activates_every_registered_source_without_hourly_spacing() {
        let epub = Arc::new(epub_with_embedded_cover());
        let response_epub = epub.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"identifier":"one","title":"One"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/one.epub","type":"application/epub+zip"}]}]}"#,
                    )
                }),
            )
            .route(
                "/one.epub",
                get(move || {
                    let bytes = response_epub.clone();
                    async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes.as_ref().clone()) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint(format!("http://{address}/root")), temporary.path());
        service.refresh_all_sources().await;
        wait_for_successful_extractions(&service, 1).await;
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        assert_eq!(service.state.cache.pages(&source_id).unwrap().len(), 1);
        let router = app(service.clone());
        let response = router.clone().oneshot(Request::builder().uri("/api/web/stats").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let stats: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert_eq!(stats["status"], "available");
        assert_eq!(stats["registry_revision"], 1);
        assert_eq!(stats["metadata_enrichment_configured"], false);
        assert!(stats["uptime_seconds"].is_u64());
        assert_eq!(stats["active_page_count"], 1);
        assert_eq!(stats["active_publication_count"], 1);
        assert_eq!(stats["active_epub_count"], 1);
        assert_eq!(stats["epub_extractions"]["succeeded"], 1);
        assert_eq!(stats["epub_extractions"]["attempts"], 1);
        assert_eq!(stats["sources"][0]["crawl_status"], "succeeded");
        assert_eq!(stats["sources"][0]["crawl_frontier_count"], 0);
        assert_eq!(stats["extraction_failures"], serde_json::json!([]));
        let response = router.oneshot(Request::builder().uri("/internal/operations").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = operations_app(service).oneshot(Request::builder().uri("/internal/operations").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let operations: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
        assert_eq!(operations["extraction_failures"], serde_json::json!([]));
        task.abort();
    }

    #[test]
    fn catalogue_failures_are_normalized_into_stable_retry_categories() {
        assert_eq!(classify_catalog_failure("publication download exceeded 45 seconds"), ("network_timeout", true));
        assert_eq!(classify_catalog_failure("HTTP 404 Not Found"), ("http_unavailable", false));
        assert_eq!(classify_catalog_failure("invalid EPUB ZIP central directory"), ("invalid_publication", false));
        assert_eq!(classify_catalog_failure("database worker stopped"), ("internal", true));
    }

    #[test]
    fn crawl_frontier_is_disk_backed_deduplicated_and_bounded() {
        let mut frontier = DiskCrawlFrontier::new("https://example.com/root", 3).unwrap();
        frontier.enqueue(["https://example.com/one".to_owned(), "https://example.com/one".to_owned(), "https://example.com/two".to_owned()]).unwrap();
        assert_eq!(frontier.claim_batch().unwrap(), ["https://example.com/root", "https://example.com/one", "https://example.com/two"]);
        frontier.enqueue(["https://example.com/root".to_owned()]).unwrap();
        assert!(frontier.claim_batch().unwrap().is_empty(), "claimed URLs must remain deduplicated on disk");

        let mut bounded = DiskCrawlFrontier::new("https://example.com/root", 2).unwrap();
        let error = bounded.enqueue(["https://example.com/one".to_owned(), "https://example.com/two".to_owned()]).unwrap_err();
        assert!(error.to_string().contains("exceeded 2 discovered pages"));
    }

    #[tokio::test]
    async fn metadata_proxy_compatibility_routes_remain_registered() {
        let temporary = tempfile::tempdir().unwrap();
        let service = service(endpoint("http://127.0.0.1:9/root".to_owned()), temporary.path());
        let body = Body::from(r#"{"isbns":["9780821338278"]}"#);
        let request = |uri: &'static str, body| Request::builder().method(Method::POST).uri(uri).header(header::CONTENT_TYPE, "application/json").body(body).unwrap();

        let legacy = app(service.clone()).oneshot(request("/api/metadata/enrichment", body)).await.unwrap();
        assert_eq!(legacy.status(), StatusCode::SERVICE_UNAVAILABLE);
        let rich = app(service).oneshot(request("/api/metadata/enrichment/v2", Body::from(r#"{"isbns":["9780821338278"]}"#))).await.unwrap();
        assert_eq!(rich.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

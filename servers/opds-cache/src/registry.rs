use super::{
    default_cache_path, default_cover_cache_directory, endpoints_from_registry, opds_catalog_source_id, opds_source, reconcile_stored_sources, store_error, validate_curated_registry, Arc, AtomicBool, BTreeMap, CatalogCoverCache,
    CatalogProcessRuntime, CatalogRegistry, CatalogService, CatalogServiceError, CatalogServiceState, CatalogSourceId, CatalogSourceRegistration, CatalogSourceRegistry, CatalogStore, Duration, HashMap, Instant,
    CatalogExtractionRuntimeMetrics, MetadataBackfillRuntimeMetrics, Mutex, Notify, OpdsClient, OpdsEndpoint, Path, PathBuf, RegistrySnapshot, RegistrySource, RwLock, Semaphore, StdMutex, Url, BACKGROUND_DOWNLOAD_LIMIT, BUNDLED_REGISTRY, CRAWL_PAUSE_SENTINEL,
    DEFAULT_MAX_CRAWL_PAGES, HARD_MAX_CRAWL_PAGES, MAX_CONCURRENT_EXTRACTIONS, MAX_INTERACTIVE_DOWNLOADS, MAX_QUEUED_DOWNLOADS, MAX_REGISTRY_BYTES,
};
use crate::http::public::read_registry_file;

impl CatalogService {
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

    pub async fn source_registry(&self) -> Result<CatalogSourceRegistry, CatalogServiceError> {
        let registry = self.registry().await;
        let endpoints = endpoints_from_registry(registry.clone()).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(CatalogSourceRegistry { revision: registry.revision, sources: endpoints.into_iter().map(|endpoint| CatalogSourceRegistration { source: opds_source(&endpoint), root_locator: endpoint.url }).collect() })
    }

    pub(crate) async fn refresh_registry(&self) {
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
    pub(crate) async fn public_endpoint(&self, source_id: &CatalogSourceId) -> Result<OpdsEndpoint, CatalogServiceError> {
        self.state.endpoints.read().await.iter().find(|endpoint| opds_catalog_source_id(&endpoint.id) == *source_id).cloned().ok_or_else(|| CatalogServiceError("catalog source is not registered".to_owned()))
    }

    pub(crate) async fn reconcile_sources(&self, endpoints: &[OpdsEndpoint]) -> Result<(), CatalogServiceError> {
        let endpoints = endpoints.to_vec();
        let acquisitions = self.state.acquisition_directory.clone();
        let covers = self.state.cover_cache_directory.clone();
        self.blocking_call(move |store| reconcile_stored_sources(&store, &endpoints, Some((&acquisitions,&covers))).map(|_| ())).await
    }

    pub(crate) fn from_bytes(source: RegistrySource, bytes: Vec<u8>, cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        let registry = parse_registry(bytes.clone())?;
        let endpoints = endpoints_from_registry(registry.clone()).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_path = cache_path.unwrap_or_else(default_cache_path);
        let acquisition_directory = cache_path.parent().map(|parent| parent.join("acquisitions")).unwrap_or_else(|| std::env::temp_dir().join("bokheim-catalog-acquisitions"));
        let store = CatalogStore::open(cache_path).map_err(store_error)?;
        store.pause_all_processes().map_err(store_error)?;
        let process_settings = store.process_settings().map_err(store_error)?;
        reconcile_stored_sources(&store, &endpoints, Some((&acquisition_directory,&cover_cache_directory)))?;
        let covers = CatalogCoverCache::new_public_only(cover_cache_directory.clone()).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(Self {
            state: Arc::new(CatalogServiceState {
                started_at: Instant::now(),
                source,
                snapshot: RwLock::new(RegistrySnapshot { observed_bytes: bytes, registry }),
                endpoints: RwLock::new(endpoints),
                opds: OpdsClient::new_public_only().map_err(|error| CatalogServiceError(error.to_string()))?,
                store,
                covers,
                cover_cache_directory,
                store_write_lock: Mutex::new(()),
                crawl_locks: Mutex::new(HashMap::new()),
                crawl_progress: Mutex::new(HashMap::new()),
                origin_throttles: Mutex::new(HashMap::new()),
                source_update_locks: Mutex::new(HashMap::new()),
                epub_range_origins: Mutex::new(HashMap::new()),
                background_download_slots: Semaphore::new(BACKGROUND_DOWNLOAD_LIMIT),
                interactive_download_slots: Semaphore::new(MAX_INTERACTIVE_DOWNLOADS),
                queued_download_slots: Semaphore::new(MAX_QUEUED_DOWNLOADS),
                extraction_slots: Semaphore::new(MAX_CONCURRENT_EXTRACTIONS),
                extraction_notify: Notify::new(),
                processes: CatalogProcessRuntime::new(process_settings),
                // EPUBs are a durable hand-off between the download and
                // extraction pools. Keep the spool beside the catalog cache
                // so staged acquisitions survive service restarts and are
                // not hidden inside systemd's private /tmp namespace.
                acquisition_directory,
                metadata_endpoint: configured_metadata_endpoint()?,
                metadata_client: reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(15)).build().map_err(|error| CatalogServiceError(error.to_string()))?,
                metadata_backfill_metrics: MetadataBackfillRuntimeMetrics::default(),
                description_backfill_metrics: MetadataBackfillRuntimeMetrics::default(),
                extraction_metrics: CatalogExtractionRuntimeMetrics::default(),
                description_backfill_available: AtomicBool::new(false),
                writer_metrics: StdMutex::new(BTreeMap::new()),
                max_crawl_pages: configured_max_crawl_pages()?,
            }),
        })
    }
}

pub(crate) fn parse_registry(bytes: Vec<u8>) -> Result<CatalogRegistry, CatalogServiceError> {
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(CatalogServiceError(format!("catalog registry exceeds {MAX_REGISTRY_BYTES} bytes")));
    }
    let registry: CatalogRegistry = serde_json::from_slice(&bytes).map_err(|error| CatalogServiceError(format!("catalog registry is not valid JSON: {error}")))?;
    validate_curated_registry(&registry).map_err(|error| CatalogServiceError(error.to_string()))?;
    Ok(registry)
}

pub(crate) fn configured_metadata_endpoint() -> Result<Option<Url>, CatalogServiceError> {
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

pub(crate) fn configured_max_crawl_pages() -> Result<usize, CatalogServiceError> {
    let Some(value) = std::env::var_os("BOKHEIM_CATALOG_MAX_CRAWL_PAGES") else { return Ok(DEFAULT_MAX_CRAWL_PAGES) };
    let value = value.to_string_lossy();
    let pages = value.parse::<usize>().map_err(|_| CatalogServiceError("BOKHEIM_CATALOG_MAX_CRAWL_PAGES must be a positive integer".to_owned()))?;
    if pages == 0 || pages > HARD_MAX_CRAWL_PAGES {
        return Err(CatalogServiceError(format!("BOKHEIM_CATALOG_MAX_CRAWL_PAGES must be between 1 and {HARD_MAX_CRAWL_PAGES}")));
    }
    Ok(pages)
}

pub(crate) fn catalog_crawls_disabled() -> bool {
    std::env::var_os("BOKHEIM_CATALOG_DISABLE_CRAWLS").is_some() || std::path::Path::new(CRAWL_PAUSE_SENTINEL).exists()
}

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use bokheim_app_contract::catalog::{CatalogAuthorDiscoveryRequest, CatalogAuthorDiscoveryResult, CatalogPage, CatalogSourceId, CatalogWideSearchResult};
use bokheim_catalog::{opds_catalog_source_id, opds_page, opds_search_definition, opds_search_query, resolve_author_discovery, search_all_catalogs, validate_opds_catalog_locator, CatalogCoverCache, CatalogServerSearchRequest};
use bokheim_opds::{endpoints_from_registry, validate_curated_registry, CatalogRegistry, OpdsClient, OpdsEndpoint};
use bokheim_wire::{self as wire, ContentEncoding};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, Semaphore};

const MAX_REGISTRY_BYTES: usize = 2 * 1024 * 1024;
const CACHE_CONTROL: &str = "public, max-age=300, stale-if-error=604800";
const PAGE_CACHE_CONTROL: &str = "public, max-age=60, stale-if-error=604800";
const COVER_CACHE_CONTROL: &str = "public, max-age=31536000, immutable, no-transform";
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_SEARCH_CACHE_ENTRIES: usize = 64;
const PREPARED_RESPONSE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PREPARED_RESPONSE_ENTRIES: usize = 512;
const MAX_PREPARED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CONCURRENT_GLOBAL_SEARCHES: usize = 2;
const REFRESH_CONCURRENCY: usize = 6;
const SERVER_CACHE_ENTRIES: usize = 256;
const SERVER_CACHE_BYTES: usize = 64 * 1024 * 1024;
const BUNDLED_REGISTRY: &[u8] = include_bytes!("../opds.json");

#[derive(Clone)]
pub struct CatalogService {
    state: Arc<CatalogServiceState>,
}

struct CatalogServiceState {
    source: RegistrySource,
    snapshot: RwLock<RegistrySnapshot>,
    endpoints: RwLock<Vec<OpdsEndpoint>>,
    opds: OpdsClient,
    search_cache: RwLock<HashMap<String, CachedSearch>>,
    search_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    prepared_responses: RwLock<PreparedResponseCache>,
    prepared_response_locks: Mutex<HashMap<PreparedResponseKey, Arc<Mutex<()>>>>,
    search_slots: Semaphore,
    covers: CatalogCoverCache,
}

enum RegistrySource {
    Bundled,
    File(PathBuf),
}

struct RegistrySnapshot {
    observed_bytes: Vec<u8>,
    registry: CatalogRegistry,
}

struct CachedSearch {
    inserted_at: Instant,
    response: PreparedSearch,
}

#[derive(Clone)]
struct PreparedSearch {
    result: Arc<CatalogWideSearchResult>,
    wire_bytes: Bytes,
    wire_encoding: ContentEncoding,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum PreparedResponseKey {
    Registry { revision: u64 },
    Page { registry_revision: u64, source_id: CatalogSourceId, locator: String },
    Search { registry_revision: u64, source_id: CatalogSourceId, request_hash: String },
}

#[derive(Clone)]
struct PreparedWireResponse {
    wire_bytes: Bytes,
    wire_encoding: ContentEncoding,
}

struct CachedPreparedResponse {
    inserted_at: Instant,
    response: PreparedWireResponse,
}

#[derive(Default)]
struct PreparedResponseCache {
    entries: HashMap<PreparedResponseKey, CachedPreparedResponse>,
    total_bytes: usize,
}

#[derive(Deserialize)]
struct PageQuery {
    locator: Option<String>,
    #[serde(default)]
    fresh: bool,
}

#[derive(Deserialize)]
struct WideSearchQuery {
    q: String,
}

#[derive(Deserialize)]
struct CoverQuery {
    image_url: String,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

#[derive(Debug)]
pub struct CatalogServiceError(String);

impl CatalogService {
    pub fn bundled() -> Result<Self, CatalogServiceError> {
        Self::from_bytes(RegistrySource::Bundled, BUNDLED_REGISTRY.to_vec(), None, default_cover_cache_directory())
    }

    pub fn bundled_with_cache(cache_path: PathBuf) -> Result<Self, CatalogServiceError> {
        let cover_cache_directory = cover_cache_directory_for(&cache_path);
        Self::from_bytes(RegistrySource::Bundled, BUNDLED_REGISTRY.to_vec(), Some(cache_path), cover_cache_directory)
    }

    pub fn bundled_with_storage(cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        Self::from_bytes(RegistrySource::Bundled, BUNDLED_REGISTRY.to_vec(), cache_path, cover_cache_directory)
    }

    pub async fn from_file(path: impl Into<PathBuf>) -> Result<Self, CatalogServiceError> {
        let path = path.into();
        let bytes = read_registry_file(&path).await?;
        Self::from_bytes(RegistrySource::File(path), bytes, None, default_cover_cache_directory())
    }

    pub async fn from_file_with_cache(path: impl Into<PathBuf>, cache_path: PathBuf) -> Result<Self, CatalogServiceError> {
        let path = path.into();
        let bytes = read_registry_file(&path).await?;
        let cover_cache_directory = cover_cache_directory_for(&cache_path);
        Self::from_bytes(RegistrySource::File(path), bytes, Some(cache_path), cover_cache_directory)
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
        let RegistrySource::File(path) = &self.state.source else {
            return self.state.snapshot.read().await.registry.clone();
        };
        let bytes = match read_registry_file(path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "retaining the last valid catalog registry");
                return self.state.snapshot.read().await.registry.clone();
            }
        };
        {
            let snapshot = self.state.snapshot.read().await;
            if snapshot.observed_bytes == bytes {
                return snapshot.registry.clone();
            }
        }
        let mut snapshot = self.state.snapshot.write().await;
        if snapshot.observed_bytes == bytes {
            return snapshot.registry.clone();
        }
        snapshot.observed_bytes = bytes.clone();
        match parse_registry(bytes) {
            Ok(registry) if registry.revision > snapshot.registry.revision => {
                tracing::info!(revision = registry.revision, catalogs = registry.catalogs.len(), "activated updated catalog registry");
                *self.state.endpoints.write().await = endpoints_from_registry(registry.clone()).expect("validated registry has valid endpoints");
                self.state.search_cache.write().await.clear();
                *self.state.prepared_responses.write().await = PreparedResponseCache::default();
                self.state.prepared_response_locks.lock().await.clear();
                snapshot.registry = registry;
            }
            Ok(registry) if registry.revision == snapshot.registry.revision && registry == snapshot.registry => {}
            Ok(registry) if registry.revision == snapshot.registry.revision => tracing::warn!(revision = registry.revision, "retaining the last valid catalog registry because content changed without a revision increment"),
            Ok(registry) => tracing::warn!(candidate_revision = registry.revision, active_revision = snapshot.registry.revision, "retaining the last valid catalog registry instead of activating a stale revision"),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "retaining the last valid catalog registry after an invalid update");
            }
        }
        snapshot.registry.clone()
    }

    pub async fn catalog_page(&self, source_id: &CatalogSourceId, locator: Option<&str>, fresh: bool) -> Result<CatalogPage, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = locator.unwrap_or(&endpoint.url);
        validate_opds_catalog_locator(&endpoint, locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        let catalog = if fresh { self.state.opds.fetch_catalog_fresh(locator).await } else { self.state.opds.fetch_catalog(locator).await }.map_err(|error| CatalogServiceError(error.to_string()))?;
        let page = opds_page(source_id.clone(), catalog);
        self.state.covers.remember_page(&page).await;
        Ok(page)
    }

    pub async fn search_catalog(&self, source_id: &CatalogSourceId, request: CatalogServerSearchRequest) -> Result<CatalogPage, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let requested_definition = opds_search_definition(&request.search).map_err(|error| CatalogServiceError(error.to_string()))?;
        let root = self.state.opds.fetch_catalog(&endpoint.url).await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let definition = root.search.ok_or_else(|| CatalogServiceError("catalog does not advertise search".to_owned()))?;
        if requested_definition != definition {
            return Err(CatalogServiceError("catalog search definition is not the currently advertised definition".to_owned()));
        }
        let search_origin = match &definition {
            bokheim_opds::OpdsSearch::UriTemplate { base_url, .. } => base_url,
            bokheim_opds::OpdsSearch::OpenSearchDescription { url } => url,
        };
        validate_opds_catalog_locator(&endpoint, search_origin).map_err(|error| CatalogServiceError(error.to_string()))?;
        let catalog = self.state.opds.search_definition(&definition, &opds_search_query(request.query)).await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let page = opds_page(source_id.clone(), catalog);
        self.state.covers.remember_page(&page).await;
        Ok(page)
    }

    async fn prepared_registry(&self, registry: CatalogRegistry) -> Result<PreparedWireResponse, CatalogServiceError> {
        let key = PreparedResponseKey::Registry { revision: registry.revision };
        self.prepared_response(key, async move { Ok(registry) }).await
    }

    async fn prepared_catalog_page(&self, source_id: &CatalogSourceId, locator: Option<&str>, fresh: bool) -> Result<PreparedWireResponse, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = locator.unwrap_or(&endpoint.url).to_owned();
        validate_opds_catalog_locator(&endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        let registry_revision = self.state.snapshot.read().await.registry.revision;
        let key = PreparedResponseKey::Page { registry_revision, source_id: source_id.clone(), locator: locator.clone() };
        if fresh {
            let page = self.catalog_page(source_id, Some(&locator), true).await?;
            let response = prepare_wire_response_async(page).await?;
            self.cache_prepared_response(key, response.clone()).await;
            return Ok(response);
        }
        self.prepared_response(key, self.catalog_page(source_id, Some(&locator), false)).await
    }

    async fn prepared_catalog_search(&self, source_id: &CatalogSourceId, request: CatalogServerSearchRequest) -> Result<PreparedWireResponse, CatalogServiceError> {
        self.public_endpoint(source_id).await?;
        let request_bytes = wire::encode(&request).map_err(|error| CatalogServiceError(error.to_string()))?;
        let registry_revision = self.state.snapshot.read().await.registry.revision;
        let key = PreparedResponseKey::Search { registry_revision, source_id: source_id.clone(), request_hash: blake3::hash(&request_bytes).to_hex().to_string() };
        self.prepared_response(key, self.search_catalog(source_id, request)).await
    }

    async fn prepared_response<T, F>(&self, key: PreparedResponseKey, value: F) -> Result<PreparedWireResponse, CatalogServiceError>
    where
        T: Serialize + Send + 'static,
        F: Future<Output = Result<T, CatalogServiceError>>,
    {
        if let Some(response) = self.cached_prepared_response(&key).await {
            return Ok(response);
        }
        let lock = {
            let mut locks = self.state.prepared_response_locks.lock().await;
            locks.entry(key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
        };
        let guard = lock.lock().await;
        if let Some(response) = self.cached_prepared_response(&key).await {
            drop(guard);
            self.state.prepared_response_locks.lock().await.remove(&key);
            return Ok(response);
        }
        let response = match value.await {
            Ok(value) => prepare_wire_response_async(value).await,
            Err(error) => Err(error),
        };
        if let Ok(response) = &response {
            self.cache_prepared_response(key.clone(), response.clone()).await;
        }
        drop(guard);
        self.state.prepared_response_locks.lock().await.remove(&key);
        response
    }

    async fn cached_prepared_response(&self, key: &PreparedResponseKey) -> Option<PreparedWireResponse> {
        let mut cache = self.state.prepared_responses.write().await;
        let expired = cache.entries.get(key).is_some_and(|cached| !matches!(key, PreparedResponseKey::Registry { .. }) && cached.inserted_at.elapsed() > PREPARED_RESPONSE_TTL);
        if expired {
            if let Some(cached) = cache.entries.remove(key) {
                cache.total_bytes = cache.total_bytes.saturating_sub(cached.response.wire_bytes.len());
            }
            return None;
        }
        cache.entries.get(key).map(|cached| cached.response.clone())
    }

    async fn cache_prepared_response(&self, key: PreparedResponseKey, response: PreparedWireResponse) {
        let response_bytes = response.wire_bytes.len();
        if response_bytes > MAX_PREPARED_RESPONSE_BYTES {
            return;
        }
        let mut cache = self.state.prepared_responses.write().await;
        if let Some(previous) = cache.entries.remove(&key) {
            cache.total_bytes = cache.total_bytes.saturating_sub(previous.response.wire_bytes.len());
        }
        while cache.entries.len() >= MAX_PREPARED_RESPONSE_ENTRIES || cache.total_bytes.saturating_add(response_bytes) > MAX_PREPARED_RESPONSE_BYTES {
            let Some(oldest) = cache.entries.iter().min_by_key(|(_, cached)| cached.inserted_at).map(|(key, _)| key.clone()) else { break };
            if let Some(previous) = cache.entries.remove(&oldest) {
                cache.total_bytes = cache.total_bytes.saturating_sub(previous.response.wire_bytes.len());
            }
        }
        cache.total_bytes = cache.total_bytes.saturating_add(response_bytes);
        cache.entries.insert(key, CachedPreparedResponse { inserted_at: Instant::now(), response });
    }

    pub async fn search_catalogs(&self, query: &str) -> Result<CatalogWideSearchResult, CatalogServiceError> {
        let requested_query = query.trim().to_owned();
        let response = self.search_catalogs_cached(query).await?;
        let mut result = response.result.as_ref().clone();
        result.query = requested_query;
        Ok(result)
    }

    pub async fn discover_author_books(&self, request: CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, CatalogServiceError> {
        let mut identifier_searches = Vec::with_capacity(request.book_identifiers.len());
        for identifier in &request.book_identifiers {
            let search = self.search_catalogs_cached(&identifier.value).await?;
            identifier_searches.push((identifier, search));
        }
        let name_search = self.search_catalogs_cached(&request.author_name).await?;
        let identifier_searches = identifier_searches.iter().map(|(identifier, search)| (*identifier, search.result.as_ref())).collect::<Vec<_>>();
        resolve_author_discovery(&request, &identifier_searches, name_search.result.as_ref()).map_err(|error| CatalogServiceError(error.to_string()))
    }

    async fn search_catalogs_cached(&self, query: &str) -> Result<PreparedSearch, CatalogServiceError> {
        let query = query.trim();
        if query.is_empty() || query.len() > 512 || query.chars().any(char::is_control) {
            return Err(CatalogServiceError("catalog search query must be non-empty bounded text without control characters".to_owned()));
        }
        let cache_key = query.to_lowercase();
        if let Some(response) = self.cached_search(&cache_key).await {
            return Ok(response);
        }
        let search_lock = {
            let mut locks = self.state.search_locks.lock().await;
            locks.entry(cache_key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
        };
        let guard = search_lock.lock().await;
        if let Some(response) = self.cached_search(&cache_key).await {
            drop(guard);
            self.state.search_locks.lock().await.remove(&cache_key);
            return Ok(response);
        }
        let endpoints = self.state.endpoints.read().await.clone();
        let _search_slot = self.state.search_slots.acquire().await.map_err(|_| CatalogServiceError("catalog search scheduler stopped".to_owned()))?;
        let result = search_all_catalogs(&self.state.opds, endpoints, query).await.map_err(|error| CatalogServiceError(error.to_string()));
        let result = match result {
            Ok(mut result) => {
                result.query = cache_key.clone();
                self.state.covers.remember_wide_search(&result).await;
                self.cache_search(cache_key.clone(), result).await
            }
            Err(error) => Err(error),
        };
        if let Ok(result) = &result {
            debug_assert_eq!(result.result.query, cache_key);
        }
        drop(guard);
        self.state.search_locks.lock().await.remove(&cache_key);
        result
    }

    pub async fn refresh_public_catalogs(&self) {
        let endpoints = self.state.endpoints.read().await.clone();
        stream::iter(endpoints)
            .for_each_concurrent(REFRESH_CONCURRENCY, |endpoint| async move {
                match self.state.opds.fetch_catalog_fresh(&endpoint.url).await {
                    Ok(_) => tracing::info!(catalog = endpoint.id.as_str(), "refreshed public OPDS cache"),
                    Err(error) => tracing::warn!(catalog = endpoint.id.as_str(), %error, "could not refresh public OPDS cache"),
                }
            })
            .await;
    }

    pub async fn run_refresh_loop(self, interval: Duration) {
        loop {
            self.refresh_public_catalogs().await;
            tokio::time::sleep(interval).await;
        }
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogServiceError> {
        self.public_endpoint(source_id).await?;
        self.state.covers.cover(source_id, image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
    }

    async fn public_endpoint(&self, source_id: &CatalogSourceId) -> Result<OpdsEndpoint, CatalogServiceError> {
        self.state
            .endpoints
            .read()
            .await
            .iter()
            .find(|endpoint| opds_catalog_source_id(&endpoint.id) == *source_id)
            .cloned()
            .ok_or_else(|| CatalogServiceError("catalog source is not a registered public OPDS endpoint".to_owned()))
    }

    async fn cached_search(&self, cache_key: &str) -> Option<PreparedSearch> {
        self.state.search_cache.read().await.get(cache_key).filter(|cached| cached.inserted_at.elapsed() <= SEARCH_CACHE_TTL).map(|cached| cached.response.clone())
    }

    async fn cache_search(&self, cache_key: String, result: CatalogWideSearchResult) -> Result<PreparedSearch, CatalogServiceError> {
        let (result, encoded) = tokio::task::spawn_blocking(move || {
            let encoded = wire::encode_transport(&result, true).map_err(|error| CatalogServiceError(error.to_string()))?;
            Ok::<_, CatalogServiceError>((result, encoded))
        })
        .await
        .map_err(|error| CatalogServiceError(error.to_string()))??;
        let response = PreparedSearch { result: Arc::new(result), wire_bytes: Bytes::from(encoded.bytes), wire_encoding: encoded.encoding };
        let mut cache = self.state.search_cache.write().await;
        while cache.len() >= MAX_SEARCH_CACHE_ENTRIES {
            let Some(oldest) = cache.iter().min_by_key(|(_, cached)| cached.inserted_at).map(|(key, _)| key.clone()) else { break };
            cache.remove(&oldest);
        }
        cache.insert(cache_key, CachedSearch { inserted_at: Instant::now(), response: response.clone() });
        Ok(response)
    }

    fn from_bytes(source: RegistrySource, bytes: Vec<u8>, cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        let registry = parse_registry(bytes.clone())?;
        let endpoints = endpoints_from_registry(registry.clone()).map_err(|error| CatalogServiceError(error.to_string()))?;
        let opds = match cache_path {
            Some(path) => OpdsClient::with_persistent_cache_capacity(path, SERVER_CACHE_ENTRIES, SERVER_CACHE_BYTES),
            None => OpdsClient::with_cache_capacity(SERVER_CACHE_ENTRIES, SERVER_CACHE_BYTES),
        }
        .map_err(|error| CatalogServiceError(error.to_string()))?;
        let covers = CatalogCoverCache::new_persistent(cover_cache_directory).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(Self {
            state: Arc::new(CatalogServiceState {
                source,
                snapshot: RwLock::new(RegistrySnapshot { observed_bytes: bytes, registry }),
                endpoints: RwLock::new(endpoints),
                opds,
                search_cache: RwLock::new(HashMap::new()),
                search_locks: Mutex::new(HashMap::new()),
                prepared_responses: RwLock::new(PreparedResponseCache::default()),
                prepared_response_locks: Mutex::new(HashMap::new()),
                search_slots: Semaphore::new(MAX_CONCURRENT_GLOBAL_SEARCHES),
                covers,
            }),
        })
    }
}

fn default_cover_cache_directory() -> PathBuf {
    std::env::temp_dir().join(format!("bokheim-catalog-server-{}-covers", std::process::id()))
}

fn cover_cache_directory_for(cache_path: &Path) -> PathBuf {
    cache_path.parent().unwrap_or_else(|| Path::new(".")).join("catalog-covers")
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
        .route("/api/opds/search", get(wide_search))
        .route("/api/opds/authors/discover", post(discover_author_books))
        .route("/api/opds/sources/:source_id/page", get(catalog_page))
        .route("/api/opds/sources/:source_id/search", post(catalog_search))
        .route("/api/opds/sources/:source_id/cover", get(catalog_cover))
        .with_state(service)
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn catalogs(State(service): State<CatalogService>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    require_wire_transport(&request_headers)?;
    let registry = service.registry().await;
    let etag = format!("\"opds-{}\"", registry.revision);
    if request_headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        add_wire_headers(&mut response, CACHE_CONTROL, ContentEncoding::Identity);
        response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("revision ETag contains only ASCII"));
        return Ok(response);
    }
    let prepared = service.prepared_registry(registry).await.map_err(ApiError::internal)?;
    let mut response = prepared_wire_response(prepared, CACHE_CONTROL);
    response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("revision ETag contains only ASCII"));
    Ok(response)
}

async fn catalog_page(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Query(query): Query<PageQuery>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    require_wire_transport(&request_headers)?;
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let prepared = service.prepared_catalog_page(&source_id, query.locator.as_deref(), query.fresh).await.map_err(ApiError::bad_gateway)?;
    Ok(prepared_wire_response(prepared, if query.fresh { "no-store" } else { PAGE_CACHE_CONTROL }))
}

async fn catalog_search(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, request_headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_transport(&request_headers)?;
    let request: CatalogServerSearchRequest = decode_wire_request(&request_headers, &body)?;
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let prepared = service.prepared_catalog_search(&source_id, request).await.map_err(ApiError::bad_gateway)?;
    Ok(prepared_wire_response(prepared, PAGE_CACHE_CONTROL))
}

async fn wide_search(State(service): State<CatalogService>, Query(query): Query<WideSearchQuery>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    require_wire_transport(&request_headers)?;
    let result = service.search_catalogs_cached(&query.q).await.map_err(ApiError::bad_gateway)?;
    let mut response = result.wire_bytes.clone().into_response();
    add_wire_headers(&mut response, PAGE_CACHE_CONTROL, result.wire_encoding);
    Ok(response)
}

async fn discover_author_books(State(service): State<CatalogService>, request_headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_transport(&request_headers)?;
    let request: CatalogAuthorDiscoveryRequest = decode_wire_request(&request_headers, &body)?;
    let result = service.discover_author_books(request).await.map_err(ApiError::bad_gateway)?;
    let prepared = prepare_wire_response_async(result).await.map_err(ApiError::bad_gateway)?;
    Ok(prepared_wire_response(prepared, PAGE_CACHE_CONTROL))
}

async fn catalog_cover(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Query(query): Query<CoverQuery>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let bytes = service.catalog_cover(&source_id, &query.image_url).await.map_err(ApiError::bad_gateway)?;
    let (bytes, etag) = tokio::task::spawn_blocking(move || {
        let etag = format!("\"{}\"", blake3::hash(&bytes).to_hex());
        (bytes, etag)
    })
    .await
    .map_err(ApiError::internal)?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(COVER_CACHE_CONTROL));
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(header::ETAG, HeaderValue::from_str(&etag).expect("BLAKE3 ETag contains only ASCII"));
    if request_headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }
    Ok((headers, bytes).into_response())
}

fn require_wire_transport(headers: &HeaderMap) -> Result<(), ApiError> {
    if !headers.get_all(header::ACCEPT).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(wire::is_current_media_type) {
        return Err(ApiError { status: StatusCode::NOT_ACCEPTABLE, message: "catalog request requires the current Bokheim binary media type".to_owned() });
    }
    if !headers.get_all(header::ACCEPT_ENCODING).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(|value| value.trim().eq_ignore_ascii_case(wire::CONTENT_ENCODING_ZSTD)) {
        return Err(ApiError { status: StatusCode::NOT_ACCEPTABLE, message: "catalog request requires zstd support".to_owned() });
    }
    Ok(())
}

fn decode_wire_request<T: serde::de::DeserializeOwned>(headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
    let media_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
    if !media_type.is_some_and(wire::is_current_media_type) {
        return Err(ApiError::bad_request("catalog request requires the current Bokheim binary content type"));
    }
    let encoding = headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok());
    let encoding = match encoding {
        None | Some("identity") => ContentEncoding::Identity,
        Some(value) if value.eq_ignore_ascii_case(wire::CONTENT_ENCODING_ZSTD) => ContentEncoding::Zstd,
        Some(_) => return Err(ApiError::bad_request("catalog request body uses an unsupported content encoding")),
    };
    wire::decode_transport(body, encoding, wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| ApiError::bad_request(error.to_string()))
}

async fn prepare_wire_response_async(value: impl Serialize + Send + 'static) -> Result<PreparedWireResponse, CatalogServiceError> {
    tokio::task::spawn_blocking(move || {
        let encoded = wire::encode_transport(&value, true).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(PreparedWireResponse { wire_bytes: Bytes::from(encoded.bytes), wire_encoding: encoded.encoding })
    })
    .await
    .map_err(|error| CatalogServiceError(error.to_string()))?
}

fn prepared_wire_response(prepared: PreparedWireResponse, cache_control: &'static str) -> Response {
    let mut response = prepared.wire_bytes.into_response();
    add_wire_headers(&mut response, cache_control, prepared.wire_encoding);
    response
}

fn add_wire_headers(response: &mut Response, cache_control: &'static str, encoding: ContentEncoding) {
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
    if encoding == ContentEncoding::Zstd {
        response.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(wire::CONTENT_ENCODING_ZSTD));
    }
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response.headers_mut().insert(header::VARY, HeaderValue::from_static("Accept, Accept-Encoding"));
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: message.into() }
    }

    fn bad_gateway(error: CatalogServiceError) -> Self {
        Self { status: StatusCode::BAD_GATEWAY, message: error.to_string() }
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
        return Err(CatalogServiceError(format!("catalog registry {} exceeds the {MAX_REGISTRY_BYTES}-byte limit", path.display())));
    }
    tokio::fs::read(path).await.map_err(|error| CatalogServiceError(format!("could not read catalog registry {}: {error}", path.display())))
}

fn parse_registry(bytes: Vec<u8>) -> Result<CatalogRegistry, CatalogServiceError> {
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(CatalogServiceError(format!("catalog registry exceeds the {MAX_REGISTRY_BYTES}-byte limit")));
    }
    let registry: CatalogRegistry = serde_json::from_slice(&bytes).map_err(|error| CatalogServiceError(format!("catalog registry is not valid JSON: {error}")))?;
    validate_curated_registry(&registry).map_err(|error| CatalogServiceError(error.to_string()))?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::get;
    use bokheim_app_contract::catalog::{CatalogAuthorDiscoveryEvidence, CatalogPublicationIdentifier, CatalogPublicationIdentifierScheme};
    use bokheim_opds::{registry_from_endpoints, OpdsEndpointId};
    use image::{DynamicImage, ImageOutputFormat};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    fn service_with_endpoint(endpoint: OpdsEndpoint) -> CatalogService {
        service_with_endpoint_and_cover_directory(endpoint, default_cover_cache_directory())
    }

    fn service_with_endpoint_and_cover_directory(endpoint: OpdsEndpoint, cover_cache_directory: PathBuf) -> CatalogService {
        let registry = registry_from_endpoints(1, std::slice::from_ref(&endpoint));
        CatalogService {
            state: Arc::new(CatalogServiceState {
                source: RegistrySource::Bundled,
                snapshot: RwLock::new(RegistrySnapshot { observed_bytes: serde_json::to_vec(&registry).unwrap(), registry }),
                endpoints: RwLock::new(vec![endpoint]),
                opds: OpdsClient::new().unwrap(),
                search_cache: RwLock::new(HashMap::new()),
                search_locks: Mutex::new(HashMap::new()),
                prepared_responses: RwLock::new(PreparedResponseCache::default()),
                prepared_response_locks: Mutex::new(HashMap::new()),
                search_slots: Semaphore::new(MAX_CONCURRENT_GLOBAL_SEARCHES),
                covers: CatalogCoverCache::new_persistent(cover_cache_directory).unwrap(),
            }),
        }
    }

    fn endpoint(url: String) -> OpdsEndpoint {
        OpdsEndpoint {
            id: OpdsEndpointId::parse("test").unwrap(),
            title: "Test".to_owned(),
            description: "Test catalog".to_owned(),
            languages: vec!["English".to_owned()],
            url,
        }
    }

    fn wire_get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).header(header::ACCEPT, wire::MEDIA_TYPE).header(header::ACCEPT_ENCODING, wire::CONTENT_ENCODING_ZSTD).body(Body::empty()).unwrap()
    }

    async fn decode_wire_response<T: serde::de::DeserializeOwned>(response: Response) -> T {
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
        let encoding = match response.headers().get(header::CONTENT_ENCODING) {
            Some(value) => {
                assert_eq!(value, wire::CONTENT_ENCODING_ZSTD);
                ContentEncoding::Zstd
            }
            None => ContentEncoding::Identity,
        };
        let bytes = to_bytes(response.into_body(), wire::MAX_DECODED_RESPONSE_BYTES).await.unwrap();
        wire::decode_transport(&bytes, encoding, wire::MAX_DECODED_RESPONSE_BYTES).unwrap()
    }

    #[tokio::test]
    async fn public_catalog_endpoint_requires_no_authentication_and_is_cacheable() {
        let service = CatalogService::bundled().unwrap();
        let application = app(service.clone());
        let unsupported = application.clone().oneshot(Request::builder().uri("/api/opds/catalogs").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(unsupported.status(), StatusCode::NOT_ACCEPTABLE);
        let response = application.clone().oneshot(wire_get("/api/opds/catalogs")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), CACHE_CONTROL);
        let etag = response.headers().get(header::ETAG).unwrap().clone();
        let registry: CatalogRegistry = decode_wire_response(response).await;
        assert_eq!(registry.catalogs.len(), 23);
        let first_prepared = service.prepared_registry(registry.clone()).await.unwrap();
        let second_prepared = service.prepared_registry(registry).await.unwrap();
        assert_eq!(first_prepared.wire_bytes.as_ptr(), second_prepared.wire_bytes.as_ptr(), "registry cache hits must reuse the same compressed allocation");
        let response = application
            .oneshot(Request::builder().uri("/api/opds/catalogs").header(header::ACCEPT, wire::MEDIA_TYPE).header(header::ACCEPT_ENCODING, wire::CONTENT_ENCODING_ZSTD).header(header::IF_NONE_MATCH, etag).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn file_updates_activate_without_restarting_and_invalid_updates_fail_safe() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("opds.json");
        tokio::fs::write(&path, BUNDLED_REGISTRY).await.unwrap();
        let service = CatalogService::from_file(&path).await.unwrap();
        let mut updated = service.registry().await;
        service.prepared_registry(updated.clone()).await.unwrap();
        assert_eq!(service.state.prepared_responses.read().await.entries.len(), 1);
        updated.revision += 1;
        updated.catalogs.truncate(1);
        tokio::fs::write(&path, serde_json::to_vec(&updated).unwrap()).await.unwrap();
        assert_eq!(service.registry().await, updated);
        assert!(service.state.prepared_responses.read().await.entries.is_empty(), "registry activation must invalidate prepared bytes from the previous revision");
        let mut stale = updated.clone();
        stale.revision -= 1;
        tokio::fs::write(&path, serde_json::to_vec(&stale).unwrap()).await.unwrap();
        assert_eq!(service.registry().await, updated);
        let mut mutated = updated.clone();
        mutated.catalogs[0].title = "Changed without revision".to_owned();
        tokio::fs::write(&path, serde_json::to_vec(&mutated).unwrap()).await.unwrap();
        assert_eq!(service.registry().await, updated);
        tokio::fs::write(&path, b"not json").await.unwrap();
        assert_eq!(service.registry().await, updated);
    }

    #[tokio::test]
    async fn invalid_or_empty_registries_are_rejected_at_startup() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("opds.json");
        tokio::fs::write(&path, br#"{"schema_version":1,"revision":1,"catalogs":[]}"#).await.unwrap();
        assert!(CatalogService::from_file(&path).await.is_err());
    }

    #[tokio::test]
    async fn prepared_response_cache_is_bounded_and_accounts_for_encoded_bytes() {
        let service = CatalogService::bundled().unwrap();
        for revision in 0..=MAX_PREPARED_RESPONSE_ENTRIES as u64 {
            service.cache_prepared_response(PreparedResponseKey::Registry { revision }, PreparedWireResponse { wire_bytes: Bytes::from_static(b"x"), wire_encoding: ContentEncoding::Identity }).await;
        }
        let prepared = service.state.prepared_responses.read().await;
        assert_eq!(prepared.entries.len(), MAX_PREPARED_RESPONSE_ENTRIES);
        assert_eq!(prepared.total_bytes, prepared.entries.values().map(|cached| cached.response.wire_bytes.len()).sum::<usize>());
        assert!(prepared.total_bytes <= MAX_PREPARED_RESPONSE_BYTES);
    }

    #[tokio::test]
    async fn public_page_proxy_reuses_the_shared_server_cache() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let upstream = Router::new().route(
            "/root",
            get(move || {
                request_counter.fetch_add(1, Ordering::SeqCst);
                async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Cached"},"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let service = service_with_endpoint(endpoint(format!("http://{address}/root")));
        let application = app(service.clone());
        let unsupported_page = application.clone().oneshot(Request::builder().uri("/api/opds/sources/opds-test/page").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(unsupported_page.status(), StatusCode::NOT_ACCEPTABLE);
        let json_search = application
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/opds/sources/opds-test/search")
                    .header(header::ACCEPT, wire::MEDIA_TYPE)
                    .header(header::ACCEPT_ENCODING, wire::CONTENT_ENCODING_ZSTD)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::CONTENT_ENCODING, wire::CONTENT_ENCODING_ZSTD)
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json_search.status(), StatusCode::BAD_REQUEST);
        let first_page = application.clone().oneshot(wire_get("/api/opds/sources/opds-test/page"));
        let second_page = application.clone().oneshot(wire_get("/api/opds/sources/opds-test/page"));
        let (first_page, second_page) = tokio::join!(first_page, second_page);
        for response in [first_page.unwrap(), second_page.unwrap()] {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");
            let page: CatalogPage = decode_wire_response(response).await;
            assert_eq!(page.title, "Cached");
        }
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let first_prepared = service.prepared_catalog_page(&source_id, None, false).await.unwrap();
        let second_prepared = service.prepared_catalog_page(&source_id, None, false).await.unwrap();
        assert_eq!(first_prepared.wire_bytes.as_ptr(), second_prepared.wire_bytes.as_ptr(), "page cache hits must reuse the same compressed allocation");
        {
            let mut prepared = service.state.prepared_responses.write().await;
            let page_entry = prepared.entries.iter_mut().find(|(key, _)| matches!(key, PreparedResponseKey::Page { .. })).unwrap().1;
            page_entry.inserted_at = Instant::now().checked_sub(PREPARED_RESPONSE_TTL + Duration::from_secs(1)).unwrap();
        }
        let expired_rebuilt = service.prepared_catalog_page(&source_id, None, false).await.unwrap();
        assert_ne!(first_prepared.wire_bytes.as_ptr(), expired_rebuilt.wire_bytes.as_ptr(), "expired prepared bytes must be rebuilt");
        assert_eq!(requests.load(Ordering::SeqCst), 1, "rebuilding prepared bytes should still reuse the parsed OPDS cache");
        let fresh_prepared = service.prepared_catalog_page(&source_id, None, true).await.unwrap();
        assert_ne!(expired_rebuilt.wire_bytes.as_ptr(), fresh_prepared.wire_bytes.as_ptr(), "fresh=true must bypass prepared bytes");
        let cached_after_fresh = service.prepared_catalog_page(&source_id, None, false).await.unwrap();
        assert_eq!(fresh_prepared.wire_bytes.as_ptr(), cached_after_fresh.wire_bytes.as_ptr(), "fresh=true must replace the prepared entry");
        let refreshed = application.clone().oneshot(wire_get("/api/opds/sources/opds-test/page?fresh=true")).await.unwrap();
        assert_eq!(refreshed.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(decode_wire_response::<CatalogPage>(refreshed).await.title, "Cached");
        let cached_after_refresh = application.clone().oneshot(wire_get("/api/opds/sources/opds-test/page")).await.unwrap();
        assert_eq!(decode_wire_response::<CatalogPage>(cached_after_refresh).await.title, "Cached");
        assert_eq!(requests.load(Ordering::SeqCst), 1, "prepared-byte refreshes must still reuse a current parsed OPDS page");
        upstream_task.abort();
    }

    #[tokio::test]
    async fn application_client_reads_normalized_pages_from_the_catalog_server() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let searches = Arc::new(AtomicUsize::new(0));
        let search_counter = searches.clone();
        let upstream = Router::new()
            .route(
                "/root",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Through Server"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}],"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#) }
                }),
            )
            .route(
                "/search",
                get(move || {
                    search_counter.fetch_add(1, Ordering::SeqCst);
                    async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Search Result"},"navigation":[{"title":"Match","href":"/book","type":"application/opds+json"}]}"#) }
                }),
            );
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream_listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move { axum::serve(upstream_listener, upstream).await.unwrap() });
        let server_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_address = server_listener.local_addr().unwrap();
        let service = service_with_endpoint(endpoint(format!("http://{upstream_address}/root")));
        let server = app(service.clone());
        let server_task = tokio::spawn(async move { axum::serve(server_listener, server).await.unwrap() });
        let client = bokheim_catalog::CatalogServerClient::new(&format!("http://{server_address}/")).unwrap();
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        assert_eq!(client.registry().await.unwrap().catalogs.len(), 1);
        let page = client.catalog_page(&source_id, None, false).await.unwrap();
        assert_eq!(page.title, "Through Server");
        assert_eq!(client.catalog_page(&source_id, None, false).await.unwrap().title, "Through Server");
        let search = page.search.unwrap();
        let query = bokheim_app_contract::catalog::CatalogSearchQuery { query: "book".to_owned(), title: None, author: None };
        assert_eq!(client.search_catalog(&source_id, search.clone(), query.clone()).await.unwrap().title, "Search Result");
        assert_eq!(client.search_catalog(&source_id, search.clone(), query.clone()).await.unwrap().title, "Search Result");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        let request = CatalogServerSearchRequest { search, query };
        let first_prepared = service.prepared_catalog_search(&source_id, request.clone()).await.unwrap();
        let second_prepared = service.prepared_catalog_search(&source_id, request).await.unwrap();
        assert_eq!(first_prepared.wire_bytes.as_ptr(), second_prepared.wire_bytes.as_ptr(), "source-search cache hits must reuse the same compressed allocation");
        let prepared = service.state.prepared_responses.read().await;
        assert_eq!(prepared.entries.keys().filter(|key| matches!(key, PreparedResponseKey::Search { .. })).count(), 1);
        server_task.abort();
        upstream_task.abort();
    }

    #[tokio::test]
    async fn shared_cover_endpoint_authorizes_normalizes_and_persists_catalog_covers() {
        let cover_requests = Arc::new(AtomicUsize::new(0));
        let cover_counter = cover_requests.clone();
        let cover_started = Arc::new(tokio::sync::Notify::new());
        let cover_start = cover_started.clone();
        let mut png = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(16, 24).write_to(&mut png, ImageOutputFormat::Png).unwrap();
        let cover_bytes = png.into_inner();
        let upstream = Router::new()
            .route("/root", get(|| async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Covers"},"publications":[{"metadata":{"identifier":"book-1","title":"Book","author":[{"name":"Writer"}]},"images":[{"href":"/cover.png","type":"image/png"}],"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/book.epub","type":"application/epub+zip"}]}]}"#) }))
            .route(
                "/cover.png",
                get(move || {
                    cover_counter.fetch_add(1, Ordering::SeqCst);
                    cover_start.notify_one();
                    let bytes = cover_bytes.clone();
                    async move {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        ([(header::CONTENT_TYPE, "image/png")], bytes)
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let endpoint = endpoint(format!("http://{address}/root"));
        let cover_url = format!("http://{address}/cover.png");
        let cover_uri = format!("/api/opds/sources/opds-test/cover?image_url={cover_url}");
        let unknown_uri = format!("/api/opds/sources/opds-test/cover?image_url=http://{address}/unknown.png");
        let service = service_with_endpoint_and_cover_directory(endpoint.clone(), temporary.path().join("covers"));
        let application = app(service);
        let unknown = application.clone().oneshot(Request::builder().uri(&unknown_uri).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(unknown.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(cover_requests.load(Ordering::SeqCst), 0);
        let page = application.clone().oneshot(wire_get("/api/opds/sources/opds-test/page")).await.unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let cover_request = application.clone().oneshot(Request::builder().uri(&cover_uri).body(Body::empty()).unwrap());
        let cover_task = tokio::spawn(cover_request);
        cover_started.notified().await;
        let health = tokio::time::timeout(Duration::from_millis(50), application.clone().oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())).await.unwrap().unwrap();
        assert_eq!(health.status(), StatusCode::OK, "a slow cover fetch must not block unrelated server work");
        let first = cover_task.await.unwrap().unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(first.headers().get(header::CONTENT_TYPE).unwrap(), "image/jpeg");
        assert_eq!(first.headers().get(header::CACHE_CONTROL).unwrap(), COVER_CACHE_CONTROL);
        assert_eq!(first.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");
        let etag = first.headers().get(header::ETAG).unwrap().clone();
        assert!(to_bytes(first.into_body(), 8 * 1024 * 1024).await.unwrap().starts_with(&[0xff, 0xd8]));
        let unchanged = application.clone().oneshot(Request::builder().uri(&cover_uri).header(header::IF_NONE_MATCH, etag).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(cover_requests.load(Ordering::SeqCst), 1);
        let restored = app(service_with_endpoint_and_cover_directory(endpoint, temporary.path().join("covers")));
        let persisted = restored.oneshot(Request::builder().uri(&cover_uri).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(persisted.status(), StatusCode::OK);
        assert_eq!(cover_requests.load(Ordering::SeqCst), 1);
        upstream_task.abort();
    }

    #[tokio::test]
    async fn global_search_fans_out_once_and_caches_the_normalized_result() {
        let searches = Arc::new(AtomicUsize::new(0));
        let search_counter = searches.clone();
        let upstream = Router::new()
            .route("/root", get(|| async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Searchable"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}],"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#) }))
            .route("/search", get(move || {
                search_counter.fetch_add(1, Ordering::SeqCst);
                async { ([(header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Results"},"publications":[{"metadata":{"identifier":"book-1","title":"One Book","author":[{"name":"Writer"}]},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/book.epub","type":"application/epub+zip"}]}]}"#) }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let service = service_with_endpoint(endpoint(format!("http://{address}/root")));
        let first = service.search_catalogs("book").await.unwrap();
        let second = service.search_catalogs("BOOK").await.unwrap();
        assert!(first.groups.iter().any(|group| matches!(&group.outcome, bokheim_app_contract::catalog::CatalogWideSearchOutcome::Results { page } if page.publications.iter().any(|publication| publication.title == "One Book"))));
        assert_eq!(second.groups, first.groups);
        let application = app(service);
        let response =
            application.clone().oneshot(Request::builder().uri("/api/opds/search?q=book").header(header::ACCEPT, wire::MEDIA_TYPE).header(header::ACCEPT_ENCODING, wire::CONTENT_ENCODING_ZSTD).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
        let encoding = if response.headers().get(header::CONTENT_ENCODING).is_some() { ContentEncoding::Zstd } else { ContentEncoding::Identity };
        let first_bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let routed: CatalogWideSearchResult = wire::decode_transport(&first_bytes, encoding, 1024 * 1024).unwrap();
        assert_eq!(routed.groups, first.groups);
        let repeated =
            application.clone().oneshot(Request::builder().uri("/api/opds/search?q=book").header(header::ACCEPT, wire::MEDIA_TYPE).header(header::ACCEPT_ENCODING, wire::CONTENT_ENCODING_ZSTD).body(Body::empty()).unwrap()).await.unwrap();
        let repeated_bytes = to_bytes(repeated.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(repeated_bytes, first_bytes, "cache hits must reuse the exact encoded bytes");
        let unsupported = application.oneshot(Request::builder().uri("/api/opds/search?q=book").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(unsupported.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        upstream_task.abort();
    }

    #[tokio::test]
    async fn author_discovery_uses_book_identifiers_to_separate_same_named_authors() {
        let upstream = Router::new()
            .route(
                "/root",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/opds+json")],
                        r#"{"metadata":{"title":"Authors"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}],"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#,
                    )
                }),
            )
            .route(
                "/search",
                get(|Query(parameters): Query<HashMap<String, String>>| async move {
                    let body = if parameters.get("query").map(String::as_str) == Some("9780306406157") {
                        r#"{"metadata":{"title":"Identifier result"},"publications":[{"metadata":{"identifier":"urn:isbn:9780306406157","title":"Known local book","author":[{"name":"Smith, Alex","identifier":"urn:isni:correct"}]},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/known.epub","type":"application/epub+zip"}]}]}"#
                    } else {
                        r#"{"metadata":{"title":"Name results"},"publications":[{"metadata":{"identifier":"catalogue:correct:second","title":"Another book by the identified author","author":[{"name":"Alex Smith","identifier":"urn:isni:correct"}]},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/correct.epub","type":"application/epub+zip"}]},{"metadata":{"identifier":"catalogue:collision","title":"Book by a different Alex Smith","author":[{"name":"Alex Smith","identifier":"urn:isni:different"}]},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/collision.epub","type":"application/epub+zip"}]}]}"#
                    };
                    ([(header::CONTENT_TYPE, "application/opds+json")], body)
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let service = service_with_endpoint(endpoint(format!("http://{address}/root")));

        let result = service
            .discover_author_books(CatalogAuthorDiscoveryRequest {
                author_name: "Alex Smith".to_owned(),
                book_identifiers: vec![CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "9780306406157".to_owned() }],
            })
            .await
            .unwrap();

        assert_eq!(result.author_name, "Alex Smith");
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].publication.title, "Another book by the identified author");
        assert!(matches!(result.matches[0].evidence, CatalogAuthorDiscoveryEvidence::BookIdentifier { .. }));
        assert_eq!(result.matches[1].publication.title, "Book by a different Alex Smith");
        assert_eq!(result.matches[1].evidence, CatalogAuthorDiscoveryEvidence::AuthorName);
        assert!(result.matches.iter().all(|result| result.publication.title != "Known local book"));
        upstream_task.abort();
    }

    #[test]
    fn catalog_server_has_no_user_data_or_authentication_dependencies() {
        let manifest = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
        for forbidden in ["server-core", "server-http", "server-postgres", "server-asset-store", "sqlx", "sync-common"] {
            assert!(!manifest.contains(forbidden), "catalog server must not depend on {forbidden}");
        }
    }
}

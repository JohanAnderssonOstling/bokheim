use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use catalog_contract::{CatalogLinkTarget, CatalogPage, CatalogPageRequest, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchRequest, CatalogSearchResponse, CatalogSourceId};
use catalog_core::{opds_catalog_source_id, opds_page, opds_source, validate_opds_catalog_locator, CatalogCache, CatalogCoverCache, CatalogCoverPipeline, CatalogLanguageFilter};
use opds::{endpoints_from_registry, validate_curated_registry, CatalogRegistry, OpdsClient, OpdsConditionalCatalog, OpdsEndpoint, OpdsHttpValidators};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use url::Url;

const MAX_REGISTRY_BYTES: usize = 2 * 1024 * 1024;
const MAX_CRAWL_PAGES: usize = 10_000;
const COVER_DOWNLOAD_WORKERS: usize = 8;
const COVER_THUMBNAIL_WORKERS: usize = 4;
const CRAWL_CYCLE: Duration = Duration::from_secs(24 * 60 * 60);
const REGISTRY_CACHE_CONTROL: &str = "public, max-age=300";
const DATA_CACHE_CONTROL: &str = "public, max-age=60";
const COVER_CACHE_CONTROL: &str = "public, max-age=31536000, immutable, no-transform";
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
    cache: CatalogCache,
    covers: CatalogCoverCache,
    cover_pipeline: CatalogCoverPipeline,
    crawl_locks: Mutex<HashMap<CatalogSourceId, Arc<Mutex<()>>>>,
}

enum RegistrySource {
    Bundled,
    File(PathBuf),
}

struct RegistrySnapshot {
    observed_bytes: Vec<u8>,
    registry: CatalogRegistry,
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
                let endpoints = endpoints_from_registry(registry.clone()).expect("validated endpoints");
                if let Err(error) = self.reconcile_sources(&endpoints).await {
                    tracing::warn!(%error, "retaining the last catalog registry after cache reconciliation failed");
                    return snapshot.registry.clone();
                }
                *self.state.endpoints.write().await = endpoints;
                snapshot.registry = registry;
            }
            Ok(registry) if registry.revision == snapshot.registry.revision && registry == snapshot.registry => {}
            Ok(registry) if registry.revision == snapshot.registry.revision => {
                tracing::warn!(revision = registry.revision, "registry changed without a revision increment");
            }
            Ok(registry) => {
                tracing::warn!(candidate_revision = registry.revision, active_revision = snapshot.registry.revision, "ignoring stale registry");
            }
            Err(error) => tracing::warn!(%error, "ignoring invalid registry update"),
        }
        snapshot.registry.clone()
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

    async fn crawl_endpoint(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        self.state.cache.ensure_source(&source_id, &endpoint.url).map_err(cache_error)?;
        let previous_pages = self.state.cache.pages(&source_id).map_err(cache_error)?;
        let previous = previous_pages.into_iter().map(|page| (page.page.locator.clone(), page)).collect::<HashMap<_, _>>();
        let crawl = self.state.cache.begin_crawl(&source_id, &endpoint.url).map_err(cache_error)?;
        let mut queue = VecDeque::from([endpoint.url.clone()]);
        let mut visited = HashSet::new();
        let mut cover_urls = HashSet::new();
        let result = async {
            while let Some(locator) = queue.pop_front() {
                if !visited.insert(locator.clone()) {
                    continue;
                }
                if visited.len() > MAX_CRAWL_PAGES {
                    return Err(CatalogServiceError(format!("catalog crawl exceeded {MAX_CRAWL_PAGES} pages")));
                }
                validate_opds_catalog_locator(&endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
                let cached = previous.get(&locator);
                let validators = cached.map(|page| OpdsHttpValidators { etag: page.etag.clone(), last_modified: page.last_modified.clone() }).unwrap_or_default();
                let (page, validators) = match self.state.opds.fetch_catalog_conditional(&locator, &validators).await.map_err(|error| CatalogServiceError(error.to_string()))? {
                    OpdsConditionalCatalog::NotModified => {
                        let cached = cached.ok_or_else(|| CatalogServiceError("upstream returned 304 for an uncached catalog page".to_owned()))?;
                        (cached.page.clone(), validators)
                    }
                    OpdsConditionalCatalog::Modified { catalog, validators } => {
                        validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
                        (opds_page(source_id.clone(), catalog), validators)
                    }
                };
                self.state.cache.stage_page(&crawl, &page, validators.etag.as_deref(), validators.last_modified.as_deref()).map_err(cache_error)?;
                enqueue_internal_links(&endpoint, &page, &visited, &mut queue)?;
                cover_urls.extend(page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter_map(|publication| publication.image_url.clone()));
            }
            self.state.cache.activate_crawl(&crawl).map_err(cache_error)?;
            self.state.cover_pipeline.enqueue(source_id.clone(), cover_urls).await;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = self.state.cache.abandon_crawl(&crawl);
        }
        result
    }

    pub async fn run_refresh_loop(self) {
        loop {
            let endpoints = self.state.endpoints.read().await.clone();
            let spacing = CRAWL_CYCLE / u32::try_from(endpoints.len().max(1)).unwrap_or(1);
            for endpoint in endpoints {
                let service = self.clone();
                tokio::spawn(async move {
                    let source_id = opds_catalog_source_id(&endpoint.id);
                    if let Err(error) = service.crawl_source(&source_id).await {
                        tracing::warn!(source = source_id.as_str(), %error, "catalog crawl failed; retaining the previous generation");
                    }
                });
                tokio::time::sleep(spacing).await;
            }
        }
    }

    pub async fn page_response(&self, source_id: &CatalogSourceId, request: CatalogPageRequest) -> Result<CatalogPageResponse, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = request.locator.as_deref().unwrap_or(&endpoint.url).to_owned();
        validate_opds_catalog_locator(&endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        let source = opds_source(&endpoint);
        let filter = CatalogLanguageFilter::new(&request.supported_languages);
        if !filter.source_is_multilingual(&source) {
            return self
                .state
                .cache
                .page_response(source_id, &locator, request.epoch.as_deref(), &request.known_publications)
                .map_err(cache_error)?
                .ok_or_else(|| CatalogServiceError("catalog has no active cached generation for this page".to_owned()));
        }

        let mut response = self.state.cache.page_response(source_id, &locator, None, &[]).map_err(cache_error)?.ok_or_else(|| CatalogServiceError("catalog has no active cached generation for this page".to_owned()))?;
        let supported = response.payloads.iter().filter(|payload| filter.publication_supported(&payload.publication)).map(|payload| payload.publication.identity.clone()).collect::<HashSet<_>>();
        response.page.publications.retain(|publication| supported.contains(&publication.identity));
        for group in &mut response.page.groups {
            group.publications.retain(|publication| supported.contains(&publication.identity));
        }
        retain_delta_payloads(&mut response.payloads, response.page.publications.iter().chain(response.page.groups.iter().flat_map(|group| group.publications.iter())), request.epoch.as_deref(), &response.epoch, request.known_publications);
        Ok(response)
    }

    pub async fn search_response(&self, source_id: &CatalogSourceId, request: CatalogSearchRequest) -> Result<CatalogSearchResponse, CatalogServiceError> {
        validate_query(&request.query)?;
        let endpoint = self.public_endpoint(source_id).await?;
        let source = opds_source(&endpoint);
        let filter = CatalogLanguageFilter::new(&request.supported_languages);
        if self.state.cache.pages(source_id).map_err(cache_error)?.is_empty() {
            return Err(CatalogServiceError("catalog has no active cached generation".to_owned()));
        }
        if !filter.source_is_multilingual(&source) {
            return self.state.cache.search_response(source_id, &request.query, request.epoch.as_deref(), &request.known_publications).map_err(cache_error)?.ok_or_else(|| CatalogServiceError("catalog has not been crawled".to_owned()));
        }
        let mut response = self.state.cache.search_response(source_id, &request.query, None, &[]).map_err(cache_error)?.ok_or_else(|| CatalogServiceError("catalog has not been crawled".to_owned()))?;
        let supported = response.payloads.iter().filter(|payload| filter.publication_supported(&payload.publication)).map(|payload| payload.publication.identity.clone()).collect::<HashSet<_>>();
        response.title_matches.retain(|item| supported.contains(&item.identity));
        response.author_matches.retain(|item| supported.contains(&item.identity));
        retain_delta_payloads(&mut response.payloads, response.title_matches.iter().chain(&response.author_matches), request.epoch.as_deref(), &response.epoch, request.known_publications);
        Ok(response)
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogServiceError> {
        self.public_endpoint(source_id).await?;
        let known = self
            .state
            .cache
            .pages(source_id)
            .map_err(cache_error)?
            .iter()
            .any(|page| page.page.publications.iter().chain(page.page.groups.iter().flat_map(|group| group.publications.iter())).any(|publication| publication.image_url.as_deref() == Some(image_url)));
        if !known {
            return Err(CatalogServiceError("cover URL is not present in the active catalog generation".to_owned()));
        }
        self.state.covers.cached_cover(image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
    }

    async fn public_endpoint(&self, source_id: &CatalogSourceId) -> Result<OpdsEndpoint, CatalogServiceError> {
        self.state.endpoints.read().await.iter().find(|endpoint| opds_catalog_source_id(&endpoint.id) == *source_id).cloned().ok_or_else(|| CatalogServiceError("catalog source is not registered".to_owned()))
    }

    async fn reconcile_sources(&self, endpoints: &[OpdsEndpoint]) -> Result<(), CatalogServiceError> {
        let retained = endpoints.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<HashSet<_>>();
        let current = self.state.endpoints.read().await.clone();
        for endpoint in endpoints {
            self.state.cache.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).map_err(cache_error)?;
        }
        for endpoint in current {
            let source_id = opds_catalog_source_id(&endpoint.id);
            if !retained.contains(&source_id) {
                self.state.cache.remove_source(&source_id).map_err(cache_error)?;
            }
        }
        Ok(())
    }

    fn from_bytes(source: RegistrySource, bytes: Vec<u8>, cache_path: Option<PathBuf>, cover_cache_directory: PathBuf) -> Result<Self, CatalogServiceError> {
        let registry = parse_registry(bytes.clone())?;
        let endpoints = endpoints_from_registry(registry.clone()).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_path = cache_path.unwrap_or_else(default_cache_path);
        let cache = CatalogCache::open(cache_path).map_err(cache_error)?;
        for endpoint in &endpoints {
            cache.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).map_err(cache_error)?;
        }
        let covers = CatalogCoverCache::new(cover_cache_directory).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cover_pipeline = CatalogCoverPipeline::new(covers.clone(), COVER_DOWNLOAD_WORKERS, COVER_THUMBNAIL_WORKERS).map_err(|error| CatalogServiceError(error.to_string()))?;
        Ok(Self {
            state: Arc::new(CatalogServiceState {
                source,
                snapshot: RwLock::new(RegistrySnapshot { observed_bytes: bytes, registry }),
                endpoints: RwLock::new(endpoints),
                opds: OpdsClient::new().map_err(|error| CatalogServiceError(error.to_string()))?,
                cache,
                covers,
                cover_pipeline,
                crawl_locks: Mutex::new(HashMap::new()),
            }),
        })
    }
}

fn enqueue_internal_links(endpoint: &OpdsEndpoint, page: &CatalogPage, visited: &HashSet<String>, queue: &mut VecDeque<String>) -> Result<(), CatalogServiceError> {
    let links = page.navigation.iter().chain(page.groups.iter().flat_map(|group| group.navigation.iter())).filter_map(|link| match &link.target {
        CatalogLinkTarget::Page { locator } => Some(locator.as_str()),
        CatalogLinkTarget::External { .. } => None,
    });
    for locator in links
        .chain(page.groups.iter().filter_map(|group| group.page_locator.as_deref()))
        .chain([page.pagination.first.as_deref(), page.pagination.previous.as_deref(), page.pagination.next.as_deref(), page.pagination.last.as_deref()].into_iter().flatten())
    {
        if !same_origin(&endpoint.url, locator)? {
            continue;
        }
        if !visited.contains(locator) && !queue.iter().any(|queued| queued == locator) {
            queue.push_back(locator.to_owned());
        }
    }
    Ok(())
}

fn same_origin(root: &str, candidate: &str) -> Result<bool, CatalogServiceError> {
    let root = Url::parse(root).map_err(|error| CatalogServiceError(error.to_string()))?;
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
    std::env::temp_dir().join(format!("bokheim-opds-cache-{}.db", std::process::id()))
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
        .route("/api/opds/sources/:source_id/cover", get(catalog_cover))
        .with_state(service)
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
    if !headers.get_all(header::ACCEPT).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(wire::is_current_media_type) {
        return Err(ApiError { status: StatusCode::NOT_ACCEPTABLE, message: "request requires the current Bokheim binary media type".to_owned() });
    }
    Ok(())
}

fn decode_wire_request<T: serde::de::DeserializeOwned>(headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
    let media_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
    if !media_type.is_some_and(wire::is_current_media_type) {
        return Err(ApiError::bad_request("request requires the current Bokheim binary content type"));
    }
    if headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).is_some_and(|value| !value.eq_ignore_ascii_case("identity")) {
        return Err(ApiError::bad_request("compressed request bodies are not supported"));
    }
    wire::decode(body, wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| ApiError::bad_request(error.to_string()))
}

fn wire_response(value: &impl serde::Serialize, cache_control: &'static str) -> Result<Response, ApiError> {
    let bytes = wire::encode(value).map_err(ApiError::internal)?;
    let mut response = bytes.into_response();
    add_common_headers(&mut response, cache_control);
    Ok(response)
}

fn add_common_headers(response: &mut Response, cache_control: &'static str) {
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::get;
    use image::ImageOutputFormat;
    use opds::{registry_from_endpoints, OpdsEndpointId};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tower::ServiceExt;

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
                crawl_locks: Mutex::new(HashMap::new()),
            }),
        }
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
        image::DynamicImage::new_rgb8(32, 48).write_to(&mut source, ImageOutputFormat::Png).unwrap();
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
    async fn public_registry_is_uncompressed_binary_and_needs_no_authentication() {
        let service = CatalogService::bundled().unwrap();
        let response = app(service).oneshot(Request::builder().uri("/api/opds/catalogs").header(header::ACCEPT, wire::MEDIA_TYPE).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
        let bytes = to_bytes(response.into_body(), wire::MAX_DECODED_RESPONSE_BYTES).await.unwrap();
        let registry: CatalogRegistry = wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
        assert!(!registry.catalogs.is_empty());
    }
}

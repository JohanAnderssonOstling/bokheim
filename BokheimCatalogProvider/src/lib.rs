use bokheim_app_contract::catalog::{
    CatalogAuthentication, CatalogAuthorCredit, CatalogAuthorDiscoveryEvidence, CatalogAuthorDiscoveryMatch, CatalogAuthorDiscoveryRequest, CatalogAuthorDiscoveryResult, CatalogBookFormat, CatalogCapability, CatalogDescription,
    CatalogExternalActionKind, CatalogFacet, CatalogFreshness, CatalogGroup, CatalogImportRequest, CatalogLink, CatalogLinkTarget, CatalogPage, CatalogPagination, CatalogPasswordRequirement, CatalogPrice, CatalogProviderKind,
    CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentifier, CatalogPublicationIdentity, CatalogSearch, CatalogSearchDefinition, CatalogSearchField, CatalogSearchQuery, CatalogSource, CatalogSourceId,
    CatalogWideSearchGroup, CatalogWideSearchOutcome, CatalogWideSearchResult,
};
use bokheim_domain::AuthorName;
use bokheim_opds::{
    CatalogRegistry, OpdsAcquisition, OpdsAcquisitionKind, OpdsBookFormat, OpdsCatalog, OpdsCatalogLink, OpdsDescription, OpdsDirectAcquisition, OpdsEndpoint, OpdsEndpointAuthentication, OpdsEndpointId, OpdsFacet, OpdsGroup,
    OpdsLinkTarget, OpdsPrice, OpdsPublication, OpdsPublicationIdentity, OpdsSearch, OpdsSearchQuery,
};
use futures_util::stream::{self, StreamExt};
use image::io::Reader as ImageReader;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use url::Url;

pub const OVERDRIVE_SOURCE_ID: &str = "overdrive";
const OPDS_SOURCE_PREFIX: &str = "opds-";
const OVERDRIVE_ROOT_LOCATOR: &str = "overdrive:root";
const OVERDRIVE_HOME_URL: &str = "https://www.overdrive.com/";
const LIBBY_HOME_URL: &str = "https://libbyapp.com/";
const MAX_WIDE_SEARCH_CONCURRENCY: usize = 16;
const WIDE_SEARCH_RESULTS_PER_SOURCE: usize = 24;
const WIDE_SEARCH_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CATALOG_SERVER_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const CATALOG_SERVER_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PERSISTENT_CATALOG_SERVER_CACHE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_CACHED_CATALOG_SERVER_RESPONSES: usize = 1024;
const MAX_CATALOG_SERVER_CACHE_BYTES: usize = 64 * 1024 * 1024;
const PERSISTENT_CATALOG_SERVER_CACHE_SCHEMA_VERSION: u32 = 1;
const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_COVER_CACHE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CACHED_CATALOG_COVERS: usize = 2048;
const MAX_KNOWN_CATALOG_COVERS: usize = 16384;
const CATALOG_COVER_WIDTH: u32 = 408;
const CATALOG_COVER_HEIGHT: u32 = 612;
const MAX_CATALOG_COVER_URL_BYTES: usize = 4096;
const MAX_CONCURRENT_CATALOG_COVERS: usize = 4;
const CATALOG_COVER_FAILURE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CACHED_CATALOG_COVER_FAILURES: usize = 512;
const MAX_AUTHOR_DISCOVERY_MATCHES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogProviderError(String);

impl CatalogProviderError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CatalogProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogProviderError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogProviderRoute {
    Opds(OpdsEndpointId),
    Overdrive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogServerSearchRequest {
    pub search: CatalogSearch,
    pub query: CatalogSearchQuery,
}

#[derive(Clone)]
pub struct CatalogServerClient {
    http: reqwest::Client,
    base_url: Url,
    cache: Arc<Mutex<CatalogServerCache>>,
    persistent_cache_path: Option<Arc<PathBuf>>,
    persistent_cache_write: Arc<AsyncMutex<()>>,
}

#[derive(Clone)]
pub struct CatalogCoverCache {
    http: reqwest::Client,
    directory: Arc<PathBuf>,
    known: Arc<Mutex<HashSet<String>>>,
    in_flight: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    downloads: Arc<Semaphore>,
    writes: Arc<AsyncMutex<()>>,
    reference_writes: Arc<AsyncMutex<()>>,
    failures: Arc<Mutex<HashMap<String, CachedCatalogCoverFailure>>>,
    persistent_references: bool,
}

struct CachedCatalogCoverFailure {
    error: CatalogProviderError,
    inserted_at: Instant,
}

#[derive(Default)]
struct CatalogServerCache {
    entries: HashMap<String, CachedCatalogServerResponse>,
    refreshing: HashSet<String>,
    total_bytes: usize,
}

struct CachedCatalogServerResponse {
    response: CatalogServerResponse,
    inserted_at: Instant,
    stored_at_unix_seconds: u64,
    estimated_bytes: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum CatalogServerResponse {
    Page(CatalogPage),
    WideSearch(CatalogWideSearchResult),
    AuthorDiscovery(CatalogAuthorDiscoveryResult),
}

#[derive(Serialize, Deserialize)]
struct PersistentCatalogServerCache {
    schema_version: u32,
    entries: Vec<PersistentCatalogServerCacheEntry>,
}

#[derive(Serialize, Deserialize)]
struct PersistentCatalogServerCacheEntry {
    key: String,
    stored_at_unix_seconds: u64,
    response: CatalogServerResponse,
}

impl CatalogServerClient {
    pub fn new(base_url: &str) -> Result<Self, CatalogProviderError> {
        Self::new_with_cache(base_url, None)
    }

    pub fn with_persistent_cache(base_url: &str, path: PathBuf) -> Result<Self, CatalogProviderError> {
        Self::new_with_cache(base_url, Some(path))
    }

    fn new_with_cache(base_url: &str, path: Option<PathBuf>) -> Result<Self, CatalogProviderError> {
        let base_url = Url::parse(base_url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.username().is_empty() || base_url.password().is_some() || base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(CatalogProviderError::new("catalog server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent(format!("Bokheim/{} catalog-client", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let cache = path.as_deref().and_then(load_persistent_catalog_server_cache).unwrap_or_default();
        Ok(Self { http, base_url, cache: Arc::new(Mutex::new(cache)), persistent_cache_path: path.map(Arc::new), persistent_cache_write: Arc::new(AsyncMutex::new(())) })
    }

    pub async fn registry(&self) -> Result<CatalogRegistry, CatalogProviderError> {
        self.get_wire(self.endpoint("api/opds/catalogs")?).await
    }

    pub async fn catalog_page(&self, source_id: &CatalogSourceId, locator: Option<&str>, fresh: bool) -> Result<CatalogPage, CatalogProviderError> {
        let mut url = self.endpoint(&format!("api/opds/sources/{}/page", source_id.as_str()))?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(locator) = locator {
                query.append_pair("locator", locator);
            }
            if fresh {
                query.append_pair("fresh", "true");
            }
        }
        let cache_key = format!("GET {url}");
        if !fresh {
            if let Some(CatalogServerResponse::Page(page)) = self.cached_response(&cache_key) {
                return Ok(page);
            }
            if let Some(page) = self.stale_page(&cache_key) {
                self.refresh_page_in_background(url, cache_key);
                return Ok(page);
            }
        }
        let page: CatalogPage = self.get_wire(url).await?;
        self.cache_response(cache_key, CatalogServerResponse::Page(page.clone())).await;
        Ok(page)
    }

    pub async fn search_catalogs(&self, query: &str) -> Result<CatalogWideSearchResult, CatalogProviderError> {
        let mut url = self.endpoint("api/opds/search")?;
        url.query_pairs_mut().append_pair("q", query);
        let cache_key = format!("GET {url}");
        if let Some(CatalogServerResponse::WideSearch(result)) = self.cached_response(&cache_key) {
            return Ok(result);
        }
        if let Some(result) = self.stale_wide_search(&cache_key) {
            self.refresh_wide_search_in_background(url, cache_key);
            return Ok(result);
        }
        let result: CatalogWideSearchResult = self.get_wire(url).await?;
        self.cache_response(cache_key, CatalogServerResponse::WideSearch(result.clone())).await;
        Ok(result)
    }

    pub async fn discover_author_books(&self, request: &CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
        if let Some((result, true)) = self.cached_author_discovery(request)? {
            return Ok(result);
        }
        self.refresh_author_books(request).await
    }

    pub async fn refresh_author_books(&self, request: &CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
        let (url, cache_key) = self.author_discovery_cache_key(request)?;
        let result: CatalogAuthorDiscoveryResult = self.post_wire(url, request).await?;
        self.cache_response(cache_key, CatalogServerResponse::AuthorDiscovery(result.clone())).await;
        Ok(result)
    }

    pub fn cached_author_discovery(&self, request: &CatalogAuthorDiscoveryRequest) -> Result<Option<(CatalogAuthorDiscoveryResult, bool)>, CatalogProviderError> {
        let (_, cache_key) = self.author_discovery_cache_key(request)?;
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = cache.entries.get(&cache_key) else { return Ok(None) };
        let CatalogServerResponse::AuthorDiscovery(result) = &entry.response else { return Ok(None) };
        Ok(Some((result.clone(), entry.inserted_at.elapsed() <= CATALOG_SERVER_CACHE_TTL)))
    }

    pub async fn cache_author_discovery(&self, request: &CatalogAuthorDiscoveryRequest, result: &CatalogAuthorDiscoveryResult) -> Result<(), CatalogProviderError> {
        let (_, cache_key) = self.author_discovery_cache_key(request)?;
        self.cache_response(cache_key, CatalogServerResponse::AuthorDiscovery(result.clone())).await;
        Ok(())
    }

    fn author_discovery_cache_key(&self, request: &CatalogAuthorDiscoveryRequest) -> Result<(Url, String), CatalogProviderError> {
        let url = self.endpoint("api/opds/authors/discover")?;
        let request_key = bokheim_wire::encode(request).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let cache_key = format!("POST {url} {}", blake3::hash(&request_key).to_hex());
        Ok((url, cache_key))
    }

    pub async fn search_catalog(&self, source_id: &CatalogSourceId, search: CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPage, CatalogProviderError> {
        let url = self.endpoint(&format!("api/opds/sources/{}/search", source_id.as_str()))?;
        let request = CatalogServerSearchRequest { search, query };
        let request_key = bokheim_wire::encode(&request).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let cache_key = format!("POST {url} {}", blake3::hash(&request_key).to_hex());
        if let Some(CatalogServerResponse::Page(page)) = self.cached_response(&cache_key) {
            return Ok(page);
        }
        if let Some(page) = self.stale_page(&cache_key) {
            self.refresh_search_page_in_background(url, request, cache_key);
            return Ok(page);
        }
        let page: CatalogPage = self.post_wire(url, &request).await?;
        self.cache_response(cache_key, CatalogServerResponse::Page(page.clone())).await;
        Ok(page)
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        let mut url = self.endpoint(&format!("api/opds/sources/{}/cover", source_id.as_str()))?;
        url.query_pairs_mut().append_pair("image_url", image_url);
        let response = self.http.get(url).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogProviderError::new(format!("catalog server returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_COVER_SOURCE_BYTES) {
            return Err(CatalogProviderError::new("catalog server cover is too large"));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next());
        if media_type != Some("image/jpeg") {
            return Err(CatalogProviderError::new("catalog server cover is not a JPEG"));
        }
        let bytes = response.bytes().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_CATALOG_COVER_SOURCE_BYTES {
            return Err(CatalogProviderError::new("catalog server cover is empty or too large"));
        }
        tokio::task::spawn_blocking(move || validate_server_cover(bytes.to_vec())).await.map_err(|error| CatalogProviderError::new(error.to_string()))?
    }

    fn endpoint(&self, path: &str) -> Result<Url, CatalogProviderError> {
        self.base_url.join(path).map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    async fn get_wire<T: serde::de::DeserializeOwned>(&self, url: Url) -> Result<T, CatalogProviderError> {
        let response = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, bokheim_wire::MEDIA_TYPE)
            .header(reqwest::header::ACCEPT_ENCODING, bokheim_wire::CONTENT_ENCODING_ZSTD)
            .send()
            .await
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn post_wire<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(&self, url: Url, value: &T) -> Result<R, CatalogProviderError> {
        let encoded = bokheim_wire::encode_transport(value, true).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let mut request = self
            .http
            .post(url)
            .header(reqwest::header::ACCEPT, bokheim_wire::MEDIA_TYPE)
            .header(reqwest::header::ACCEPT_ENCODING, bokheim_wire::CONTENT_ENCODING_ZSTD)
            .header(reqwest::header::CONTENT_TYPE, bokheim_wire::MEDIA_TYPE)
            .body(encoded.bytes);
        if encoded.encoding == bokheim_wire::ContentEncoding::Zstd {
            request = request.header(reqwest::header::CONTENT_ENCODING, bokheim_wire::CONTENT_ENCODING_ZSTD);
        }
        let response = request.send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn response_wire<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, CatalogProviderError> {
        if !response.status().is_success() {
            let status = response.status();
            let detail = if response.content_length().is_some_and(|length| length <= 4096) { response.bytes().await.ok().and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()).filter(|value| !value.trim().is_empty()) } else { None };
            return Err(CatalogProviderError::new(match detail {
                Some(detail) => format!("catalog server returned HTTP {status}: {detail}"),
                None => format!("catalog server returned HTTP {status}"),
            }));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        if !media_type.is_some_and(bokheim_wire::is_current_media_type) {
            return Err(CatalogProviderError::new("catalog server returned an unsupported media type"));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_SERVER_RESPONSE_BYTES) {
            return Err(CatalogProviderError::new("catalog server response is too large"));
        }
        let encoding = match response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()) {
            None | Some("identity") => bokheim_wire::ContentEncoding::Identity,
            Some(value) if value.eq_ignore_ascii_case(bokheim_wire::CONTENT_ENCODING_ZSTD) => bokheim_wire::ContentEncoding::Zstd,
            Some(_) => return Err(CatalogProviderError::new("catalog server returned an unsupported content encoding")),
        };
        let bytes = response.bytes().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        bokheim_wire::decode_transport(&bytes, encoding, bokheim_wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    fn cached_response(&self, cache_key: &str) -> Option<CatalogServerResponse> {
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.entries.get(cache_key).filter(|entry| entry.inserted_at.elapsed() <= CATALOG_SERVER_CACHE_TTL).map(|entry| entry.response.clone())
    }

    fn stale_page(&self, cache_key: &str) -> Option<CatalogPage> {
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let CatalogServerResponse::Page(mut page) = cache.entries.get(cache_key)?.response.clone() else { return None };
        page.freshness = CatalogFreshness::Stale;
        Some(page)
    }

    fn stale_wide_search(&self, cache_key: &str) -> Option<CatalogWideSearchResult> {
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let CatalogServerResponse::WideSearch(mut result) = cache.entries.get(cache_key)?.response.clone() else { return None };
        for group in &mut result.groups {
            if let CatalogWideSearchOutcome::Results { page } = &mut group.outcome {
                page.freshness = CatalogFreshness::Stale;
            }
        }
        Some(result)
    }

    fn begin_refresh(&self, cache_key: &str) -> bool {
        self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).refreshing.insert(cache_key.to_owned())
    }

    fn finish_refresh(&self, cache_key: &str) {
        self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).refreshing.remove(cache_key);
    }

    fn refresh_page_in_background(&self, url: Url, cache_key: String) {
        if !self.begin_refresh(&cache_key) {
            return;
        }
        let client = self.clone();
        tokio::spawn(async move {
            if let Ok(page) = client.get_wire::<CatalogPage>(url).await {
                client.cache_response(cache_key.clone(), CatalogServerResponse::Page(page)).await;
            }
            client.finish_refresh(&cache_key);
        });
    }

    fn refresh_wide_search_in_background(&self, url: Url, cache_key: String) {
        if !self.begin_refresh(&cache_key) {
            return;
        }
        let client = self.clone();
        tokio::spawn(async move {
            if let Ok(result) = client.get_wire::<CatalogWideSearchResult>(url).await {
                client.cache_response(cache_key.clone(), CatalogServerResponse::WideSearch(result)).await;
            }
            client.finish_refresh(&cache_key);
        });
    }

    fn refresh_search_page_in_background(&self, url: Url, request: CatalogServerSearchRequest, cache_key: String) {
        if !self.begin_refresh(&cache_key) {
            return;
        }
        let client = self.clone();
        tokio::spawn(async move {
            if let Ok(page) = client.post_wire::<_, CatalogPage>(url, &request).await {
                client.cache_response(cache_key.clone(), CatalogServerResponse::Page(page)).await;
            }
            client.finish_refresh(&cache_key);
        });
    }

    async fn cache_response(&self, cache_key: String, response: CatalogServerResponse) {
        let estimated_bytes = serde_json::to_vec(&response).map(|bytes| bytes.len()).unwrap_or(MAX_CATALOG_SERVER_CACHE_BYTES);
        if estimated_bytes > MAX_CATALOG_SERVER_CACHE_BYTES {
            return;
        }
        let stored_at_unix_seconds = unix_seconds_now();
        {
            let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(previous) = cache.entries.remove(&cache_key) {
                cache.total_bytes = cache.total_bytes.saturating_sub(previous.estimated_bytes);
            }
            while cache.entries.len() >= MAX_CACHED_CATALOG_SERVER_RESPONSES || cache.total_bytes.saturating_add(estimated_bytes) > MAX_CATALOG_SERVER_CACHE_BYTES {
                let Some(oldest_key) = cache.entries.iter().min_by_key(|(_, entry)| entry.inserted_at).map(|(key, _)| key.clone()) else { break };
                if let Some(entry) = cache.entries.remove(&oldest_key) {
                    cache.total_bytes = cache.total_bytes.saturating_sub(entry.estimated_bytes);
                }
            }
            cache.total_bytes = cache.total_bytes.saturating_add(estimated_bytes);
            cache.entries.insert(cache_key, CachedCatalogServerResponse { response, inserted_at: Instant::now(), stored_at_unix_seconds, estimated_bytes });
        }
        self.persist_cache().await;
    }

    async fn persist_cache(&self) {
        let Some(path) = self.persistent_cache_path.as_deref() else { return };
        let _write = self.persistent_cache_write.lock().await;
        let persistent = {
            let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            PersistentCatalogServerCache {
                schema_version: PERSISTENT_CATALOG_SERVER_CACHE_SCHEMA_VERSION,
                entries: cache.entries.iter().map(|(key, entry)| PersistentCatalogServerCacheEntry { key: key.clone(), stored_at_unix_seconds: entry.stored_at_unix_seconds, response: entry.response.clone() }).collect(),
            }
        };
        let Ok(bytes) = serde_json::to_vec(&persistent) else { return };
        if bytes.len() > MAX_CATALOG_SERVER_CACHE_BYTES {
            return;
        }
        let Some(parent) = path.parent() else { return };
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return;
        }
        let temporary = parent.join(format!(".catalog-server-pages-{}.tmp", uuid::Uuid::new_v4()));
        let result = async {
            let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).await?;
            file.write_all(&bytes).await?;
            file.flush().await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary, path).await
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(temporary).await;
        }
    }
}

impl CatalogCoverCache {
    pub fn new(directory: PathBuf) -> Result<Self, CatalogProviderError> {
        Self::new_with_reference_persistence(directory, false)
    }

    pub fn new_persistent(directory: PathBuf) -> Result<Self, CatalogProviderError> {
        Self::new_with_reference_persistence(directory, true)
    }

    fn new_with_reference_persistence(directory: PathBuf, persistent_references: bool) -> Result<Self, CatalogProviderError> {
        std::fs::create_dir_all(&directory).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let directory = directory.canonicalize().map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let known = if persistent_references { load_known_cover_references(&directory) } else { HashSet::new() };
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(format!("Bokheim/{} catalog-cover-client", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Ok(Self {
            http,
            directory: Arc::new(directory),
            known: Arc::new(Mutex::new(known)),
            in_flight: Arc::new(AsyncMutex::new(HashMap::new())),
            downloads: Arc::new(Semaphore::new(MAX_CONCURRENT_CATALOG_COVERS)),
            writes: Arc::new(AsyncMutex::new(())),
            reference_writes: Arc::new(AsyncMutex::new(())),
            failures: Arc::new(Mutex::new(HashMap::new())),
            persistent_references,
        })
    }

    pub async fn remember_page(&self, page: &CatalogPage) {
        let references =
            page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter_map(|publication| publication.image_url.as_ref().map(|image_url| (page.source_id.clone(), image_url.clone()))).collect();
        self.remember_cover_urls(references).await;
    }

    pub async fn remember_author_discovery(&self, result: &CatalogAuthorDiscoveryResult) {
        let references = result.matches.iter().filter_map(|result| result.publication.image_url.as_ref().map(|image_url| (result.source.id.clone(), image_url.clone()))).collect();
        self.remember_cover_urls(references).await;
    }

    async fn remember_cover_urls(&self, references: Vec<(CatalogSourceId, String)>) {
        let _reference_write = if self.persistent_references { Some(self.reference_writes.lock().await) } else { None };
        let (added, removed) = {
            let mut known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut added = Vec::new();
            let mut removed = Vec::new();
            for (source_id, image_url) in references {
                if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
                    continue;
                }
                let cache_key = cover_cache_key(&source_id, &image_url);
                if known.len() >= MAX_KNOWN_CATALOG_COVERS && !known.contains(&cache_key) {
                    if let Some(oldest) = known.iter().next().cloned() {
                        known.remove(&oldest);
                        removed.push(oldest);
                    }
                }
                if known.insert(cache_key.clone()) {
                    added.push(cache_key);
                }
            }
            (added, removed)
        };
        if !self.persistent_references {
            return;
        }
        for cache_key in removed {
            let _ = tokio::fs::remove_file(self.directory.join(format!("{cache_key}.ref"))).await;
        }
        for cache_key in added {
            let _ = tokio::fs::OpenOptions::new().write(true).create_new(true).open(self.directory.join(format!("{cache_key}.ref"))).await;
        }
    }

    pub async fn remember_wide_search(&self, result: &CatalogWideSearchResult) {
        for group in &result.groups {
            if let CatalogWideSearchOutcome::Results { page } = &group.outcome {
                self.remember_page(page).await;
            }
        }
    }

    pub async fn cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        self.cover_with(source_id, image_url, self.fetch_cover(image_url)).await
    }

    pub async fn cover_from_server(&self, server: &CatalogServerClient, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        self.cover_with(source_id, image_url, server.catalog_cover(source_id, image_url)).await
    }

    async fn cover_with(&self, source_id: &CatalogSourceId, image_url: &str, fetch: impl Future<Output = Result<Vec<u8>, CatalogProviderError>>) -> Result<Vec<u8>, CatalogProviderError> {
        if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
            return Err(CatalogProviderError::new("catalog cover URL is too long"));
        }
        let cache_key = cover_cache_key(source_id, image_url);
        let path = self.directory.join(format!("{cache_key}.jpg"));
        if let Some(bytes) = read_cached_cover(&path).await {
            return Ok(bytes);
        }
        if !self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&cache_key) {
            return Err(CatalogProviderError::new("catalog cover URL was not present in a returned catalog page"));
        }
        if let Some(error) = self.cached_failure(&cache_key) {
            return Err(error);
        }
        let lock = {
            let mut in_flight = self.in_flight.lock().await;
            in_flight.entry(cache_key.clone()).or_insert_with(|| Arc::new(AsyncMutex::new(()))).clone()
        };
        let _fetch = lock.lock().await;
        if let Some(bytes) = read_cached_cover(&path).await {
            self.in_flight.lock().await.remove(&cache_key);
            return Ok(bytes);
        }
        if let Some(error) = self.cached_failure(&cache_key) {
            self.in_flight.lock().await.remove(&cache_key);
            return Err(error);
        }
        let _download = match self.downloads.acquire().await {
            Ok(download) => download,
            Err(error) => {
                self.in_flight.lock().await.remove(&cache_key);
                return Err(CatalogProviderError::new(error.to_string()));
            }
        };
        let result = fetch.await;
        if let Ok(bytes) = &result {
            self.publish_cover(&path, bytes).await;
            self.failures.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&cache_key);
        } else if let Err(error) = &result {
            self.cache_failure(cache_key.clone(), error.clone());
        }
        self.in_flight.lock().await.remove(&cache_key);
        result
    }

    fn cached_failure(&self, cache_key: &str) -> Option<CatalogProviderError> {
        let mut failures = self.failures.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let failure = failures.get(cache_key)?;
        if failure.inserted_at.elapsed() <= CATALOG_COVER_FAILURE_TTL {
            return Some(failure.error.clone());
        }
        failures.remove(cache_key);
        None
    }

    fn cache_failure(&self, cache_key: String, error: CatalogProviderError) {
        let mut failures = self.failures.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while failures.len() >= MAX_CACHED_CATALOG_COVER_FAILURES {
            let Some(oldest) = failures.iter().min_by_key(|(_, failure)| failure.inserted_at).map(|(key, _)| key.clone()) else { break };
            failures.remove(&oldest);
        }
        failures.insert(cache_key, CachedCatalogCoverFailure { error, inserted_at: Instant::now() });
    }

    async fn fetch_cover(&self, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        let url = Url::parse(image_url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err(CatalogProviderError::new("catalog cover URL must be absolute credential-free HTTP(S)"));
        }
        let response = self.http.get(url).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogProviderError::new(format!("catalog cover returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_COVER_SOURCE_BYTES) {
            return Err(CatalogProviderError::new("catalog cover is too large"));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).unwrap_or("");
        if media_type == "image/svg+xml" || !(media_type.is_empty() || media_type.starts_with("image/") || media_type == "application/octet-stream") {
            return Err(CatalogProviderError::new("catalog cover has an unsupported media type"));
        }
        let mut source = Vec::with_capacity(response.content_length().unwrap_or(0).min(MAX_CATALOG_COVER_SOURCE_BYTES) as usize);
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|error| CatalogProviderError::new(error.to_string()))?;
            if source.len().saturating_add(chunk.len()) as u64 > MAX_CATALOG_COVER_SOURCE_BYTES {
                return Err(CatalogProviderError::new("catalog cover is too large"));
            }
            source.extend_from_slice(&chunk);
        }
        if source.is_empty() {
            return Err(CatalogProviderError::new("catalog cover is empty or too large"));
        }
        tokio::task::spawn_blocking(move || normalize_cover(&source)).await.map_err(|error| CatalogProviderError::new(error.to_string()))?
    }

    async fn publish_cover(&self, path: &Path, bytes: &[u8]) {
        let _write = self.writes.lock().await;
        let temporary = self.directory.join(format!(".catalog-cover-{}.tmp", uuid::Uuid::new_v4()));
        let result = async {
            let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).await?;
            file.write_all(bytes).await?;
            file.flush().await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary, path).await
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
            return;
        }
        prune_cover_cache(&self.directory).await;
    }
}

fn cover_cache_key(source_id: &CatalogSourceId, image_url: &str) -> String {
    blake3::hash(format!("{}\n{image_url}", source_id.as_str()).as_bytes()).to_hex().to_string()
}

fn load_known_cover_references(directory: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(directory) else { return HashSet::new() };
    let mut references = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let key = name.strip_suffix(".ref")?;
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().unwrap_or(UNIX_EPOCH);
            Some((modified, key.to_owned(), entry.path()))
        })
        .collect::<Vec<_>>();
    references.sort_by_key(|(modified, _, _)| std::cmp::Reverse(*modified));
    for (_, _, path) in references.iter().skip(MAX_KNOWN_CATALOG_COVERS) {
        let _ = std::fs::remove_file(path);
    }
    references.into_iter().take(MAX_KNOWN_CATALOG_COVERS).map(|(_, key, _)| key).collect()
}

fn normalize_cover(bytes: &[u8]) -> Result<Vec<u8>, CatalogProviderError> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| CatalogProviderError::new(format!("invalid catalog cover: {error}")))?;
    let thumbnail = image.thumbnail(CATALOG_COVER_WIDTH, CATALOG_COVER_HEIGHT).to_rgb8();
    let mut normalized = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(thumbnail).write_to(&mut normalized, image::ImageOutputFormat::Jpeg(85)).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    Ok(normalized.into_inner())
}

fn validate_server_cover(bytes: Vec<u8>) -> Result<Vec<u8>, CatalogProviderError> {
    let mut reader = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if reader.format() != Some(image::ImageFormat::Jpeg) {
        return Err(CatalogProviderError::new("catalog server cover body is not a JPEG"));
    }
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(CATALOG_COVER_WIDTH);
    limits.max_image_height = Some(CATALOG_COVER_HEIGHT);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| CatalogProviderError::new(format!("invalid catalog server cover: {error}")))?;
    if image.width() == 0 || image.height() == 0 {
        return Err(CatalogProviderError::new("catalog server cover has invalid dimensions"));
    }
    Ok(bytes)
}

async fn read_cached_cover(path: &Path) -> Option<Vec<u8>> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CATALOG_COVER_SOURCE_BYTES {
        return None;
    }
    tokio::fs::read(path).await.ok()
}

async fn prune_cover_cache(directory: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else { return };
    let mut covers = Vec::new();
    let mut total_bytes = 0_u64;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("jpg") {
            continue;
        }
        let Ok(metadata) = entry.metadata().await else { continue };
        if !metadata.is_file() {
            continue;
        }
        total_bytes = total_bytes.saturating_add(metadata.len());
        covers.push((metadata.modified().unwrap_or(UNIX_EPOCH), metadata.len(), path));
    }
    covers.sort_by_key(|(modified, _, _)| *modified);
    while covers.len() > MAX_CACHED_CATALOG_COVERS || total_bytes > MAX_CATALOG_COVER_CACHE_BYTES {
        let (_, bytes, path) = covers.remove(0);
        if tokio::fs::remove_file(path).await.is_ok() {
            total_bytes = total_bytes.saturating_sub(bytes);
        }
    }
}

fn unix_seconds_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn load_persistent_catalog_server_cache(path: &Path) -> Option<CatalogServerCache> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > MAX_CATALOG_SERVER_CACHE_BYTES {
        return None;
    }
    let persistent: PersistentCatalogServerCache = serde_json::from_slice(&bytes).ok()?;
    if persistent.schema_version != PERSISTENT_CATALOG_SERVER_CACHE_SCHEMA_VERSION || persistent.entries.len() > MAX_CACHED_CATALOG_SERVER_RESPONSES {
        return None;
    }
    let now_unix_seconds = unix_seconds_now();
    let mut cache = CatalogServerCache::default();
    for entry in persistent.entries {
        let age = now_unix_seconds.saturating_sub(entry.stored_at_unix_seconds);
        if entry.stored_at_unix_seconds > now_unix_seconds.saturating_add(5 * 60) || age > MAX_PERSISTENT_CATALOG_SERVER_CACHE_AGE.as_secs() {
            continue;
        }
        let estimated_bytes = serde_json::to_vec(&entry.response).map(|bytes| bytes.len()).ok()?;
        if estimated_bytes > MAX_CATALOG_SERVER_CACHE_BYTES || cache.total_bytes.saturating_add(estimated_bytes) > MAX_CATALOG_SERVER_CACHE_BYTES {
            continue;
        }
        let inserted_at = Instant::now().checked_sub(Duration::from_secs(age)).unwrap_or_else(Instant::now);
        cache.total_bytes = cache.total_bytes.saturating_add(estimated_bytes);
        cache.entries.insert(entry.key, CachedCatalogServerResponse { response: entry.response, inserted_at, stored_at_unix_seconds: entry.stored_at_unix_seconds, estimated_bytes });
    }
    Some(cache)
}

pub fn catalog_sources(endpoints: impl IntoIterator<Item = OpdsEndpoint>) -> Vec<CatalogSource> {
    endpoints.into_iter().map(opds_source).chain(std::iter::once(overdrive_source())).collect()
}

pub async fn search_all_catalogs(client: &bokheim_opds::OpdsClient, endpoints: Vec<OpdsEndpoint>, query: &str) -> Result<CatalogWideSearchResult, CatalogProviderError> {
    let mut result = CatalogWideSearchResult { query: query.trim().to_owned(), groups: search_opds_catalogs(client, endpoints, query).await? };
    let source = overdrive_source();
    let page = overdrive_search_page(&overdrive_page(None).expect("OverDrive root is valid").search.expect("OverDrive advertises search"), &CatalogSearchQuery { query: result.query.clone(), title: None, author: None })?;
    result.groups.push(CatalogWideSearchGroup { source, outcome: CatalogWideSearchOutcome::Results { page } });
    Ok(result)
}

pub async fn discover_author_books_locally(client: &bokheim_opds::OpdsClient, endpoints: Vec<OpdsEndpoint>, request: &CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
    validate_author_discovery_request(request)?;
    let identifier_searches = stream::iter(request.book_identifiers.iter().cloned())
        .map(|identifier| {
            let endpoints = endpoints.clone();
            async move { search_all_catalogs(client, endpoints, &identifier.value).await.map(|result| (identifier, result)) }
        })
        .buffer_unordered(request.book_identifiers.len().max(1))
        .collect::<Vec<_>>();
    let name_search = search_all_catalogs(client, endpoints.clone(), &request.author_name);
    let (identifier_searches, name_search) = tokio::join!(identifier_searches, name_search);
    let identifier_searches = identifier_searches.into_iter().collect::<Result<Vec<_>, _>>()?;
    let name_search = name_search?;
    let identifier_searches = identifier_searches.iter().map(|(identifier, result)| (identifier, result)).collect::<Vec<_>>();
    resolve_author_discovery(request, &identifier_searches, &name_search)
}

pub fn resolve_author_discovery(
    request: &CatalogAuthorDiscoveryRequest, identifier_searches: &[(&CatalogPublicationIdentifier, &CatalogWideSearchResult)], name_search: &CatalogWideSearchResult,
) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
    validate_author_discovery_request(request)?;
    let author_name = AuthorName::parse(&request.author_name).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let author_key = author_name.match_key().as_str().to_owned();
    let mut resolved = HashMap::<CatalogSourceId, ResolvedCatalogAuthor>::new();

    for &(identifier, search) in identifier_searches {
        if !request.book_identifiers.contains(identifier) {
            continue;
        }
        for group in &search.groups {
            let CatalogWideSearchOutcome::Results { page } = &group.outcome else { continue };
            for publication in page_publications(page).filter(|publication| publication_matches_identifier(publication, identifier)) {
                let matching_authors = publication.authors.iter().filter(|author| author_name_key(&author.name).as_deref() == Some(author_key.as_str())).collect::<Vec<_>>();
                if matching_authors.is_empty() {
                    continue;
                }
                let entry = resolved.entry(group.source.id.clone()).or_default();
                if !entry.identifiers.contains(identifier) {
                    entry.identifiers.push(identifier.clone());
                }
                for author in matching_authors {
                    entry.author_keys.extend(catalog_author_keys(author));
                }
            }
        }
    }

    let mut matches = Vec::<CatalogAuthorDiscoveryMatch>::new();
    let mut match_indices = HashMap::<String, usize>::new();
    for group in &name_search.groups {
        let CatalogWideSearchOutcome::Results { page } = &group.outcome else { continue };
        for publication in page_publications(page) {
            if request.book_identifiers.iter().any(|identifier| publication_matches_identifier(publication, identifier)) {
                continue;
            }
            let matching_authors = publication.authors.iter().filter(|author| author_name_key(&author.name).as_deref() == Some(author_key.as_str())).collect::<Vec<_>>();
            if matching_authors.is_empty() {
                continue;
            }
            let evidence = resolved
                .get(&group.source.id)
                .and_then(|resolved_author| catalog_identifier_evidence(resolved_author, &matching_authors))
                .map(|identifier| CatalogAuthorDiscoveryEvidence::BookIdentifier { identifier })
                .unwrap_or(CatalogAuthorDiscoveryEvidence::AuthorName);
            insert_author_match(&mut matches, &mut match_indices, group.source.clone(), publication.clone(), evidence);
        }
    }

    matches.sort_by_key(|result| match &result.evidence {
        CatalogAuthorDiscoveryEvidence::BookIdentifier { .. } => 0_u8,
        CatalogAuthorDiscoveryEvidence::AuthorName => 1_u8,
    });
    matches.truncate(MAX_AUTHOR_DISCOVERY_MATCHES);
    Ok(CatalogAuthorDiscoveryResult { author_name: author_name.as_str().to_owned(), matches })
}

fn validate_author_discovery_request(request: &CatalogAuthorDiscoveryRequest) -> Result<(), CatalogProviderError> {
    AuthorName::parse(&request.author_name).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if request.book_identifiers.len() > 8 {
        return Err(CatalogProviderError::new("author discovery accepts at most eight book identifiers"));
    }
    if request.book_identifiers.iter().any(|identifier| !identifier.is_valid_search_identifier()) {
        return Err(CatalogProviderError::new("author discovery contains an invalid book identifier"));
    }
    Ok(())
}

#[derive(Default)]
struct ResolvedCatalogAuthor {
    author_keys: HashSet<String>,
    identifiers: Vec<CatalogPublicationIdentifier>,
}

fn page_publications(page: &CatalogPage) -> impl Iterator<Item = &CatalogPublication> {
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter()))
}

fn publication_matches_identifier(publication: &CatalogPublication, identifier: &CatalogPublicationIdentifier) -> bool {
    let Some(expected) = identifier.normalized_value() else { return false };
    [Some(publication.identity.edition_id.as_str()), publication.identity.work_id.as_deref()]
        .into_iter()
        .flatten()
        .any(|value| CatalogPublicationIdentifier { scheme: identifier.scheme, value: value.to_owned() }.normalized_value().as_deref() == Some(expected.as_str()))
}

fn author_name_key(value: &str) -> Option<String> {
    AuthorName::parse(value).ok().map(|name| name.match_key().as_str().to_owned())
}

fn catalog_author_keys(author: &CatalogAuthorCredit) -> HashSet<String> {
    let identifiers = author.identifiers.iter().filter_map(|identifier| {
        let identifier = identifier.trim();
        (!identifier.is_empty()).then(|| format!("identifier:{identifier}"))
    });
    let provider_link = author.provider_link.as_ref().map(|target| match target {
        CatalogLinkTarget::Page { locator } => format!("provider:{locator}"),
        CatalogLinkTarget::External { url } => format!("provider:{url}"),
    });
    identifiers.chain(provider_link).collect()
}

fn catalog_identifier_evidence(resolved_author: &ResolvedCatalogAuthor, matching_authors: &[&CatalogAuthorCredit]) -> Option<CatalogPublicationIdentifier> {
    if resolved_author.author_keys.is_empty() {
        return None;
    }
    let candidate_keys = matching_authors.iter().flat_map(|author| catalog_author_keys(author)).collect::<HashSet<_>>();
    candidate_keys.iter().any(|key| resolved_author.author_keys.contains(key)).then(|| resolved_author.identifiers.first().cloned()).flatten()
}

fn insert_author_match(matches: &mut Vec<CatalogAuthorDiscoveryMatch>, indices: &mut HashMap<String, usize>, source: CatalogSource, publication: CatalogPublication, evidence: CatalogAuthorDiscoveryEvidence) {
    let key = format!("{}\n{}\n{}", source.id.as_str(), publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or(""));
    if let Some(index) = indices.get(&key).copied() {
        let existing = &mut matches[index].evidence;
        if matches!(existing, CatalogAuthorDiscoveryEvidence::AuthorName) && matches!(evidence, CatalogAuthorDiscoveryEvidence::BookIdentifier { .. }) {
            *existing = evidence;
        }
        return;
    }
    indices.insert(key, matches.len());
    matches.push(CatalogAuthorDiscoveryMatch { source, publication, evidence });
}

pub async fn search_opds_catalogs(client: &bokheim_opds::OpdsClient, endpoints: Vec<OpdsEndpoint>, query: &str) -> Result<Vec<CatalogWideSearchGroup>, CatalogProviderError> {
    let query = query.trim();
    let search_query = CatalogSearchQuery { query: query.to_owned(), title: None, author: None };
    let terms = search_terms(&search_query)?;
    let plans = endpoints.into_iter().map(|endpoint| (opds_source(endpoint.clone()), endpoint)).collect::<Vec<_>>();
    let concurrency = wide_search_concurrency(plans.len());
    let mut groups = stream::iter(plans.into_iter().enumerate().map(|(index, (source, endpoint))| {
        let terms = terms.clone();
        async move {
            let outcome = search_opds_catalog(client, &source, &endpoint, &terms).await;
            (index, CatalogWideSearchGroup { source, outcome })
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;
    groups.sort_by_key(|(index, _)| *index);
    Ok(groups.into_iter().map(|(_, group)| group).collect())
}

fn wide_search_concurrency(source_count: usize) -> usize {
    source_count.clamp(1, MAX_WIDE_SEARCH_CONCURRENCY)
}

async fn search_opds_catalog(client: &bokheim_opds::OpdsClient, source: &CatalogSource, endpoint: &OpdsEndpoint, terms: &str) -> CatalogWideSearchOutcome {
    if matches!(endpoint.authentication, OpdsEndpointAuthentication::Basic { .. }) {
        return CatalogWideSearchOutcome::AuthenticationRequired;
    }
    let search = async {
        let root = client.fetch_catalog(&endpoint.url, None).await.map_err(|error| error.to_string())?;
        validate_opds_catalog_locator(endpoint, &root.source_url).map_err(|error| error.to_string())?;
        let Some(definition) = root.search else { return Ok(None) };
        let search_origin = match &definition {
            OpdsSearch::UriTemplate { base_url, .. } => base_url,
            OpdsSearch::OpenSearchDescription { url } => url,
        };
        validate_opds_catalog_locator(endpoint, search_origin).map_err(|error| error.to_string())?;
        let fields = opds_search(definition.clone()).fields;
        let query = if fields.contains(&CatalogSearchField::Query) {
            OpdsSearchQuery { query: terms.to_owned(), title: None, author: None }
        } else if fields.contains(&CatalogSearchField::Title) {
            OpdsSearchQuery { query: String::new(), title: Some(terms.to_owned()), author: None }
        } else if fields.contains(&CatalogSearchField::Author) {
            OpdsSearchQuery { query: String::new(), title: None, author: Some(terms.to_owned()) }
        } else {
            return Ok(None);
        };
        let catalog = client.search_definition(&definition, &query, None).await.map_err(|error| error.to_string())?;
        validate_opds_catalog_locator(endpoint, &catalog.source_url).map_err(|error| error.to_string())?;
        Ok::<_, String>(Some(limit_wide_search_page(opds_page(source.id.clone(), catalog))))
    };
    match tokio::time::timeout(WIDE_SEARCH_TIMEOUT, search).await {
        Ok(Ok(Some(page))) => CatalogWideSearchOutcome::Results { page },
        Ok(Ok(None)) => CatalogWideSearchOutcome::Unsupported,
        Ok(Err(message)) => CatalogWideSearchOutcome::Failed { message },
        Err(_) => CatalogWideSearchOutcome::TimedOut,
    }
}

fn limit_wide_search_page(mut page: CatalogPage) -> CatalogPage {
    let mut remaining = WIDE_SEARCH_RESULTS_PER_SOURCE;
    page.publications.truncate(remaining);
    remaining = remaining.saturating_sub(page.publications.len());
    for group in &mut page.groups {
        group.publications.truncate(remaining);
        remaining = remaining.saturating_sub(group.publications.len());
    }
    page
}

pub fn opds_source(endpoint: OpdsEndpoint) -> CatalogSource {
    let authentication = match endpoint.authentication {
        OpdsEndpointAuthentication::None => CatalogAuthentication::None,
        OpdsEndpointAuthentication::Basic { username_label, password } => CatalogAuthentication::Basic {
            username_label,
            password: match password {
                bokheim_opds::OpdsPasswordRequirement::Required => CatalogPasswordRequirement::Required,
                bokheim_opds::OpdsPasswordRequirement::Empty => CatalogPasswordRequirement::Empty,
            },
        },
    };
    CatalogSource {
        id: opds_catalog_source_id(&endpoint.id),
        provider: CatalogProviderKind::Opds,
        title: endpoint.title,
        description: endpoint.description,
        languages: endpoint.languages,
        authentication,
        capabilities: vec![CatalogCapability::Browse, CatalogCapability::Search, CatalogCapability::Facets, CatalogCapability::Import, CatalogCapability::ExternalFulfillment],
    }
}

pub fn opds_catalog_source_id(endpoint_id: &OpdsEndpointId) -> CatalogSourceId {
    CatalogSourceId::parse(&format!("{OPDS_SOURCE_PREFIX}{}", endpoint_id.as_str())).expect("namespaced OPDS endpoint id is a catalog source id")
}

pub fn catalog_provider_route(source_id: &CatalogSourceId) -> Result<CatalogProviderRoute, CatalogProviderError> {
    if source_id.as_str() == OVERDRIVE_SOURCE_ID {
        return Ok(CatalogProviderRoute::Overdrive);
    }
    let value = source_id.as_str().strip_prefix(OPDS_SOURCE_PREFIX).ok_or_else(|| CatalogProviderError::new("catalog source does not belong to a registered provider"))?;
    OpdsEndpointId::parse(value).map(CatalogProviderRoute::Opds).map_err(|error| CatalogProviderError::new(error.to_string()))
}

pub fn opds_endpoint_id(source_id: &CatalogSourceId) -> Result<OpdsEndpointId, CatalogProviderError> {
    match catalog_provider_route(source_id)? {
        CatalogProviderRoute::Opds(endpoint_id) => Ok(endpoint_id),
        CatalogProviderRoute::Overdrive => Err(CatalogProviderError::new("catalog source does not belong to the OPDS provider")),
    }
}

pub fn validate_opds_catalog_locator(endpoint: &OpdsEndpoint, locator: &str) -> Result<(), CatalogProviderError> {
    let expected = url::Url::parse(&endpoint.url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let actual = url::Url::parse(locator).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if !matches!(actual.scheme(), "http" | "https") || !actual.username().is_empty() || actual.password().is_some() {
        return Err(CatalogProviderError::new("catalog page locator must be an HTTP URL without embedded credentials"));
    }
    if actual.origin() != expected.origin() {
        return Err(CatalogProviderError::new("catalog page locator does not belong to the registered OPDS origin"));
    }
    Ok(())
}

pub fn overdrive_source() -> CatalogSource {
    CatalogSource {
        id: CatalogSourceId::parse(OVERDRIVE_SOURCE_ID).expect("OverDrive source id is valid"),
        provider: CatalogProviderKind::Overdrive,
        title: "OverDrive / Libby".to_owned(),
        description: "Find ebooks and audiobooks offered by participating libraries. Borrowing and holds continue securely in Libby until an approved circulation integration is configured.".to_owned(),
        languages: vec!["Multilingual".to_owned()],
        authentication: CatalogAuthentication::External { action_label: "Open Libby".to_owned(), url: LIBBY_HOME_URL.to_owned() },
        capabilities: vec![CatalogCapability::Browse, CatalogCapability::Search, CatalogCapability::ExternalFulfillment],
    }
}

pub fn opds_page(source_id: CatalogSourceId, catalog: OpdsCatalog) -> CatalogPage {
    let catalog_locator = catalog.source_url.clone();
    CatalogPage {
        source_id: source_id.clone(),
        freshness: match catalog.freshness {
            bokheim_opds::OpdsCatalogFreshness::Current => CatalogFreshness::Current,
            bokheim_opds::OpdsCatalogFreshness::Stale => CatalogFreshness::Stale,
        },
        title: catalog.title,
        locator: catalog.source_url,
        navigation: catalog.navigation.into_iter().map(catalog_link).collect(),
        publications: catalog.publications.into_iter().map(|publication| opds_publication(&source_id, &catalog_locator, publication)).collect(),
        groups: catalog.groups.into_iter().map(|group| opds_group(&source_id, &catalog_locator, group)).collect(),
        facets: catalog.facets.into_iter().filter_map(catalog_facet).collect(),
        pagination: CatalogPagination {
            first: catalog.pagination.first,
            previous: catalog.pagination.previous,
            next: catalog.pagination.next,
            last: catalog.pagination.last,
            number_of_items: catalog.pagination.number_of_items,
            items_per_page: catalog.pagination.items_per_page,
            current_page: catalog.pagination.current_page,
        },
        search: catalog.search.map(opds_search),
    }
}

pub fn overdrive_page(locator: Option<&str>) -> Result<CatalogPage, CatalogProviderError> {
    if locator.is_some_and(|locator| locator != OVERDRIVE_ROOT_LOCATOR) {
        return Err(CatalogProviderError::new("OverDrive page locator is invalid"));
    }
    Ok(CatalogPage {
        source_id: CatalogSourceId::parse(OVERDRIVE_SOURCE_ID).expect("OverDrive source id is valid"),
        freshness: CatalogFreshness::Current,
        title: "OverDrive / Libby".to_owned(),
        locator: OVERDRIVE_ROOT_LOCATOR.to_owned(),
        navigation: vec![external_link("Find your library in Libby", LIBBY_HOME_URL), external_link("Browse OverDrive", OVERDRIVE_HOME_URL)],
        publications: Vec::new(),
        groups: Vec::new(),
        facets: Vec::new(),
        pagination: CatalogPagination::default(),
        search: Some(CatalogSearch { definition: CatalogSearchDefinition::Overdrive, fields: vec![CatalogSearchField::Query, CatalogSearchField::Title, CatalogSearchField::Author] }),
    })
}

pub fn overdrive_search_page(search: &CatalogSearch, query: &CatalogSearchQuery) -> Result<CatalogPage, CatalogProviderError> {
    if search.definition != CatalogSearchDefinition::Overdrive {
        return Err(CatalogProviderError::new("catalog search definition is invalid for the OverDrive provider"));
    }
    let terms = search_terms(query)?;
    let url = format!("https://www.overdrive.com/search?q={}", urlencoding::encode(&terms));
    let mut page = overdrive_page(None)?;
    page.title = format!("Search: {terms}");
    page.locator = format!("overdrive:search:{}", urlencoding::encode(&terms));
    page.navigation = vec![external_link(&format!("See OverDrive results for “{terms}”"), &url)];
    Ok(page)
}

pub fn opds_search_definition(search: &CatalogSearch) -> Result<OpdsSearch, CatalogProviderError> {
    match &search.definition {
        CatalogSearchDefinition::OpdsUriTemplate { base_url, template } => Ok(OpdsSearch::UriTemplate { base_url: base_url.clone(), template: template.clone() }),
        CatalogSearchDefinition::OpdsOpenSearchDescription { url } => Ok(OpdsSearch::OpenSearchDescription { url: url.clone() }),
        CatalogSearchDefinition::Overdrive => Err(CatalogProviderError::new("OverDrive search cannot be executed by the OPDS provider")),
    }
}

pub fn opds_search_query(query: CatalogSearchQuery) -> OpdsSearchQuery {
    OpdsSearchQuery { query: query.query, title: query.title, author: query.author }
}

pub fn opds_import_request(request: &CatalogImportRequest, catalog: &OpdsCatalog) -> Result<(OpdsEndpointId, OpdsPublicationIdentity, OpdsDirectAcquisition), CatalogProviderError> {
    let endpoint = opds_endpoint_id(&request.source_id)?;
    let requested_identity = OpdsPublicationIdentity { publication_id: request.publication.edition_id.clone(), work_id: request.publication.work_id.clone() };
    requested_identity.validate().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if catalog.source_url != request.catalog_locator {
        return Err(CatalogProviderError::new("catalog import does not belong to the verified catalog page"));
    }
    let publication = catalog
        .publications
        .iter()
        .chain(catalog.groups.iter().flat_map(|group| group.publications.iter()))
        .find(|publication| publication.id == requested_identity.publication_id && publication.work_id == requested_identity.work_id)
        .ok_or_else(|| CatalogProviderError::new("catalog publication is not present on the verified catalog page"))?;
    let requested_format = match request.book_format {
        CatalogBookFormat::Epub => OpdsBookFormat::Epub,
        CatalogBookFormat::Pdf => OpdsBookFormat::Pdf,
    };
    let acquisition = publication
        .acquisitions
        .iter()
        .find_map(|acquisition| match acquisition {
            OpdsAcquisition::Direct(acquisition)
                if acquisition.url == request.download_url && acquisition.book_format == requested_format && acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) =>
            {
                Some(acquisition.clone())
            }
            _ => None,
        })
        .ok_or_else(|| CatalogProviderError::new("catalog acquisition is not an importable link on the verified catalog page"))?;
    Ok((endpoint, requested_identity, acquisition))
}

pub fn catalog_page_import_request(request: &CatalogImportRequest, page: &CatalogPage) -> Result<(OpdsEndpointId, OpdsPublicationIdentity, OpdsDirectAcquisition), CatalogProviderError> {
    let endpoint = opds_endpoint_id(&request.source_id)?;
    if page.source_id != request.source_id || page.locator != request.catalog_locator {
        return Err(CatalogProviderError::new("catalog import does not belong to the verified catalog page"));
    }
    let publication = page
        .publications
        .iter()
        .chain(page.groups.iter().flat_map(|group| group.publications.iter()))
        .find(|publication| publication.identity == request.publication)
        .ok_or_else(|| CatalogProviderError::new("catalog publication is not present on the verified catalog page"))?;
    if !publication.actions.iter().any(|action| matches!(action, CatalogPublicationAction::Import { request: candidate } if candidate == request)) {
        return Err(CatalogProviderError::new("catalog acquisition is not an importable link on the verified catalog page"));
    }
    let identity = OpdsPublicationIdentity { publication_id: request.publication.edition_id.clone(), work_id: request.publication.work_id.clone() };
    identity.validate().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let book_format = match request.book_format {
        CatalogBookFormat::Epub => OpdsBookFormat::Epub,
        CatalogBookFormat::Pdf => OpdsBookFormat::Pdf,
    };
    let acquisition = OpdsDirectAcquisition { url: request.download_url.clone(), kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("catalog-book.{}", book_format.extension()), price: None };
    Ok((endpoint, identity, acquisition))
}

fn search_terms(query: &CatalogSearchQuery) -> Result<String, CatalogProviderError> {
    let terms = [Some(query.query.trim()).filter(|value| !value.is_empty()), query.title.as_deref().map(str::trim).filter(|value| !value.is_empty()), query.author.as_deref().map(str::trim).filter(|value| !value.is_empty())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if terms.is_empty() || terms.len() > 500 || terms.chars().any(char::is_control) {
        return Err(CatalogProviderError::new("catalog search must contain between 1 and 500 characters without control characters"));
    }
    Ok(terms)
}

fn external_link(title: &str, url: &str) -> CatalogLink {
    CatalogLink { title: title.to_owned(), target: CatalogLinkTarget::External { url: url.to_owned() }, number_of_items: None, active: false }
}

fn catalog_link(link: OpdsCatalogLink) -> CatalogLink {
    let target = match link.target {
        OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
        OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
    };
    CatalogLink { title: link.title, target, number_of_items: link.number_of_items, active: link.active }
}

fn catalog_facet(mut facet: OpdsFacet) -> Option<CatalogFacet> {
    if matches!(facet.title.trim().to_ascii_lowercase().as_str(), "license" | "collection") {
        return None;
    }
    if facet.title.trim().eq_ignore_ascii_case("format") {
        facet.links = facet.links.into_iter().filter_map(compact_format_facet_link).collect();
    }
    (!facet.links.is_empty()).then(|| CatalogFacet { title: facet.title, links: facet.links.into_iter().map(catalog_link).collect() })
}

fn compact_format_facet_link(mut link: OpdsCatalogLink) -> Option<OpdsCatalogLink> {
    let words = link.title.split(|character: char| !character.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>();
    link.title = if words.iter().any(|word| word.eq_ignore_ascii_case("epub")) {
        "EPUB".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("pdf")) {
        "PDF".to_owned()
    } else {
        return None;
    };
    Some(link)
}

fn opds_group(source_id: &CatalogSourceId, catalog_locator: &str, group: OpdsGroup) -> CatalogGroup {
    CatalogGroup {
        title: group.title,
        page_locator: group.self_url,
        number_of_items: group.number_of_items,
        navigation: group.navigation.into_iter().map(catalog_link).collect(),
        publications: group.publications.into_iter().map(|publication| opds_publication(source_id, catalog_locator, publication)).collect(),
    }
}

fn opds_publication(source_id: &CatalogSourceId, catalog_locator: &str, publication: OpdsPublication) -> CatalogPublication {
    let identity = CatalogPublicationIdentity { edition_id: publication.id, work_id: publication.work_id };
    let mut import_formats = Vec::new();
    let actions = publication
        .acquisitions
        .into_iter()
        .filter_map(|acquisition| opds_action(source_id, catalog_locator, &identity, acquisition))
        .filter(|action| match action {
            CatalogPublicationAction::Import { request } if import_formats.contains(&request.book_format) => false,
            CatalogPublicationAction::Import { request } => {
                import_formats.push(request.book_format);
                true
            }
            CatalogPublicationAction::External { .. } => true,
        })
        .collect();
    let description = publication.description.map(|description| match description {
        OpdsDescription::PlainText { text } => CatalogDescription::PlainText { text },
        OpdsDescription::Html { safe_html, plain_text, .. } | OpdsDescription::Xhtml { safe_html, plain_text, .. } => CatalogDescription::RichHtml { safe_html, plain_text },
    });
    let authors = publication
        .authors
        .into_iter()
        .map(|author| CatalogAuthorCredit {
            name: author.name,
            sort_name: author.sort_name,
            identifiers: author.identifiers,
            provider_link: author.provider_link.map(|link| match link.target {
                OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
                OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
            }),
        })
        .collect();
    CatalogPublication { identity, title: publication.title, authors, description, image_url: publication.image_url, actions }
}

fn opds_action(source_id: &CatalogSourceId, catalog_locator: &str, identity: &CatalogPublicationIdentity, acquisition: OpdsAcquisition) -> Option<CatalogPublicationAction> {
    match acquisition {
        OpdsAcquisition::Direct(acquisition) if acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) => Some(CatalogPublicationAction::Import {
            request: CatalogImportRequest { source_id: source_id.clone(), catalog_locator: catalog_locator.to_owned(), publication: identity.clone(), download_url: acquisition.url, book_format: book_format(acquisition.book_format) },
        }),
        OpdsAcquisition::Direct(acquisition) => {
            Some(CatalogPublicationAction::External { purpose: external_purpose(acquisition.kind), url: acquisition.url, price: acquisition.price.map(price), available_formats: vec![book_format(acquisition.book_format)] })
        }
        OpdsAcquisition::Indirect(acquisition) => Some(CatalogPublicationAction::External {
            purpose: external_purpose(acquisition.kind),
            url: acquisition.url,
            price: acquisition.price.map(price),
            available_formats: acquisition.available_formats.into_iter().map(book_format).collect(),
        }),
        OpdsAcquisition::Unsupported(acquisition) => Some(CatalogPublicationAction::External { purpose: external_purpose(acquisition.kind), url: acquisition.url, price: acquisition.price.map(price), available_formats: Vec::new() }),
    }
}

fn opds_search(search: OpdsSearch) -> CatalogSearch {
    let mut fields = vec![CatalogSearchField::Query];
    if let OpdsSearch::UriTemplate { template, .. } = &search {
        if template_has_variable(template, "title") {
            fields.push(CatalogSearchField::Title);
        }
        if template_has_variable(template, "author") {
            fields.push(CatalogSearchField::Author);
        }
    }
    let definition = match search {
        OpdsSearch::UriTemplate { base_url, template } => CatalogSearchDefinition::OpdsUriTemplate { base_url, template },
        OpdsSearch::OpenSearchDescription { url } => CatalogSearchDefinition::OpdsOpenSearchDescription { url },
    };
    CatalogSearch { definition, fields }
}

fn template_has_variable(template: &str, expected: &str) -> bool {
    template
        .split('{')
        .skip(1)
        .filter_map(|suffix| suffix.split_once('}').map(|(expression, _)| expression))
        .flat_map(|expression| expression.trim_start_matches(['+', '#', '.', '/', ';', '?', '&']).split(','))
        .any(|variable| variable.trim_end_matches(['?', '*']).split(':').next() == Some(expected))
}

fn book_format(format: OpdsBookFormat) -> CatalogBookFormat {
    match format {
        OpdsBookFormat::Epub => CatalogBookFormat::Epub,
        OpdsBookFormat::Pdf => CatalogBookFormat::Pdf,
    }
}

fn price(value: OpdsPrice) -> CatalogPrice {
    CatalogPrice { currency: value.currency, value: value.value }
}

fn external_purpose(kind: OpdsAcquisitionKind) -> CatalogExternalActionKind {
    match kind {
        OpdsAcquisitionKind::Preview => CatalogExternalActionKind::Preview,
        OpdsAcquisitionKind::Buy => CatalogExternalActionKind::Buy,
        OpdsAcquisitionKind::Borrow => CatalogExternalActionKind::Borrow,
        OpdsAcquisitionKind::Subscribe => CatalogExternalActionKind::Subscribe,
        OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic => CatalogExternalActionKind::Open,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::Query;
    use axum::http::{header, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use bokheim_app_contract::catalog::CatalogPublicationIdentifierScheme;
    use bokheim_opds::{OpdsAcquisition, OpdsAuthorCredit, OpdsAuthorLink, OpdsIndirectAcquisition, OpdsPagination, OpdsPublication, OpdsUnsupportedAcquisition, OpdsVersion};
    use image::{DynamicImage, ImageOutputFormat};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn opds_actions_preserve_all_import_formats_and_external_fulfillment() {
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let raw_publication = OpdsPublication {
            id: "edition".to_owned(),
            work_id: Some("work".to_owned()),
            title: "Book".to_owned(),
            authors: vec![OpdsAuthorCredit {
                name: "Author".to_owned(),
                sort_name: Some("Author, An".to_owned()),
                identifiers: vec!["https://isni.org/isni/0000000121400562".to_owned()],
                provider_link: Some(OpdsAuthorLink { url: "https://catalog.example/authors/1".to_owned(), target: OpdsLinkTarget::Catalog }),
            }],
            description: None,
            image_url: None,
            preferred_download: None,
            acquisitions: vec![
                OpdsAcquisition::Direct(OpdsDirectAcquisition {
                    url: "https://downloads.example/book.epub".to_owned(),
                    kind: OpdsAcquisitionKind::OpenAccess,
                    book_format: OpdsBookFormat::Epub,
                    suggested_file_name: "book.epub".to_owned(),
                    price: None,
                }),
                OpdsAcquisition::Direct(OpdsDirectAcquisition {
                    url: "https://downloads.example/book-legacy.epub".to_owned(),
                    kind: OpdsAcquisitionKind::OpenAccess,
                    book_format: OpdsBookFormat::Epub,
                    suggested_file_name: "book-legacy.epub".to_owned(),
                    price: None,
                }),
                OpdsAcquisition::Direct(OpdsDirectAcquisition {
                    url: "https://example.com/book.pdf".to_owned(),
                    kind: OpdsAcquisitionKind::OpenAccess,
                    book_format: OpdsBookFormat::Pdf,
                    suggested_file_name: "book.pdf".to_owned(),
                    price: None,
                }),
                OpdsAcquisition::Unsupported(OpdsUnsupportedAcquisition {
                    url: "https://downloads.example/book.kindle".to_owned(),
                    kind: OpdsAcquisitionKind::Generic,
                    media_type: Some("application/x-mobipocket-ebook".to_owned()),
                    price: None,
                }),
                OpdsAcquisition::Indirect(OpdsIndirectAcquisition {
                    url: "https://example.com/borrow".to_owned(),
                    kind: OpdsAcquisitionKind::Borrow,
                    media_type: Some("text/html".to_owned()),
                    available_formats: vec![OpdsBookFormat::Epub],
                    price: None,
                }),
            ],
        };
        let catalog = OpdsCatalog {
            version: OpdsVersion::V2_0,
            freshness: bokheim_opds::OpdsCatalogFreshness::Current,
            title: "Catalog".to_owned(),
            source_url: "https://example.com/catalog".to_owned(),
            self_url: None,
            start_url: None,
            navigation: Vec::new(),
            publications: vec![raw_publication.clone()],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: OpdsPagination::default(),
            search: None,
        };
        let publication = opds_publication(&source_id, &catalog.source_url, raw_publication);
        assert_eq!(publication.authors[0].name, "Author");
        assert_eq!(publication.authors[0].sort_name.as_deref(), Some("Author, An"));
        assert_eq!(publication.authors[0].identifiers, vec!["https://isni.org/isni/0000000121400562"]);
        assert_eq!(publication.authors[0].provider_link, Some(CatalogLinkTarget::Page { locator: "https://catalog.example/authors/1".to_owned() }));
        assert_eq!(publication.actions.len(), 4);
        assert!(matches!(publication.actions[0], CatalogPublicationAction::Import { ref request } if request.book_format == CatalogBookFormat::Epub));
        assert!(matches!(publication.actions[1], CatalogPublicationAction::Import { ref request } if request.book_format == CatalogBookFormat::Pdf));
        assert!(matches!(publication.actions[2], CatalogPublicationAction::External { purpose: CatalogExternalActionKind::Open, .. }));
        assert!(matches!(publication.actions[3], CatalogPublicationAction::External { purpose: CatalogExternalActionKind::Borrow, .. }));
        let CatalogPublicationAction::Import { request } = &publication.actions[0] else { panic!("expected import action") };
        assert_eq!(request.catalog_locator, catalog.source_url);
        let (_, identity, acquisition) = opds_import_request(request, &catalog).unwrap();
        assert_eq!(identity.publication_id, "edition");
        assert_eq!(acquisition.url, "https://downloads.example/book.epub");
        assert_eq!(acquisition.suggested_file_name, "book.epub");
        let page = opds_page(source_id, catalog.clone());
        let (_, cached_identity, cached_acquisition) = catalog_page_import_request(request, &page).unwrap();
        assert_eq!(cached_identity, identity);
        assert_eq!(cached_acquisition.url, acquisition.url);
        let mut forged = request.clone();
        forged.download_url = "https://attacker.example/forged.epub".to_owned();
        assert!(opds_import_request(&forged, &catalog).is_err());
        assert!(catalog_page_import_request(&forged, &page).is_err());
    }

    #[test]
    fn catalog_facets_hide_provider_metadata_and_compact_supported_formats() {
        let link = |title: &str| OpdsCatalogLink { title: title.to_owned(), url: format!("https://example.com/{}", title.replace(' ', "-")), target: OpdsLinkTarget::Catalog, number_of_items: None, active: false };
        let catalog = OpdsCatalog {
            version: OpdsVersion::V1_2,
            freshness: bokheim_opds::OpdsCatalogFreshness::Current,
            title: "Catalog".to_owned(),
            source_url: "https://example.com/catalog".to_owned(),
            self_url: None,
            start_url: None,
            navigation: Vec::new(),
            publications: Vec::new(),
            groups: Vec::new(),
            facets: vec![
                OpdsFacet { title: "Order".to_owned(), links: vec![link("Newest")] },
                OpdsFacet { title: "Format".to_owned(), links: vec![link("eBooks available in format: pdf"), link("eBooks available in format: epub"), link("MOBI")] },
                OpdsFacet { title: "License".to_owned(), links: vec![link("Public domain")] },
                OpdsFacet { title: "Collection".to_owned(), links: vec![link("indexed in DOAB")] },
            ],
            pagination: OpdsPagination::default(),
            search: None,
        };
        let page = opds_page(CatalogSourceId::parse("opds-test").unwrap(), catalog);
        assert_eq!(page.facets.iter().map(|facet| facet.title.as_str()).collect::<Vec<_>>(), vec!["Order", "Format"]);
        assert_eq!(page.facets[1].links.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), vec!["PDF", "EPUB"]);
    }

    #[test]
    fn overdrive_is_an_honest_external_provider_until_circulation_is_configured() {
        let source = overdrive_source();
        assert_eq!(source.provider, CatalogProviderKind::Overdrive);
        assert!(source.capabilities.contains(&CatalogCapability::ExternalFulfillment));
        assert!(!source.capabilities.contains(&CatalogCapability::Borrow));
        assert!(matches!(source.authentication, CatalogAuthentication::External { .. }));
        let page = overdrive_search_page(&overdrive_page(None).unwrap().search.unwrap(), &CatalogSearchQuery { query: "Ursula Le Guin".to_owned(), title: None, author: None }).unwrap();
        assert!(matches!(page.navigation[0].target, CatalogLinkTarget::External { .. }));
    }

    #[test]
    fn provider_routing_validates_source_namespace_and_search_definitions() {
        assert!(matches!(catalog_provider_route(&CatalogSourceId::parse("overdrive").unwrap()).unwrap(), CatalogProviderRoute::Overdrive));
        assert!(matches!(catalog_provider_route(&CatalogSourceId::parse("opds-example").unwrap()).unwrap(), CatalogProviderRoute::Opds(_)));
        assert!(catalog_provider_route(&CatalogSourceId::parse("unknown").unwrap()).is_err());
        let wrong_search = CatalogSearch { definition: CatalogSearchDefinition::OpdsUriTemplate { base_url: "https://example.com".to_owned(), template: "search{?query}".to_owned() }, fields: vec![CatalogSearchField::Query] };
        assert!(overdrive_search_page(&wrong_search, &CatalogSearchQuery { query: "book".to_owned(), title: None, author: None }).is_err());
        let endpoint = OpdsEndpoint {
            id: OpdsEndpointId::parse("example").unwrap(),
            title: "Example".to_owned(),
            description: "Example".to_owned(),
            languages: vec!["English".to_owned()],
            url: "https://catalog.example/root".to_owned(),
            access: bokheim_opds::OpdsEndpointAccess::Open,
            authentication: OpdsEndpointAuthentication::None,
        };
        assert!(validate_opds_catalog_locator(&endpoint, "https://catalog.example/page").is_ok());
        assert!(validate_opds_catalog_locator(&endpoint, "https://attacker.example/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint, "https://user:secret@catalog.example/page").is_err());
    }

    #[test]
    fn opds_search_only_advertises_fields_supported_by_its_template() {
        let simple = opds_search(OpdsSearch::UriTemplate { base_url: "https://example.com".to_owned(), template: "search{?searchTerms}".to_owned() });
        assert_eq!(simple.fields, vec![CatalogSearchField::Query]);
        assert!(matches!(&simple.definition, CatalogSearchDefinition::OpdsUriTemplate { base_url, template } if base_url == "https://example.com" && template == "search{?searchTerms}"));
        let advanced = opds_search(OpdsSearch::UriTemplate { base_url: "https://example.com".to_owned(), template: "search{?query,title,author}".to_owned() });
        assert_eq!(advanced.fields, vec![CatalogSearchField::Query, CatalogSearchField::Title, CatalogSearchField::Author]);
        let encoded = bokheim_wire::encode(&advanced).unwrap();
        assert_eq!(bokheim_wire::decode::<CatalogSearch>(&encoded, bokheim_wire::MAX_DECODED_RESPONSE_BYTES).unwrap(), advanced);
    }

    #[tokio::test]
    async fn wide_search_reports_authentication_without_contacting_the_catalog() {
        let endpoint = OpdsEndpoint {
            id: OpdsEndpointId::parse("members").unwrap(),
            title: "Members".to_owned(),
            description: "Members only".to_owned(),
            languages: vec!["English".to_owned()],
            url: "https://members.example/catalog".to_owned(),
            access: bokheim_opds::OpdsEndpointAccess::Membership,
            authentication: OpdsEndpointAuthentication::Basic { username_label: "Email".to_owned(), password: bokheim_opds::OpdsPasswordRequirement::Required },
        };
        let result = search_all_catalogs(&bokheim_opds::OpdsClient::new().unwrap(), vec![endpoint], "history").await.unwrap();
        assert_eq!(result.query, "history");
        assert!(matches!(result.groups[0].outcome, CatalogWideSearchOutcome::AuthenticationRequired));
        assert!(matches!(result.groups[1].outcome, CatalogWideSearchOutcome::Results { .. }));
        assert!(search_all_catalogs(&bokheim_opds::OpdsClient::new().unwrap(), Vec::new(), " ").await.is_err());
    }

    #[tokio::test]
    async fn local_author_discovery_needs_no_bokheim_catalog_server() {
        let application = Router::new()
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
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let endpoint = OpdsEndpoint {
            id: OpdsEndpointId::parse("local-fallback").unwrap(),
            title: "Local fallback".to_owned(),
            description: "Local fallback test".to_owned(),
            languages: vec!["English".to_owned()],
            url: format!("http://{address}/root"),
            access: bokheim_opds::OpdsEndpointAccess::Open,
            authentication: OpdsEndpointAuthentication::None,
        };
        let request = CatalogAuthorDiscoveryRequest { author_name: "Alex Smith".to_owned(), book_identifiers: vec![CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "9780306406157".to_owned() }] };

        let result = discover_author_books_locally(&bokheim_opds::OpdsClient::new().unwrap(), vec![endpoint], &request).await.unwrap();

        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].publication.title, "Another book by the identified author");
        assert!(matches!(result.matches[0].evidence, CatalogAuthorDiscoveryEvidence::BookIdentifier { .. }));
        assert_eq!(result.matches[1].publication.title, "Book by a different Alex Smith");
        assert_eq!(result.matches[1].evidence, CatalogAuthorDiscoveryEvidence::AuthorName);
        server.abort();
    }

    #[tokio::test]
    async fn catalog_server_pages_survive_in_the_bounded_local_cache() {
        let temporary = tempfile::tempdir().unwrap();
        let cache_path = temporary.path().join("catalog-server-pages.json");
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let locator = "https://catalog.example/root";
        let client = CatalogServerClient::with_persistent_cache("http://127.0.0.1:9/", cache_path.clone()).unwrap();
        let mut url = client.endpoint("api/opds/sources/opds-test/page").unwrap();
        url.query_pairs_mut().append_pair("locator", locator);
        let cache_key = format!("GET {url}");
        let page = CatalogPage {
            source_id: source_id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Locally cached".to_owned(),
            locator: locator.to_owned(),
            navigation: Vec::new(),
            publications: Vec::new(),
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        client.cache_response(cache_key.clone(), CatalogServerResponse::Page(page.clone())).await;
        let restored = CatalogServerClient::with_persistent_cache("http://127.0.0.1:9/", cache_path).unwrap();
        assert_eq!(restored.catalog_page(&source_id, Some(locator), false).await.unwrap(), page);
        restored.cache.lock().unwrap().entries.get_mut(&cache_key).unwrap().inserted_at = Instant::now().checked_sub(CATALOG_SERVER_CACHE_TTL + Duration::from_secs(1)).unwrap();
        let stale = restored.catalog_page(&source_id, Some(locator), false).await.unwrap();
        assert_eq!(stale.freshness, CatalogFreshness::Stale);
    }

    #[tokio::test]
    async fn author_discovery_cache_persists_empty_results_and_reports_freshness() {
        let temporary = tempfile::tempdir().unwrap();
        let cache_path = temporary.path().join("catalog-server-pages.json");
        let request = CatalogAuthorDiscoveryRequest { author_name: "Alex Smith".to_owned(), book_identifiers: Vec::new() };
        let result = CatalogAuthorDiscoveryResult { author_name: "Alex Smith".to_owned(), matches: Vec::new() };
        let client = CatalogServerClient::with_persistent_cache("http://127.0.0.1:9/", cache_path.clone()).unwrap();
        client.cache_author_discovery(&request, &result).await.unwrap();
        assert_eq!(client.cached_author_discovery(&request).unwrap(), Some((result.clone(), true)));

        let restored = CatalogServerClient::with_persistent_cache("http://127.0.0.1:9/", cache_path).unwrap();
        assert_eq!(restored.cached_author_discovery(&request).unwrap(), Some((result.clone(), true)));
        let (_, cache_key) = restored.author_discovery_cache_key(&request).unwrap();
        restored.cache.lock().unwrap().entries.get_mut(&cache_key).unwrap().inserted_at = Instant::now().checked_sub(CATALOG_SERVER_CACHE_TTL + Duration::from_secs(1)).unwrap();
        assert_eq!(restored.cached_author_discovery(&request).unwrap(), Some((result, false)));
    }

    #[tokio::test]
    async fn global_search_uses_the_compressed_bokheim_wire_response_and_local_cache() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let result = CatalogWideSearchResult { query: "history".to_owned(), groups: Vec::new() };
        let compressed = bokheim_wire::zstd(&bokheim_wire::encode(&result).unwrap()).unwrap();
        let application = Router::new().route(
            "/api/opds/search",
            get(move |headers: axum::http::HeaderMap| {
                request_counter.fetch_add(1, Ordering::SeqCst);
                assert!(headers.get(header::ACCEPT).and_then(|value| value.to_str().ok()).is_some_and(bokheim_wire::is_current_media_type));
                let body = compressed.clone();
                async move { Response::builder().header(header::CONTENT_TYPE, bokheim_wire::MEDIA_TYPE).header(header::CONTENT_ENCODING, bokheim_wire::CONTENT_ENCODING_ZSTD).body(Body::from(body)).unwrap() }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let client = CatalogServerClient::new(&format!("http://{address}/")).unwrap();

        assert_eq!(client.search_catalogs("history").await.unwrap().query, "history");
        assert_eq!(client.search_catalogs("history").await.unwrap().query, "history");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn catalog_covers_are_authorized_normalized_and_reused_from_disk() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let failures = Arc::new(AtomicUsize::new(0));
        let failure_counter = failures.clone();
        let mut png = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(16, 24).write_to(&mut png, ImageOutputFormat::Png).unwrap();
        let source_bytes = png.into_inner();
        let application = Router::new()
            .route(
                "/cover.png",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    let bytes = source_bytes.clone();
                    async move { Response::builder().header(header::CONTENT_TYPE, "image/png").body(Body::from(bytes)).unwrap() }
                }),
            )
            .route(
                "/broken.png",
                get(move || {
                    failure_counter.fetch_add(1, Ordering::SeqCst);
                    async { StatusCode::BAD_GATEWAY }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let image_url = format!("http://{address}/cover.png");
        let broken_url = format!("http://{address}/broken.png");
        let page = CatalogPage {
            source_id: source_id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Covers".to_owned(),
            locator: format!("http://{address}/root"),
            navigation: Vec::new(),
            publications: vec![
                CatalogPublication {
                    identity: CatalogPublicationIdentity { edition_id: "edition".to_owned(), work_id: None },
                    title: "Book".to_owned(),
                    authors: Vec::new(),
                    description: None,
                    image_url: Some(image_url.clone()),
                    actions: Vec::new(),
                },
                CatalogPublication {
                    identity: CatalogPublicationIdentity { edition_id: "broken".to_owned(), work_id: None },
                    title: "Broken".to_owned(),
                    authors: Vec::new(),
                    description: None,
                    image_url: Some(broken_url.clone()),
                    actions: Vec::new(),
                },
            ],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        let cache = CatalogCoverCache::new_persistent(temporary.path().join("covers")).unwrap();
        assert!(cache.cover(&source_id, &image_url).await.is_err());
        cache.remember_page(&page).await;
        let first = cache.cover(&source_id, &image_url).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(cache.cover(&source_id, &image_url).await.unwrap(), first);
        let restored = CatalogCoverCache::new_persistent(temporary.path().join("covers")).unwrap();
        assert_eq!(restored.cover(&source_id, &image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(cache.cover(&source_id, &broken_url).await.is_err());
        assert!(cache.cover(&source_id, &broken_url).await.is_err());
        assert_eq!(failures.load(Ordering::SeqCst), 1);
        std::fs::remove_file(temporary.path().join("covers").join(format!("{}.jpg", cover_cache_key(&source_id, &image_url)))).unwrap();
        let authorized_after_restart = CatalogCoverCache::new_persistent(temporary.path().join("covers")).unwrap();
        assert_eq!(authorized_after_restart.cover(&source_id, &image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn local_cover_cache_fetches_server_covers_only_once() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(16, 24).write_to(&mut jpeg, ImageOutputFormat::Jpeg(85)).unwrap();
        let server_bytes = jpeg.into_inner();
        let application = Router::new().route(
            "/api/opds/sources/opds-test/cover",
            get(move || {
                request_counter.fetch_add(1, Ordering::SeqCst);
                let bytes = server_bytes.clone();
                async move { Response::builder().header(header::CONTENT_TYPE, "image/jpeg").body(Body::from(bytes)).unwrap() }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let image_url = "https://covers.example/book.jpg";
        let page = CatalogPage {
            source_id: source_id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Covers".to_owned(),
            locator: "https://catalog.example/root".to_owned(),
            navigation: Vec::new(),
            publications: vec![CatalogPublication {
                identity: CatalogPublicationIdentity { edition_id: "edition".to_owned(), work_id: None },
                title: "Book".to_owned(),
                authors: Vec::new(),
                description: None,
                image_url: Some(image_url.to_owned()),
                actions: Vec::new(),
            }],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        let server = CatalogServerClient::new(&format!("http://{address}/")).unwrap();
        let cache_path = temporary.path().join("covers");
        let cache = CatalogCoverCache::new(cache_path.clone()).unwrap();
        cache.remember_page(&page).await;
        let first = cache.cover_from_server(&server, &source_id, image_url).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(cache.cover_from_server(&server, &source_id, image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server_task.abort();
        let restored = CatalogCoverCache::new(cache_path).unwrap();
        assert_eq!(restored.cover_from_server(&server, &source_id, image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn wide_search_starts_the_normal_catalogue_set_in_one_wave() {
        assert_eq!(wide_search_concurrency(9), 9);
        assert_eq!(wide_search_concurrency(16), 16);
        assert_eq!(wide_search_concurrency(100), 16);
    }

    #[tokio::test]
    #[ignore = "requires live third-party catalog servers"]
    async fn live_wide_search_never_exposes_invalid_publication_identities() {
        let result = search_all_catalogs(&bokheim_opds::OpdsClient::new().unwrap(), bokheim_opds::built_in_opds_endpoints(), "Carmen").await.unwrap();
        for group in result.groups {
            if let CatalogWideSearchOutcome::Results { page } = group.outcome {
                for publication in page.publications.into_iter().chain(page.groups.into_iter().flat_map(|group| group.publications)) {
                    OpdsPublicationIdentity { publication_id: publication.identity.edition_id, work_id: publication.identity.work_id }
                        .validate()
                        .unwrap_or_else(|error| panic!("{} returned an invalid identity: {error}", group.source.title));
                }
            }
        }
    }
}

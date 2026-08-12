pub use catalog_contract::{CatalogGroupLayout, CatalogPageLayout, CatalogPageRequest, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchRequest, CatalogSearchResponse};
pub use catalog_core::{
    discover_author_books_locally, opds_catalog_source_id, opds_page, opds_search_definition, opds_search_query, opds_source, resolve_author_discovery, search_all_catalogs, validate_opds_catalog_locator, CatalogCache, CatalogCoverCache,
    CatalogLanguageFilter, CatalogProviderError, CatalogSearchMatches,
};
use catalog_core::{opds_endpoint_id, search_opds_catalogs, validate_server_cover};
#[cfg(test)]
use catalog_core::{opds_publication, opds_search, wide_search_concurrency};

use catalog_contract::{
    CatalogAuthorDiscoveryRequest, CatalogAuthorDiscoveryResult, CatalogBookFormat, CatalogFreshness, CatalogGroup, CatalogImportRequest, CatalogPage, CatalogPagination, CatalogSearch, CatalogSearchQuery, CatalogSource, CatalogSourceId,
    CatalogWideSearchGroup, CatalogWideSearchOutcome, CatalogWideSearchResult,
};
use futures_util::stream::{self, StreamExt};
use opds::{CatalogRegistry, DownloadedOpdsBook, OpdsAcquisition, OpdsAcquisitionKind, OpdsBookFormat, OpdsCatalog, OpdsDirectAcquisition, OpdsEndpoint, OpdsEndpointId, OpdsEndpointStore, OpdsPublicationIdentity, OpdsSearch};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use url::Url;

const MAX_WIDE_SEARCH_CONCURRENCY: usize = 16;
const MAX_CATALOG_SERVER_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct CatalogServerClient {
    http: reqwest::Client,
    base_url: Url,
}

pub struct CatalogClient {
    opds: opds::OpdsClient,
    server: CatalogServerClient,
    cache: CatalogCache,
    covers: CatalogCoverCache,
    endpoints: Mutex<OpdsEndpointStore>,
    languages: RwLock<CatalogLanguageFilter>,
}

pub struct CatalogDownload {
    download: DownloadedOpdsBook,
}

impl CatalogDownload {
    pub fn path(&self) -> &Path {
        self.download.path()
    }

    pub fn file_name(&self) -> &str {
        self.download.file_name()
    }
}

impl CatalogClient {
    pub fn with_persistent_cache(server_base_url: &str, data_directory: &Path) -> Result<Self, CatalogProviderError> {
        Self::with_supported_languages(server_base_url, data_directory, std::iter::empty::<&str>())
    }

    pub fn with_supported_languages(server_base_url: &str, data_directory: &Path, languages: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self, CatalogProviderError> {
        let opds = opds::OpdsClient::new().map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let server = CatalogServerClient::new(server_base_url)?;
        let cache = CatalogCache::open(data_directory.join("catalog-cache.db")).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let covers = CatalogCoverCache::new(data_directory.join("catalog-covers"))?;
        let endpoints = OpdsEndpointStore::load(data_directory.join("opds-catalogs.json"), data_directory.join("custom-opds-catalogs.json"));
        Ok(Self { opds, server, cache, covers, endpoints: Mutex::new(endpoints), languages: RwLock::new(CatalogLanguageFilter::new(languages)) })
    }

    pub fn set_supported_languages(&self, languages: impl IntoIterator<Item = impl AsRef<str>>) -> Result<(), CatalogProviderError> {
        *self.languages.write().map_err(|_| CatalogProviderError::new("catalog language preference lock was poisoned"))? = CatalogLanguageFilter::new(languages);
        Ok(())
    }

    fn language_filter(&self) -> CatalogLanguageFilter {
        self.languages.read().map(|languages| languages.clone()).unwrap_or_else(|_| CatalogLanguageFilter::new(std::iter::empty::<&str>()))
    }

    fn opds_endpoints(&self) -> Vec<OpdsEndpoint> {
        self.endpoints.lock().map(|store| store.endpoints()).unwrap_or_else(|_| opds::built_in_opds_endpoints())
    }

    fn supported_endpoint_sources(&self, endpoints: impl IntoIterator<Item = OpdsEndpoint>) -> Vec<(CatalogSource, OpdsEndpoint)> {
        endpoints
            .into_iter()
            .filter_map(|endpoint| {
                let source = opds_source(&endpoint);
                self.language_filter().source_supported(&source).then_some((source, endpoint))
            })
            .collect()
    }

    pub fn sources(&self) -> Vec<CatalogSource> {
        self.opds_endpoints().into_iter().map(|endpoint| opds_source(&endpoint)).collect()
    }

    pub fn custom_sources(&self) -> Result<Vec<CatalogSource>, CatalogProviderError> {
        let endpoints = self.endpoint_store()?.custom_endpoints().to_vec();
        Ok(endpoints.into_iter().map(|endpoint| opds_source(&endpoint)).collect())
    }

    /// Returns the current local SQLite matches without performing network I/O.
    pub async fn cached_search_catalogs(&self, query: &str) -> Result<CatalogWideSearchResult, CatalogProviderError> {
        let mut groups = Vec::new();
        for (source, _) in self.supported_endpoint_sources(self.opds_endpoints()) {
            let page = self.cached_search_page(&source, query)?;
            groups.push(CatalogWideSearchGroup { source, outcome: CatalogWideSearchOutcome::Results { page } });
        }
        let result = CatalogWideSearchResult { query: query.trim().to_owned(), groups };
        self.covers.remember_wide_search(&result);
        Ok(result)
    }

    /// Returns local SQLite matches for one source without network I/O.
    pub async fn cached_search(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogPage, CatalogProviderError> {
        let endpoint = self.endpoint_for_source(source_id)?;
        let page = self.cached_search_page(&opds_source(&endpoint), query)?;
        self.covers.remember_page(&page);
        Ok(page)
    }

    fn cached_search_page(&self, source: &CatalogSource, query: &str) -> Result<CatalogPage, CatalogProviderError> {
        let matches = self.cache.search(&source.id, query).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let mut page = search_page(source.id.clone(), query.trim().to_owned(), matches);
        page.freshness = CatalogFreshness::Stale;
        self.language_filter().filter_page(source, &mut page);
        Ok(page)
    }

    fn endpoint_for_source(&self, source_id: &CatalogSourceId) -> Result<OpdsEndpoint, CatalogProviderError> {
        let endpoint_id = opds_endpoint_id(source_id)?;
        self.opds_endpoints().into_iter().find(|endpoint| endpoint.id == endpoint_id).ok_or_else(|| CatalogProviderError::new("catalog source is no longer available"))
    }

    fn is_curated_endpoint(&self, endpoint: &OpdsEndpoint) -> Result<bool, CatalogProviderError> {
        Ok(self.endpoint_store()?.is_curated_endpoint(endpoint))
    }

    pub async fn search_catalogs(&self, query: &str) -> Result<CatalogWideSearchResult, CatalogProviderError> {
        let (endpoints, custom_ids) = {
            let store = self.endpoint_store()?;
            (store.endpoints(), store.custom_endpoints().iter().map(|endpoint| endpoint.id.clone()).collect::<HashSet<_>>())
        };
        let plans = self.supported_endpoint_sources(endpoints).into_iter().map(|(source, endpoint)| self.cached_search_page(&source, query).map(|cached| (source, endpoint, cached))).collect::<Result<Vec<_>, _>>()?;
        let mut indexed = stream::iter(plans.into_iter().enumerate())
            .map(|(index, (source, endpoint, cached))| {
                let custom_ids = &custom_ids;
                async move {
                    let refreshed = if custom_ids.contains(&endpoint.id) {
                        search_opds_catalogs(&self.opds, vec![endpoint], query).await.map(|mut groups| groups.pop().map(|group| group.outcome).unwrap_or(CatalogWideSearchOutcome::Unsupported))
                    } else {
                        self.search_cached_endpoint(&endpoint, query).await
                    };
                    let outcome = refresh_outcome_or_cached(refreshed, cached);
                    (index, CatalogWideSearchGroup { source, outcome })
                }
            })
            .buffer_unordered(MAX_WIDE_SEARCH_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        indexed.sort_by_key(|(index, _)| *index);
        let mut result = CatalogWideSearchResult { query: query.trim().to_owned(), groups: indexed.into_iter().map(|(_, group)| group).collect() };
        self.language_filter().filter_wide_search(&mut result);
        self.covers.remember_wide_search(&result);
        Ok(result)
    }

    pub async fn page(&self, source_id: &CatalogSourceId, locator: Option<&str>) -> Result<CatalogPage, CatalogProviderError> {
        let endpoint = self.endpoint_for_source(source_id)?;
        let locator = locator.unwrap_or(&endpoint.url);
        validate_opds_catalog_locator(&endpoint, locator)?;
        if self.is_curated_endpoint(&endpoint)? {
            let request = self.page_request(source_id, locator)?;
            match self.server.catalog_page(source_id, &request).await {
                Ok(response) => {
                    let mut page = self.cache.apply_page_response(&endpoint.url, response).map_err(|error| CatalogProviderError::new(error.to_string()))?;
                    self.language_filter().filter_page(&opds_source(&endpoint), &mut page);
                    self.covers.remember_page(&page);
                    return Ok(page);
                }
                Err(error) => log::warn!(target: "catalog", "catalog server page unavailable; using direct OPDS: {error}"),
            }
        }
        let catalog = self.opds.fetch_catalog(locator).await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url)?;
        let mut page = opds_page(source_id.clone(), catalog);
        self.language_filter().filter_page(&opds_source(&endpoint), &mut page);
        self.cache.put_page(source_id, &endpoint.url, &page, None, None).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        self.covers.remember_page(&page);
        Ok(page)
    }

    pub async fn search(&self, source_id: &CatalogSourceId, search: &CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPage, CatalogProviderError> {
        let endpoint = self.endpoint_for_source(source_id)?;
        if self.is_curated_endpoint(&endpoint)? {
            match self.search_cached_source(source_id, &endpoint, &query.query).await {
                Ok(mut page) => {
                    self.language_filter().filter_page(&opds_source(&endpoint), &mut page);
                    self.covers.remember_page(&page);
                    return Ok(page);
                }
                Err(error) => log::warn!(target: "catalog", "catalog server search unavailable; using direct OPDS: {error}"),
            }
        }
        let search = opds_search_definition(search)?;
        let search_origin = match &search {
            OpdsSearch::UriTemplate { base_url, .. } => base_url,
            OpdsSearch::OpenSearchDescription { url } => url,
        };
        validate_opds_catalog_locator(&endpoint, search_origin)?;
        let query_text = query.query.clone();
        let catalog = self.opds.search_definition(&search, &opds_search_query(query)).await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url)?;
        let mut page = opds_page(source_id.clone(), catalog);
        self.language_filter().filter_page(&opds_source(&endpoint), &mut page);
        self.cache.put_page(source_id, &endpoint.url, &page, None, None).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        page = search_page(source_id.clone(), query_text.clone(), self.cache.search(source_id, &query_text).map_err(|error| CatalogProviderError::new(error.to_string()))?);
        self.covers.remember_page(&page);
        Ok(page)
    }

    pub async fn cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        let endpoint = self.endpoint_for_source(source_id)?;
        if self.is_curated_endpoint(&endpoint)? {
            match self.covers.cover_with(source_id, image_url, self.server.catalog_cover(source_id, image_url)).await {
                Ok(bytes) => return Ok(bytes),
                Err(error) => log::warn!(target: "catalog", "catalog server cover unavailable; fetching the remembered OPDS cover directly: {error}"),
            }
        }
        self.covers.cover(source_id, image_url).await
    }

    pub async fn download_import(&self, request: &CatalogImportRequest, temporary_directory: &Path) -> Result<CatalogDownload, CatalogProviderError> {
        let endpoint = self.endpoint_for_source(&request.source_id)?;
        validate_opds_catalog_locator(&endpoint, &request.catalog_locator)?;
        let catalog = self.opds.fetch_catalog_fresh(&request.catalog_locator).await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url)?;
        let (_, identity, acquisition) = opds_import_request(request, &catalog)?;
        identity.validate().map_err(|error| CatalogProviderError::new(error.to_string()))?;
        self.opds.download_book(&acquisition, temporary_directory).await.map(|download| CatalogDownload { download }).map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    pub fn add_custom_source(&self, title: &str, url: &str) -> Result<CatalogSource, CatalogProviderError> {
        let endpoint = self.endpoint_store()?.add_custom_endpoint(title, url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let source_id = opds_catalog_source_id(&endpoint.id);
        self.cache.ensure_source(&source_id, &endpoint.url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Ok(opds_source(&endpoint))
    }

    pub fn remove_custom_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogProviderError> {
        let endpoint_id = opds_endpoint_id(source_id)?;
        self.endpoint_store()?.remove_custom_endpoint(&endpoint_id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        self.cache.remove_source(source_id).map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    pub async fn refresh_sources(&self) -> Result<Vec<CatalogSource>, CatalogProviderError> {
        let registry = self.server.registry().await?;
        let mut store = self.endpoint_store()?;
        let previous = store.endpoints();
        store.replace_curated_registry(registry).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let current = store.endpoints();
        drop(store);
        let retained = current.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<HashSet<_>>();
        for endpoint in &current {
            self.cache.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        }
        for endpoint in previous {
            let source_id = opds_catalog_source_id(&endpoint.id);
            if !retained.contains(&source_id) {
                self.cache.remove_source(&source_id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
            }
        }
        Ok(current.into_iter().map(|endpoint| opds_source(&endpoint)).collect())
    }

    pub async fn discover_author_books(&self, request: &CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
        let mut identifier_searches = Vec::new();
        for identifier in &request.book_identifiers {
            identifier_searches.push((identifier, self.search_catalogs(&identifier.value).await?));
        }
        let name_search = self.search_catalogs(&request.author_name).await?;
        let identifier_searches = identifier_searches.iter().map(|(identifier, result)| (*identifier, result)).collect::<Vec<_>>();
        let result = resolve_author_discovery(request, &identifier_searches, &name_search)?;
        self.covers.remember_author_discovery(&result);
        Ok(result)
    }

    fn endpoint_store(&self) -> Result<std::sync::MutexGuard<'_, OpdsEndpointStore>, CatalogProviderError> {
        self.endpoints.lock().map_err(|_| CatalogProviderError::new("OPDS endpoint registry lock was poisoned"))
    }

    fn page_request(&self, source_id: &CatalogSourceId, locator: &str) -> Result<CatalogPageRequest, CatalogProviderError> {
        let state = self.cache.source_state(source_id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let known_publications = if state.is_some() { self.cache.known_page_publications(source_id, locator).map_err(|error| CatalogProviderError::new(error.to_string()))? } else { Vec::new() };
        Ok(CatalogPageRequest { locator: Some(locator.to_owned()), supported_languages: self.language_filter().supported().to_vec(), epoch: state.and_then(|state| state.provider_epoch), known_publications })
    }

    async fn search_cached_endpoint(&self, endpoint: &OpdsEndpoint, query: &str) -> Result<CatalogWideSearchOutcome, CatalogProviderError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        match self.search_cached_source(&source_id, endpoint, query).await {
            Ok(page) => Ok(CatalogWideSearchOutcome::Results { page }),
            Err(server_error) => {
                log::warn!(target: "catalog", "catalog server search unavailable; using direct OPDS: {server_error}");
                search_opds_catalogs(&self.opds, vec![endpoint.clone()], query).await.map(|mut groups| groups.pop().map(|group| group.outcome).unwrap_or(CatalogWideSearchOutcome::Unsupported))
            }
        }
    }

    async fn search_cached_source(&self, source_id: &CatalogSourceId, endpoint: &OpdsEndpoint, query: &str) -> Result<CatalogPage, CatalogProviderError> {
        let cached = self.cache.search(source_id, query).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let state = self.cache.source_state(source_id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let known_publications = cached.title_matches.iter().chain(&cached.author_matches).map(|publication| CatalogPublicationReference { identity: publication.publication.identity.clone(), sequence: publication.sequence }).collect();
        let request = CatalogSearchRequest { query: query.to_owned(), supported_languages: self.language_filter().supported().to_vec(), epoch: state.and_then(|state| state.provider_epoch), known_publications };
        let response = self.server.search_catalog(source_id, &request).await?;
        let matches = self.cache.apply_search_response(&endpoint.url, response).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Ok(search_page(source_id.clone(), query.to_owned(), matches))
    }
}

fn refresh_outcome_or_cached(refreshed: Result<CatalogWideSearchOutcome, CatalogProviderError>, cached: CatalogPage) -> CatalogWideSearchOutcome {
    match refreshed {
        Ok(outcome @ CatalogWideSearchOutcome::Results { .. }) => outcome,
        Ok(CatalogWideSearchOutcome::Unsupported) => CatalogWideSearchOutcome::Unsupported,
        Ok(CatalogWideSearchOutcome::TimedOut | CatalogWideSearchOutcome::Failed { .. }) | Err(_) => CatalogWideSearchOutcome::Results { page: cached },
    }
}

fn search_page(source_id: CatalogSourceId, query: String, matches: CatalogSearchMatches) -> CatalogPage {
    CatalogPage {
        source_id,
        freshness: CatalogFreshness::Current,
        title: format!("Search results for {query}"),
        locator: format!("catalog-search:{query}"),
        navigation: Vec::new(),
        publications: matches.title_matches.into_iter().map(|publication| publication.publication).collect(),
        groups: vec![CatalogGroup {
            title: "Author matches".to_owned(),
            page_locator: None,
            number_of_items: None,
            navigation: Vec::new(),
            publications: matches.author_matches.into_iter().map(|publication| publication.publication).collect(),
        }],
        facets: Vec::new(),
        pagination: CatalogPagination::default(),
        search: None,
    }
}

impl CatalogServerClient {
    pub fn new(base_url: &str) -> Result<Self, CatalogProviderError> {
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
        Ok(Self { http, base_url })
    }

    pub async fn registry(&self) -> Result<CatalogRegistry, CatalogProviderError> {
        self.get_wire(self.endpoint("api/opds/catalogs")?).await
    }

    pub async fn catalog_page(&self, source_id: &CatalogSourceId, request: &CatalogPageRequest) -> Result<CatalogPageResponse, CatalogProviderError> {
        self.post_wire(self.endpoint(&format!("api/opds/sources/{}/page", source_id.as_str()))?, request).await
    }

    pub async fn search_catalog(&self, source_id: &CatalogSourceId, request: &CatalogSearchRequest) -> Result<CatalogSearchResponse, CatalogProviderError> {
        self.post_wire(self.endpoint(&format!("api/opds/sources/{}/search", source_id.as_str()))?, request).await
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
        let response = self.http.get(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn post_wire<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(&self, url: Url, value: &T) -> Result<R, CatalogProviderError> {
        let bytes = wire::encode(value).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let response = self.http.post(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(bytes).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn response_wire<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, CatalogProviderError> {
        if !response.status().is_success() {
            let status = response.status();
            let detail = if response.content_length().is_some_and(|length| length <= 4096) { response.bytes().await.ok().and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()).filter(|value| !value.trim().is_empty()) } else { None };
            return Err(CatalogProviderError::new(detail.map_or_else(|| format!("catalog server returned HTTP {status}"), |detail| format!("catalog server returned HTTP {status}: {detail}"))));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        if !media_type.is_some_and(wire::is_current_media_type) {
            return Err(CatalogProviderError::new("catalog server returned an unsupported media type"));
        }
        if response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).is_some_and(|value| !value.eq_ignore_ascii_case("identity")) {
            return Err(CatalogProviderError::new("catalog server returned a compressed response"));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_SERVER_RESPONSE_BYTES) {
            return Err(CatalogProviderError::new("catalog server response is too large"));
        }
        let bytes = response.bytes().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| CatalogProviderError::new(error.to_string()))
    }
}

fn opds_import_request(request: &CatalogImportRequest, catalog: &OpdsCatalog) -> Result<(OpdsEndpointId, OpdsPublicationIdentity, OpdsDirectAcquisition), CatalogProviderError> {
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
    let requested_format = opds_book_format(request.book_format);
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

fn opds_book_format(format: CatalogBookFormat) -> OpdsBookFormat {
    match format {
        CatalogBookFormat::Epub => OpdsBookFormat::Epub,
        CatalogBookFormat::Pdf => OpdsBookFormat::Pdf,
        CatalogBookFormat::Mobi => OpdsBookFormat::Mobi,
        CatalogBookFormat::Fb2 => OpdsBookFormat::Fb2,
        CatalogBookFormat::Cbz => OpdsBookFormat::Cbz,
        CatalogBookFormat::WebPub => OpdsBookFormat::WebPub,
        CatalogBookFormat::M4b => OpdsBookFormat::M4b,
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
    use catalog_contract::{
        CatalogAuthorDiscoveryEvidence, CatalogExternalActionKind, CatalogLinkTarget, CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentifier, CatalogPublicationIdentifierScheme, CatalogPublicationIdentity,
        CatalogSearchDefinition, CatalogSearchField,
    };
    use image::{DynamicImage, ImageOutputFormat};
    use opds::{OpdsAcquisition, OpdsAuthorCredit, OpdsAuthorLink, OpdsCatalogLink, OpdsFacet, OpdsIndirectAcquisition, OpdsLinkTarget, OpdsPagination, OpdsPublication, OpdsUnsupportedAcquisition, OpdsVersion};
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn unsupported_catalog_languages_are_removed_before_search_planning() {
        let temporary = tempfile::tempdir().unwrap();
        let client = CatalogClient::with_supported_languages("http://127.0.0.1:9", temporary.path(), ["en-US"]).unwrap();
        let endpoint = |id: &str, language: &str| OpdsEndpoint { id: OpdsEndpointId::parse(id).unwrap(), title: id.to_owned(), description: String::new(), languages: vec![language.to_owned()], url: format!("https://{id}.example/opds") };

        let plans = client.supported_endpoint_sources([endpoint("english", "English"), endpoint("french", "French"), endpoint("multilingual", "Multilingual")]);

        assert_eq!(plans.into_iter().map(|(_, endpoint)| endpoint.id.as_str().to_owned()).collect::<Vec<_>>(), ["english", "multilingual"]);
    }

    #[test]
    fn unsupported_catalog_languages_remain_visible_for_explicit_browsing() {
        let temporary = tempfile::tempdir().unwrap();
        let client = CatalogClient::with_supported_languages("http://127.0.0.1:9", temporary.path(), ["en-US"]).unwrap();

        assert!(client.sources().iter().any(|source| source.id.as_str() == "opds-litteraturbanken"));
    }

    #[test]
    fn wide_search_refresh_failures_retain_the_cached_source_page() {
        let mut cached = search_page(CatalogSourceId::parse("opds-test").unwrap(), "book".to_owned(), CatalogSearchMatches::default());
        cached.freshness = CatalogFreshness::Stale;

        for refreshed in [Ok(CatalogWideSearchOutcome::TimedOut), Ok(CatalogWideSearchOutcome::Failed { message: "offline".to_owned() }), Err(CatalogProviderError::new("offline"))] {
            assert!(matches!(refresh_outcome_or_cached(refreshed, cached.clone()), CatalogWideSearchOutcome::Results { page } if page == cached));
        }
    }

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
            languages: vec!["en".to_owned()],
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
            freshness: opds::OpdsCatalogFreshness::Current,
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
        let mut forged = request.clone();
        forged.download_url = "https://attacker.example/forged.epub".to_owned();
        assert!(opds_import_request(&forged, &catalog).is_err());
    }

    #[test]
    fn catalog_facets_hide_provider_metadata_and_compact_supported_formats() {
        let link = |title: &str| OpdsCatalogLink { title: title.to_owned(), url: format!("https://example.com/{}", title.replace(' ', "-")), target: OpdsLinkTarget::Catalog, number_of_items: None, active: false };
        let catalog = OpdsCatalog {
            version: OpdsVersion::V1_2,
            freshness: opds::OpdsCatalogFreshness::Current,
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
        assert_eq!(page.facets[1].links.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), vec!["PDF", "EPUB", "MOBI/AZW"]);
    }

    #[test]
    fn provider_routing_validates_source_namespace_and_search_definitions() {
        assert_eq!(opds_endpoint_id(&CatalogSourceId::parse("opds-example").unwrap()).unwrap().as_str(), "example");
        assert!(opds_endpoint_id(&CatalogSourceId::parse("unknown").unwrap()).is_err());
        let endpoint = OpdsEndpoint { id: OpdsEndpointId::parse("example").unwrap(), title: "Example".to_owned(), description: "Example".to_owned(), languages: vec!["English".to_owned()], url: "https://catalog.example/root".to_owned() };
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
        let encoded = wire::encode(&advanced).unwrap();
        assert_eq!(wire::decode::<CatalogSearch>(&encoded, wire::MAX_DECODED_RESPONSE_BYTES).unwrap(), advanced);
    }

    #[tokio::test]
    async fn local_author_discovery_needs_no_catalog_provider_server() {
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
        };
        let request = CatalogAuthorDiscoveryRequest { author_name: "Alex Smith".to_owned(), book_identifiers: vec![CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "9780306406157".to_owned() }] };

        let result = discover_author_books_locally(&opds::OpdsClient::new().unwrap(), vec![endpoint], &request).await.unwrap();

        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].publication.title, "Another book by the identified author");
        assert!(matches!(result.matches[0].evidence, CatalogAuthorDiscoveryEvidence::BookIdentifier { .. }));
        assert_eq!(result.matches[1].publication.title, "Book by a different Alex Smith");
        assert_eq!(result.matches[1].evidence, CatalogAuthorDiscoveryEvidence::AuthorName);
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
                    languages: Vec::new(),
                    description: None,
                    image_url: Some(image_url.clone()),
                    actions: Vec::new(),
                },
                CatalogPublication {
                    identity: CatalogPublicationIdentity { edition_id: "broken".to_owned(), work_id: None },
                    title: "Broken".to_owned(),
                    authors: Vec::new(),
                    languages: Vec::new(),
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
        let cache = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert!(cache.cover(&source_id, &image_url).await.is_err());
        cache.remember_page(&page);
        let first = cache.cover(&source_id, &image_url).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(cache.cover(&source_id, &image_url).await.unwrap(), first);
        let restored = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert_eq!(restored.cover(&source_id, &image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(cache.cover(&source_id, &broken_url).await.is_err());
        assert!(cache.cover(&source_id, &broken_url).await.is_err());
        assert_eq!(failures.load(Ordering::SeqCst), 2);
        let cached_cover = std::fs::read_dir(temporary.path().join("covers")).unwrap().filter_map(Result::ok).map(|entry| entry.path()).find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("jpg")).unwrap();
        std::fs::remove_file(cached_cover).unwrap();
        let authorized_after_restart = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert!(authorized_after_restart.cover(&source_id, &image_url).await.is_err());
        authorized_after_restart.remember_page(&page);
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
                languages: Vec::new(),
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
        cache.remember_page(&page);
        let first = cache.cover_with(&source_id, image_url, server.catalog_cover(&source_id, image_url)).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(cache.cover_with(&source_id, image_url, server.catalog_cover(&source_id, image_url)).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server_task.abort();
        let restored = CatalogCoverCache::new(cache_path).unwrap();
        assert_eq!(restored.cover_with(&source_id, image_url, server.catalog_cover(&source_id, image_url)).await.unwrap(), first);
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
        let result = search_all_catalogs(&opds::OpdsClient::new().unwrap(), opds::built_in_opds_endpoints(), "Carmen").await.unwrap();
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

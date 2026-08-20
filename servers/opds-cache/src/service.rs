use super::{
    canonicalize_opds_catalog_locator, canonicalize_page_layout_locators, fmt, materialize_page, opds_page, opds_source, validate_opds_catalog_locator, validated_opds_import, CatalogCache, CatalogImportRequest, CatalogInventory,
    CatalogLanguageFilter, CatalogPage, CatalogPageRequest, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogRecommendationRequest, CatalogRecommendationResponse, CatalogSearchRequest,
    CatalogSearchResponse, CatalogService, CatalogServiceError, CatalogSource, CatalogSourceId, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CrawlProgress, DownloadedOpdsBook, HashMap, HashSet, Instant, OpdsEndpoint,
    OpdsSearch, PathBuf, BROWSER_UPSTREAM_TIMEOUT,
};
use crate::http::public::filter_browser_page;

impl CatalogService {
    pub(crate) async fn begin_crawl_progress(&self, source_id: &CatalogSourceId) {
        self.state.crawl_progress.lock().await.insert(source_id.clone(), CrawlProgress { started_at: Instant::now(), pages_fetched: 0, publications_seen: 0 });
    }

    pub(crate) async fn record_crawl_page(&self, source_id: &CatalogSourceId, publications: u64) {
        if let Some(progress) = self.state.crawl_progress.lock().await.get_mut(source_id) {
            progress.pages_fetched = progress.pages_fetched.saturating_add(1);
            progress.publications_seen = progress.publications_seen.saturating_add(publications);
        }
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

    pub(crate) async fn cache_page_on_demand(&self, source_id: &CatalogSourceId, endpoint: &OpdsEndpoint, locator: &str) -> Result<(), CatalogServiceError> {
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
        self.cache_write_call_named("crawl_page_write", move |cache| cache.put_page(&cache_source, &root, &cached_page, None, None)).await?;
        self.remember_browser_covers(source_id, &page).await;
        self.state.extraction_notify.notify_one();
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
}

pub(crate) fn validate_query(query: &str) -> Result<(), CatalogServiceError> {
    if query.trim().is_empty() || query.len() > 512 || query.chars().any(char::is_control) {
        return Err(CatalogServiceError("catalog search query must be non-empty bounded text without control characters".to_owned()));
    }
    Ok(())
}

pub(crate) fn retain_delta_payloads<'a>(
    payloads: &mut Vec<CatalogPublicationPayload>, returned: impl IntoIterator<Item = &'a CatalogPublicationReference>, request_epoch: Option<&str>, response_epoch: &str, known_publications: Vec<CatalogPublicationReference>,
) {
    let returned = returned.into_iter().map(|publication| publication.identity.clone()).collect::<HashSet<_>>();
    let known = if request_epoch == Some(response_epoch) { known_publications.into_iter().map(|publication| (publication.identity, publication.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
    payloads.retain(|payload| returned.contains(&payload.publication.identity) && known.get(&payload.publication.identity) != Some(&payload.sequence));
}

pub(crate) fn cache_error(error: impl fmt::Display) -> CatalogServiceError {
    CatalogServiceError(error.to_string())
}

pub(crate) fn default_cache_path() -> PathBuf {
    std::env::temp_dir().join("bokheim-opds-cache.db")
}

pub(crate) fn default_cover_cache_directory() -> PathBuf {
    std::env::temp_dir().join(format!("bokheim-opds-covers-{}", std::process::id()))
}

impl fmt::Display for CatalogServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogServiceError {}

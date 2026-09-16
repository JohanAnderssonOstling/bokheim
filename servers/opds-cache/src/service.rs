use super::{
    canonicalize_opds_catalog_locator, canonicalize_page_layout_locators, fmt, materialize_page, opds_page, opds_source, validate_opds_catalog_locator, validated_opds_import, CatalogFreshness, CatalogGroup, CatalogImportRequest,
    CatalogInventory, CatalogLanguageFilter, CatalogPage, CatalogPageRequest, CatalogPageResponse, CatalogPageWrite, CatalogPagination, CatalogPublication, CatalogPublicationIdentity, CatalogPublicationPayload, CatalogPublicationReference,
    CatalogSearchRequest, CatalogSearchResponse, CatalogService, CatalogServiceError, CatalogSource, CatalogSourceId, CatalogStore, CatalogSubjectBrowseIndexResponse, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse,
    CatalogSubjectPublicationPayloadRequest, CatalogSubjectPublicationPayloadResponse, CrawlProgress, DownloadedOpdsBook, HashMap, HashSet, Instant, PathBuf, Url, BROWSER_UPSTREAM_TIMEOUT,
};

const PUBLIC_COVER_EPOCH_SUFFIX: &str = "+bokheim-covers-v1";

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
    pub async fn page_response(&self, source_id: &CatalogSourceId, mut request: CatalogPageRequest) -> Result<CatalogPageResponse, CatalogServiceError> {
        if !internalize_transport_epoch(&mut request.epoch) {
            request.known_publications.clear();
        }
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = canonicalize_opds_catalog_locator(&endpoint, request.locator.as_deref().unwrap_or(&endpoint.url)).map_err(|error| CatalogServiceError(error.to_string()))?;
        let source = opds_source(&endpoint);
        let filter = CatalogLanguageFilter::new(&request.supported_languages);
        if !filter.source_is_multilingual(&source) {
            let cache_source = source_id.clone();
            let cache_locator = locator.clone();
            let epoch = request.epoch.clone();
            let known = request.known_publications.clone();
            let mut response =
                self.store_call(move |store| store.page_response(&cache_source, &cache_locator, epoch.as_deref(), &known)).await?.ok_or_else(|| CatalogServiceError("catalog has no active snapshot for this page".to_owned()))?;
            canonicalize_page_layout_locators(&endpoint, &mut response.page)?;
            self.publicize_payload_covers(&mut response.payloads).await;
            publicize_transport_epoch(&mut response.epoch);
            return Ok(response);
        }

        let cache_source = source_id.clone();
        let cache_locator = locator.clone();
        let mut response = self.store_call(move |store| store.page_response(&cache_source, &cache_locator, None, &[])).await?.ok_or_else(|| CatalogServiceError("catalog has no active snapshot for this page".to_owned()))?;
        let supported = response.payloads.iter().filter(|payload| filter.publication_supported(&payload.publication)).map(|payload| payload.publication.identity.clone()).collect::<HashSet<_>>();
        response.page.publications.retain(|publication| supported.contains(&publication.identity));
        for group in &mut response.page.groups {
            group.publications.retain(|publication| supported.contains(&publication.identity));
        }
        retain_delta_payloads(&mut response.payloads, response.page.publications.iter().chain(response.page.groups.iter().flat_map(|group| group.publications.iter())), request.epoch.as_deref(), &response.epoch, request.known_publications);
        canonicalize_page_layout_locators(&endpoint, &mut response.page)?;
        self.publicize_payload_covers(&mut response.payloads).await;
        publicize_transport_epoch(&mut response.epoch);
        Ok(response)
    }

    pub async fn search_response(&self, source_id: &CatalogSourceId, mut request: CatalogSearchRequest) -> Result<CatalogSearchResponse, CatalogServiceError> {
        if !internalize_transport_epoch(&mut request.epoch) {
            request.known_publications.clear();
        }
        validate_query(&request.query)?;
        let endpoint = self.public_endpoint(source_id).await?;
        let source = opds_source(&endpoint);
        let filter = CatalogLanguageFilter::new(&request.supported_languages);
        let cache_source = source_id.clone();
        if !self.store_call(move |store| store.has_pages(&cache_source)).await? {
            return Err(CatalogServiceError("catalog has no active snapshot".to_owned()));
        }
        if !filter.source_is_multilingual(&source) {
            let cache_source = source_id.clone();
            let query = request.query.clone();
            let epoch = request.epoch.clone();
            let known = request.known_publications.clone();
            let mut response = self.store_call(move |cache| cache.search_response(&cache_source, &query, epoch.as_deref(), &known)).await?.ok_or_else(|| CatalogServiceError("catalog has not been crawled".to_owned()))?;
            self.publicize_payload_covers(&mut response.payloads).await;
            publicize_transport_epoch(&mut response.epoch);
            return Ok(response);
        }
        let cache_source = source_id.clone();
        let query = request.query.clone();
        let mut response = self.store_call(move |cache| cache.search_response(&cache_source, &query, None, &[])).await?.ok_or_else(|| CatalogServiceError("catalog has not been crawled".to_owned()))?;
        let supported = response.payloads.iter().filter(|payload| filter.publication_supported(&payload.publication)).map(|payload| payload.publication.identity.clone()).collect::<HashSet<_>>();
        response.search.title_matches.retain(|item| supported.contains(&item.identity));
        response.search.author_matches.retain(|item| supported.contains(&item.identity));
        response.search.toc_matches.retain(|item| supported.contains(&item.identity));
        retain_delta_payloads(&mut response.payloads, response.search.title_matches.iter().chain(&response.search.author_matches).chain(&response.search.toc_matches), request.epoch.as_deref(), &response.epoch, request.known_publications);
        self.publicize_payload_covers(&mut response.payloads).await;
        publicize_transport_epoch(&mut response.epoch);
        Ok(response)
    }

    /// Browser clients receive a fully materialized view of the same
    /// authoritative snapshot used by native clients. A cold browser request
    /// may seed the requested page once; subsequent requests use the durable
    /// snapshot and never contact the upstream catalogue.
    pub async fn browser_page(&self, source_id: &CatalogSourceId, request: CatalogPageRequest) -> Result<CatalogPage, CatalogServiceError> {
        let response = match self.page_response(source_id, request.clone()).await {
            Ok(response) => response,
            Err(_) => self.cache_page_on_demand(source_id, request).await?,
        };
        let payloads = publication_payload_map(response.payloads);
        materialize_page(response.page, |reference| resolve_payload(&payloads, reference)).ok_or_else(|| CatalogServiceError("catalogue server could not materialize its page snapshot".to_owned()))
    }

    async fn cache_page_on_demand(&self, source_id: &CatalogSourceId, request: CatalogPageRequest) -> Result<CatalogPageResponse, CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let locator = canonicalize_opds_catalog_locator(&endpoint, request.locator.as_deref().unwrap_or(&endpoint.url)).map_err(|error| CatalogServiceError(error.to_string()))?;
        let catalog = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.opds.fetch_catalog_fresh(&locator))
            .await
            .map_err(|_| CatalogServiceError("catalogue page did not respond before the browser timeout".to_owned()))?
            .map_err(|error| CatalogServiceError(error.to_string()))?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let (mut page, provider_paginated) = self.compact_provider_pagination(&endpoint, &root_url, source_id, opds_page(source_id.clone(), catalog)).await?;
        let cache_source = source_id.clone();
        page = self
            .store_call(move |store| {
                store.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                Ok(page)
            })
            .await?;
        self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
        let write = CatalogPageWrite { page: page.clone(), etag: None, last_modified: None, provider_paginated };
        let prepared = self.store_call(move |_| CatalogStore::prepare_page_writes(vec![write])).await?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.store_write_call_named("browser_page_seed", move |store| store.put_prepared_pages(&cache_source, &cache_root, &prepared)).await?;
        self.state.extraction_notify.notify_one();
        self.page_response(source_id, request).await
    }

    pub async fn browser_search(&self, source_id: &CatalogSourceId, request: CatalogSearchRequest) -> Result<CatalogPage, CatalogServiceError> {
        let response = self.search_response(source_id, request).await?;
        materialize_search_page(response)
    }
    pub async fn subject_index(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseIndexResponse, CatalogServiceError> {
        let started = Instant::now();
        let request = request.clone();
        let mut response = self.store_call(move |cache| cache.browse_subject_index(&request)).await?;
        publicize_transport_epoch(&mut response.epoch);
        tracing::info!(returned = response.entry_ids.len(), elapsed_ms = started.elapsed().as_millis(), "catalogue subject index completed");
        Ok(response)
    }

    pub async fn browse_subjects(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogServiceError> {
        let started = Instant::now();
        let request = request.clone();
        let mut response = self.store_call(move |cache| cache.browse_subjects(&request)).await?;
        self.publicize_sourced_covers(&mut response.publications).await;
        publicize_transport_epoch(&mut response.epoch);
        tracing::info!(returned = response.publications.len(), elapsed_ms = started.elapsed().as_millis(), "catalogue browser subject browse completed");
        Ok(response)
    }

    pub async fn subject_publication_payloads(&self, request: &CatalogSubjectPublicationPayloadRequest) -> Result<CatalogSubjectPublicationPayloadResponse, CatalogServiceError> {
        let started = Instant::now();
        let mut request = request.clone();
        let mut epoch = Some(request.epoch);
        if !internalize_transport_epoch(&mut epoch) {
            return Err(CatalogServiceError("catalogue publication epoch is stale".to_owned()));
        }
        request.epoch = epoch.expect("successful epoch internalization preserves an epoch");
        let requested = request.entry_ids.len();
        let mut response = self.store_call(move |cache| cache.subject_publication_payloads(&request)).await?;
        self.publicize_subject_covers(&mut response.payloads).await;
        publicize_transport_epoch(&mut response.epoch);
        tracing::info!(requested, returned = response.payloads.len(), elapsed_ms = started.elapsed().as_millis(), "catalogue subject publication payloads completed");
        Ok(response)
    }

    pub async fn browser_sources(&self) -> Vec<CatalogSource> {
        self.state.endpoints.read().await.iter().map(opds_source).collect()
    }

    pub async fn inventory(&self) -> Result<CatalogInventory, CatalogServiceError> {
        self.store_call(CatalogStore::inventory).await
    }

    /// Downloads only an acquisition that is present on a freshly fetched
    /// page from the registered catalogue origin. This deliberately accepts
    /// structured identity data rather than an arbitrary proxy URL.
    pub async fn acquisition(&self, source_id: &CatalogSourceId, request: &CatalogImportRequest) -> Result<DownloadedOpdsBook, CatalogServiceError> {
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

fn internalize_transport_epoch(epoch: &mut Option<String>) -> bool {
    let Some(public_epoch) = epoch.take() else {
        return false;
    };
    let Some(internal_epoch) = public_epoch.strip_suffix(PUBLIC_COVER_EPOCH_SUFFIX) else {
        return false;
    };
    *epoch = Some(internal_epoch.to_owned());
    true
}

fn publicize_transport_epoch(epoch: &mut String) {
    epoch.push_str(PUBLIC_COVER_EPOCH_SUFFIX);
}

fn publication_payload_map(payloads: Vec<CatalogPublicationPayload>) -> HashMap<CatalogPublicationIdentity, (u64, CatalogPublication)> {
    payloads.into_iter().map(|payload| (payload.publication.identity.clone(), (payload.sequence, payload.publication))).collect()
}

fn resolve_payload(payloads: &HashMap<CatalogPublicationIdentity, (u64, CatalogPublication)>, reference: &CatalogPublicationReference) -> Option<CatalogPublication> {
    payloads.get(&reference.identity).filter(|(sequence, _)| *sequence == reference.sequence).map(|(_, publication)| publication.clone())
}

fn materialize_search_page(response: CatalogSearchResponse) -> Result<CatalogPage, CatalogServiceError> {
    let payloads = publication_payload_map(response.payloads);
    let resolve = |references: &[CatalogPublicationReference]| references.iter().map(|reference| resolve_payload(&payloads, reference)).collect::<Option<Vec<_>>>();
    let title_matches = resolve(&response.search.title_matches).ok_or_else(|| CatalogServiceError("catalogue server could not materialize its search title matches".to_owned()))?;
    let author_matches = resolve(&response.search.author_matches).ok_or_else(|| CatalogServiceError("catalogue server could not materialize its search author matches".to_owned()))?;
    let toc_matches = resolve(&response.search.toc_matches).ok_or_else(|| CatalogServiceError("catalogue server could not materialize its search table-of-contents matches".to_owned()))?;
    Ok(CatalogPage {
        source_id: response.search.source_id,
        freshness: CatalogFreshness::Current,
        title: format!("Search results for {}", response.search.query),
        locator: format!("catalog-search:{}", response.search.query),
        navigation: Vec::new(),
        publications: title_matches,
        groups: vec![
            CatalogGroup { title: "Author matches".to_owned(), page_locator: None, number_of_items: None, navigation: Vec::new(), publications: author_matches },
            CatalogGroup { title: "Table of contents matches".to_owned(), page_locator: None, number_of_items: None, navigation: Vec::new(), publications: toc_matches },
        ],
        facets: Vec::new(),
        pagination: CatalogPagination::default(),
    })
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

pub(crate) fn store_error(error: impl fmt::Display) -> CatalogServiceError {
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

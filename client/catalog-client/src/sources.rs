use super::*;

impl CatalogClient {
    pub fn sources(&self) -> Vec<CatalogSource> {
        self.catalog_sources()
    }

    /// Returns local SQLite matches for one source without network I/O.
    pub async fn cached_search(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogPage, CatalogProviderError> {
        let registration = self.registration_for_source(source_id)?;
        let page = self.cached_search_page(&registration.source, query)?;
        self.covers.remember_page(&page);
        Ok(page)
    }

    fn cached_search_page(&self, source: &CatalogSource, query: &str) -> Result<CatalogPage, CatalogProviderError> {
        let matches = self.store.search(&source.id, query).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let mut page = search_page(source.id.clone(), query.trim().to_owned(), matches);
        page.freshness = CatalogFreshness::Stale;
        self.prepare_page(source, &mut page);
        Ok(page)
    }

    fn registration_for_source(&self, source_id: &CatalogSourceId) -> Result<CatalogSourceRegistration, CatalogProviderError> {
        self.registry.lock().map_err(|_| CatalogProviderError::new("catalogue server registry lock was poisoned"))?.registration(source_id).ok_or_else(|| CatalogProviderError::new("catalog source is no longer available"))
    }

    pub async fn page(&self, source_id: &CatalogSourceId, locator: Option<&str>) -> Result<CatalogPage, CatalogProviderError> {
        let registration = self.registration_for_source(source_id)?;
        let locator = locator.unwrap_or(&registration.root_locator).to_owned();
        registry::validate_locator(&locator)?;
        let request = self.page_request(source_id, &locator)?;
        match self.server.catalog_page(source_id, &request).await {
            Ok(response) => {
                let mut page = self.store.apply_page_response(&self.cache, &registration.root_locator, response).map_err(|error| CatalogProviderError::new(error.to_string()))?;
                self.prepare_page(&registration.source, &mut page);
                self.covers.remember_page(&page);
                Ok(page)
            }
            Err(error) => {
                log::warn!(target: "catalog", "catalog server page unavailable; using the local cache: {error}");
                if let Some(cached) = self.cache.page(source_id, &locator).map_err(|cache_error| CatalogProviderError::new(cache_error.to_string()))? {
                    let mut page = cached.page;
                    page.freshness = CatalogFreshness::Stale;
                    self.prepare_page(&registration.source, &mut page);
                    self.covers.remember_page(&page);
                    Ok(page)
                } else {
                    Err(error)
                }
            }
        }
    }

    pub async fn search(&self, source_id: &CatalogSourceId, _search: &CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPage, CatalogProviderError> {
        let registration = self.registration_for_source(source_id)?;
        let mut page = self.search_cached_source(source_id, &registration, &query.query).await?;
        self.prepare_page(&registration.source, &mut page);
        self.covers.remember_page(&page);
        Ok(page)
    }

    pub async fn refresh_sources(&self) -> Result<Vec<CatalogSource>, CatalogProviderError> {
        let registry = self.server.registry().await?;
        let mut store = self.registry.lock().map_err(|_| CatalogProviderError::new("catalogue server registry lock was poisoned"))?;
        let previous = store.registrations();
        store.replace(&self.store, registry)?;
        let current = store.registrations();
        drop(store);
        let retained = current.iter().map(|registration| registration.source.id.clone()).collect::<HashSet<_>>();
        for registration in &current {
            self.cache.ensure_source(&registration.source.id, &registration.root_locator).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        }
        for registration in previous {
            if !retained.contains(&registration.source.id) {
                self.cache.remove_source(&registration.source.id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
            }
        }
        Ok(current.into_iter().map(|registration| registration.source).collect())
    }

    fn page_request(&self, source_id: &CatalogSourceId, locator: &str) -> Result<CatalogPageRequest, CatalogProviderError> {
        let state = self.cache.source_state(source_id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let known_publications = if state.is_some() { self.store.known_page_publications(source_id, locator).map_err(|error| CatalogProviderError::new(error.to_string()))? } else { Vec::new() };
        Ok(CatalogPageRequest { locator: Some(locator.to_owned()), supported_languages: self.language_filter().supported().to_vec(), epoch: state.and_then(|state| state.provider_epoch), known_publications })
    }

    async fn search_cached_source(&self, source_id: &CatalogSourceId, registration: &CatalogSourceRegistration, query: &str) -> Result<CatalogPage, CatalogProviderError> {
        let cached = self.store.search(source_id, query).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let state = self.cache.source_state(source_id).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let known_publications = cached
            .title_matches
            .iter()
            .chain(&cached.author_matches)
            .chain(&cached.toc_matches)
            .map(|publication| CatalogPublicationReference { identity: publication.publication.identity.clone(), sequence: publication.sequence })
            .collect();
        let request = CatalogSearchRequest { query: query.to_owned(), supported_languages: self.language_filter().supported().to_vec(), epoch: state.and_then(|state| state.provider_epoch), known_publications };
        let response = self.server.search_catalog(source_id, &request).await?;
        let matches = self.store.apply_search_response(&self.cache, &registration.root_locator, response).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Ok(search_page(source_id.clone(), query.to_owned(), matches))
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
        groups: vec![
            CatalogGroup { title: "Author matches".to_owned(), page_locator: None, number_of_items: None, navigation: Vec::new(), publications: matches.author_matches.into_iter().map(|publication| publication.publication).collect() },
            CatalogGroup {
                title: "Table of contents matches".to_owned(),
                page_locator: None,
                number_of_items: None,
                navigation: Vec::new(),
                publications: matches.toc_matches.into_iter().map(|publication| publication.publication).collect(),
            },
        ],
        facets: Vec::new(),
        pagination: CatalogPagination::default(),
        search: None,
    }
}

impl CatalogServerClient {
    async fn registry(&self) -> Result<CatalogSourceRegistry, CatalogProviderError> {
        self.get_wire(self.endpoint("api/catalog/sources")?).await
    }

    async fn catalog_page(&self, source_id: &CatalogSourceId, request: &CatalogPageRequest) -> Result<CatalogPageResponse, CatalogProviderError> {
        self.post_wire(self.endpoint(&format!("api/opds/sources/{}/page", source_id.as_str()))?, request).await
    }

    async fn search_catalog(&self, source_id: &CatalogSourceId, request: &CatalogSearchRequest) -> Result<CatalogSearchResponse, CatalogProviderError> {
        self.post_wire(self.endpoint(&format!("api/opds/sources/{}/search", source_id.as_str()))?, request).await
    }
}

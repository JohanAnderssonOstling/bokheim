use super::{
    cache_error, canonicalize_opds_catalog_locator, catalog_crawls_disabled, duration_millis, opds_catalog_source_id, opds_page, prepare_catalog_subject_assignments, validate_opds_catalog_locator, AdaptiveCrawlWriteBatch, Arc,
    CatalogCache, CatalogCacheError, CatalogCrawlGeneration, CatalogLinkTarget, CatalogMetadataBackfillIndexedUpdate, CatalogMetadataBackfillWriteTimings, CatalogPage, CatalogPageWrite, CatalogService, CatalogServiceError, CatalogSourceId,
    DiskCrawlFrontier, Duration, HashSet, Instant, Mutex, OpdsConditionalCatalog, OpdsEndpoint, OpdsHttpValidators, Url, CRAWL_ACTIVATION_BATCH, CRAWL_CYCLE, CRAWL_PAUSE_SENTINEL, EPUB_EXTRACTION_SWEEP_INTERVAL,
    EXTRACTION_RECONCILIATION_BATCH, INACTIVE_GENERATION_CLEANUP_BATCH, METADATA_BACKFILL_BATCH_SIZE, PRIORITY_CRAWL_SOURCE, REGISTRY_REFRESH_INTERVAL,
};
use crate::extraction::is_synthetic_cover_key;
impl CatalogService {
    pub async fn crawl_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogServiceError> {
        let endpoint = self.public_endpoint(source_id).await?;
        let lock = {
            let mut locks = self.state.crawl_locks.lock().await;
            locks.entry(source_id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
        };
        let _crawl = lock.lock().await;
        self.crawl_endpoint(endpoint).await
    }

    pub(crate) async fn source_update_lock(&self, source_id: &CatalogSourceId) -> Arc<Mutex<()>> {
        let mut locks = self.state.source_update_locks.lock().await;
        locks.entry(source_id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub(crate) async fn cache_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogCache) -> Result<T, CatalogCacheError> + Send + 'static,
    {
        let cache = self.state.cache.clone();
        tokio::task::spawn_blocking(move || operation(&cache)).await.map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}")))?.map_err(cache_error)
    }

    pub(crate) async fn cache_write_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogCache) -> Result<T, CatalogCacheError> + Send + 'static,
    {
        self.cache_write_call_named("other", operation).await
    }

    pub(crate) async fn cache_write_call_named<T, F>(&self, name: &'static str, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogCache) -> Result<T, CatalogCacheError> + Send + 'static,
    {
        let wait_started = Instant::now();
        let _writer = self.state.cache_write_lock.lock().await;
        let wait_ms = duration_millis(wait_started.elapsed());
        let hold_started = Instant::now();
        let cache = self.state.cache.clone();
        let result = tokio::task::spawn_blocking(move || operation(&cache)).await.map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}"))).and_then(|result| result.map_err(cache_error));
        let hold_ms = duration_millis(hold_started.elapsed());
        let failures = u64::from(result.is_err());
        if let Ok(mut metrics) = self.state.writer_metrics.lock() {
            let metric = metrics.entry(name.to_owned()).or_default();
            metric.calls = metric.calls.saturating_add(1);
            metric.failures = metric.failures.saturating_add(failures);
            metric.wait_ms = metric.wait_ms.saturating_add(wait_ms);
            metric.hold_ms = metric.hold_ms.saturating_add(hold_ms);
        }
        result
    }

    pub(crate) async fn finish_metadata_backfill_indexed_batch_timed(&self, updates: Vec<CatalogMetadataBackfillIndexedUpdate>) -> Result<(u64, CatalogMetadataBackfillWriteTimings), CatalogServiceError> {
        let prepared_assignments = updates.iter().map(|update| prepare_catalog_subject_assignments(&update.subjects)).collect::<Vec<_>>();
        let wait_started = Instant::now();
        let _writer = self.state.cache_write_lock.lock().await;
        let writer_wait_ms = duration_millis(wait_started.elapsed());
        let hold_started = Instant::now();
        let cache = self.state.cache.clone();
        let result = tokio::task::spawn_blocking(move || cache.finish_metadata_backfill_indexed_batch_timed(&updates, &prepared_assignments))
            .await
            .map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}")))
            .and_then(|result| result.map_err(cache_error));
        let hold_ms = duration_millis(hold_started.elapsed());
        if let Ok(mut metrics) = self.state.writer_metrics.lock() {
            let metric = metrics.entry("backfill_finish".to_owned()).or_default();
            metric.calls = metric.calls.saturating_add(1);
            metric.failures = metric.failures.saturating_add(u64::from(result.is_err()));
            metric.wait_ms = metric.wait_ms.saturating_add(writer_wait_ms);
            metric.hold_ms = metric.hold_ms.saturating_add(hold_ms);
        }
        let timings = result?;
        Ok((writer_wait_ms, timings))
    }

    pub(crate) async fn flush_replacement_crawl_batch(&self, crawl: &CatalogCrawlGeneration, source_id: &CatalogSourceId, batch: &mut AdaptiveCrawlWriteBatch) -> Result<(), CatalogServiceError> {
        if batch.pages.is_empty() {
            return Ok(());
        }
        let pages = batch.take_pages();
        let publication_counts = pages.iter().map(|write| page_publication_count(&write.page)).collect::<Vec<_>>();
        let cover_urls = pages.iter().flat_map(|write| page_cover_urls(&write.page)).collect::<Vec<_>>();
        let prepared = self.cache_call(move |_| CatalogCache::prepare_page_writes(pages)).await?;
        let cache_crawl = crawl.clone();
        let started = Instant::now();
        self.cache_write_call_named("crawl_page_write", move |cache| cache.stage_prepared_pages(&cache_crawl, &prepared)).await?;
        batch.record_commit(started.elapsed());
        for publications in publication_counts {
            self.record_crawl_page(source_id, publications).await;
        }
        self.state.cover_pipeline.enqueue(source_id.clone(), cover_urls).await;
        tokio::task::yield_now().await;
        Ok(())
    }

    pub(crate) async fn flush_incremental_crawl_batch(&self, source_id: &CatalogSourceId, source_url: &str, batch: &mut AdaptiveCrawlWriteBatch) -> Result<(), CatalogServiceError> {
        if batch.pages.is_empty() {
            return Ok(());
        }
        let pages = batch.take_pages();
        // Re-read enrichment only after acquiring the source lock. This keeps
        // a concurrent EPUB result from being overwritten by buffered feed data.
        let update_lock = self.source_update_lock(source_id).await;
        let _update = update_lock.lock().await;
        let mut enriched_pages = Vec::with_capacity(pages.len());
        for mut write in pages {
            let cache_source = source_id.clone();
            let mut page = write.page;
            page = self
                .cache_call(move |cache| {
                    cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                    Ok(page)
                })
                .await?;
            self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
            write.page = page;
            enriched_pages.push(write);
        }
        let pages = enriched_pages;
        let publication_counts = pages.iter().map(|write| page_publication_count(&write.page)).collect::<Vec<_>>();
        let cover_urls = pages.iter().flat_map(|write| page_cover_urls(&write.page)).collect::<Vec<_>>();
        let prepared = self.cache_call(move |_| CatalogCache::prepare_page_writes(pages)).await?;
        let cache_source = source_id.clone();
        let cache_root = source_url.to_owned();
        let started = Instant::now();
        self.cache_write_call_named("crawl_page_write", move |cache| cache.put_prepared_pages(&cache_source, &cache_root, &prepared)).await?;
        batch.record_commit(started.elapsed());
        drop(_update);
        for publications in publication_counts {
            self.record_crawl_page(source_id, publications).await;
        }
        self.state.cover_pipeline.enqueue(source_id.clone(), cover_urls).await;
        tokio::task::yield_now().await;
        Ok(())
    }

    pub(crate) async fn cleanup_inactive_generations(&self, source_id: CatalogSourceId) {
        loop {
            let cache_source = source_id.clone();
            match self.cache_write_call_named("cleanup", move |cache| cache.cleanup_inactive_generation_batch(&cache_source, INACTIVE_GENERATION_CLEANUP_BATCH)).await {
                Ok(true) => tokio::time::sleep(Duration::from_millis(10)).await,
                Ok(false) => return,
                Err(error) => {
                    tracing::warn!(source = source_id.as_str(), %error, "could not finish bounded inactive-generation cleanup");
                    return;
                }
            }
        }
    }

    pub(crate) async fn reconcile_active_epub_extractions(&self, source_id: CatalogSourceId) -> Result<(), CatalogServiceError> {
        loop {
            let cache_source = source_id.clone();
            let more = self.cache_write_call_named("epub_reconcile", move |cache| cache.reconcile_active_epub_extractions_batch(&cache_source, EXTRACTION_RECONCILIATION_BATCH)).await?;
            if !more {
                self.state.extraction_notify.notify_one();
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    }

    pub(crate) async fn blocking_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(CatalogCache) -> Result<T, CatalogServiceError> + Send + 'static,
    {
        let cache = self.state.cache.clone();
        tokio::task::spawn_blocking(move || operation(cache)).await.map_err(|error| CatalogServiceError(format!("catalog cache worker failed: {error}")))?
    }

    pub(crate) async fn crawl_endpoint(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.cache_write_call_named("crawl_setup", move |cache| cache.ensure_source(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not reconcile crawl source: {error}")))?;
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
        let crawl = self.cache_write_call_named("crawl_begin", move |cache| cache.begin_crawl(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not start replacement crawl: {error}")))?;
        self.begin_crawl_progress(&source_id).await;
        let mut frontier = DiskCrawlFrontier::new(&endpoint.url, self.state.max_crawl_pages)?;
        let mut write_batch = AdaptiveCrawlWriteBatch::default();
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
                    // A cached leaf cannot lead us to any newly discovered
                    // catalogue pages. Re-stage it without contacting the
                    // upstream server so it remains part of the replacement
                    // generation, while avoiding recurring requests for the
                    // (typically large) book-only collections.
                    if let Some(cached_page) = cached.as_ref() {
                        if internal_page_links(&root_url, &cached_page.page)?.is_empty() {
                            let cache_source = source_id.clone();
                            let mut page = cached_page.page.clone();
                            page = self
                                .cache_call(move |cache| {
                                    cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                                    Ok(page)
                                })
                                .await?;
                            let publications = page_publication_count(&page) as usize;
                            let bytes = wire::encode(&page).map_err(|error| CatalogServiceError(error.to_string()))?.len();
                            write_batch.push(CatalogPageWrite { page, etag: cached_page.etag.clone(), last_modified: cached_page.last_modified.clone() }, publications, bytes);
                            if write_batch.reached_capacity() {
                                self.flush_replacement_crawl_batch(&crawl, &source_id, &mut write_batch).await?;
                            }
                            continue;
                        }
                    }
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
                    let publications = page_publication_count(&page) as usize;
                    let bytes = wire::encode(&page).map_err(|error| CatalogServiceError(error.to_string()))?.len();
                    write_batch.push(CatalogPageWrite { page, etag: validators.etag, last_modified: validators.last_modified }, publications, bytes);
                    if write_batch.reached_capacity() {
                        self.flush_replacement_crawl_batch(&crawl, &source_id, &mut write_batch).await?;
                    }
                }
            }
            self.flush_replacement_crawl_batch(&crawl, &source_id, &mut write_batch).await?;
            loop {
                let cache_crawl = crawl.clone();
                let more = self.cache_write_call_named("crawl_activation", move |cache| cache.prepare_crawl_activation_batch(&cache_crawl, CRAWL_ACTIVATION_BATCH)).await?;
                if !more {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let cache_crawl = crawl.clone();
            self.cache_write_call_named("crawl_finalize", move |cache| cache.finalize_crawl_activation(&cache_crawl)).await?;
            self.reconcile_active_epub_extractions(source_id.clone()).await?;
            self.cleanup_inactive_generations(source_id.clone()).await;
            Ok::<(), CatalogServiceError>(())
        }
        .await;
        if let Err(error) = &result {
            let message = error.to_string();
            let cache_crawl = crawl.clone();
            let _ = self.cache_write_call_named("crawl_abandon", move |cache| cache.abandon_crawl_with_error(&cache_crawl, Some(&message))).await;
            self.cleanup_inactive_generations(source_id.clone()).await;
        }
        result
    }

    pub(crate) async fn crawl_endpoint_incrementally(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.cache_write_call_named("crawl_begin_incremental", move |cache| cache.begin_incremental_crawl(&cache_source, &cache_root)).await?;
        self.begin_crawl_progress(&source_id).await;
        let mut frontier = DiskCrawlFrontier::new(&endpoint.url, self.state.max_crawl_pages)?;
        let mut fetched = 0usize;
        let mut write_batch = AdaptiveCrawlWriteBatch::default();
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
                        self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
                        return Ok(false);
                    }
                    fetched += 1;
                    let catalog = self.state.opds.fetch_catalog_fresh(&locator).await.map_err(|error| CatalogServiceError(error.to_string()))?;
                    validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
                    let page = opds_page(source_id.clone(), catalog);
                    frontier.enqueue(internal_page_links(&root_url, &page)?)?;
                    let publications = page_publication_count(&page) as usize;
                    let bytes = wire::encode(&page).map_err(|error| CatalogServiceError(error.to_string()))?.len();
                    write_batch.push(CatalogPageWrite { page, etag: None, last_modified: None }, publications, bytes);
                    if write_batch.should_flush_incremental() {
                        self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
                    }
                }
            }
            self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
            Ok::<_, CatalogServiceError>(true)
        }
        .await;
        let complete = result.as_ref().copied().unwrap_or(false);
        let cache_source = source_id.clone();
        self.cache_write_call_named("crawl_finish_incremental", move |cache| cache.finish_incremental_crawl(&cache_source, complete)).await?;
        self.state.extraction_notify.notify_one();
        result.map(|_| ())
    }

    pub async fn run_refresh_loop(self) {
        loop {
            if !catalog_crawls_disabled() {
                self.refresh_all_sources().await;
            } else {
                tracing::info!(sentinel = CRAWL_PAUSE_SENTINEL, "catalog crawls are paused");
            }
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
            self.resume_pending_epub_extractions().await;
            self.resume_metadata_index().await;
            self.resume_metadata_backfill().await;
            self.resume_description_backfill().await;
            tokio::select! {
                _ = tokio::time::sleep(EPUB_EXTRACTION_SWEEP_INTERVAL) => {}
                _ = self.state.extraction_notify.notified() => {}
            }
        }
    }

    pub(crate) async fn resume_metadata_index(&self) {
        loop {
            match self.cache_call(|cache| cache.migrate_metadata_index_batch(METADATA_BACKFILL_BATCH_SIZE)).await {
                Ok(count) if count == METADATA_BACKFILL_BATCH_SIZE => tokio::task::yield_now().await,
                Ok(_) => return,
                Err(error) => {
                    tracing::warn!(%error, "could not migrate saved catalogue metadata index");
                    return;
                }
            }
        }
    }

    pub async fn refresh_search_index(&self) {
        match self.cache_write_call_named("search_index_refresh", |cache| cache.refresh_search_index()).await {
            Ok(true) => tracing::info!("catalogue full-text search index is ready"),
            Ok(false) => {}
            Err(error) => tracing::error!(%error, "failed to refresh catalogue full-text search index"),
        }
    }

    pub async fn refresh_unified_subject_assignments(&self) {
        if let Err(error) = self.cache_write_call_named("subject_refresh", |cache| cache.refresh_unified_subject_assignments()).await {
            tracing::error!(%error, "failed to refresh unified catalogue subject assignments");
        }
    }
    pub(crate) async fn refresh_all_sources(&self) {
        let mut endpoints = self.state.endpoints.read().await.clone();
        prioritize_startup_crawls(&mut endpoints);
        let source_ids = endpoints.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        if let Err(error) = self.cache_write_call(move |cache| cache.queue_crawls(&source_ids)).await {
            tracing::warn!(%error, "could not persist the startup crawl queue");
        }
        for endpoint in endpoints {
            let source_id = opds_catalog_source_id(&endpoint.id);
            if let Err(error) = self.crawl_source(&source_id).await {
                tracing::warn!(source = source_id.as_str(), %error, "catalog crawl failed; retaining the previous generation");
            }
        }
    }
}

pub(crate) fn classify_catalog_failure(message: &str) -> (&'static str, bool) {
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

pub(crate) fn prioritize_startup_crawls(endpoints: &mut [OpdsEndpoint]) {
    endpoints.sort_by_key(|endpoint| endpoint.id.as_str() != PRIORITY_CRAWL_SOURCE);
}

pub(crate) fn reconcile_cached_sources(cache: &CatalogCache, endpoints: &[OpdsEndpoint]) -> Result<Vec<CatalogSourceId>, CatalogServiceError> {
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

pub(crate) fn page_publication_count(page: &CatalogPage) -> u64 {
    page.publications.len().saturating_add(page.groups.iter().map(|group| group.publications.len()).sum::<usize>()) as u64
}

pub(crate) fn page_cover_urls(page: &CatalogPage) -> Vec<String> {
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter_map(|publication| publication.image_url.clone()).filter(|url| !is_synthetic_cover_key(url)).collect()
}

pub(crate) fn internal_page_links(root: &Url, page: &CatalogPage) -> Result<Vec<String>, CatalogServiceError> {
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

pub(crate) fn canonicalize_page_layout_locators(endpoint: &OpdsEndpoint, page: &mut catalog_contract::CatalogPageLayout) -> Result<(), CatalogServiceError> {
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

pub(crate) fn same_origin(root: &Url, candidate: &str) -> Result<bool, CatalogServiceError> {
    let candidate = Url::parse(candidate).map_err(|error| CatalogServiceError(error.to_string()))?;
    Ok(root.scheme() == candidate.scheme() && root.host_str() == candidate.host_str() && root.port_or_known_default() == candidate.port_or_known_default())
}

use super::{
    canonicalize_gutenberg_page, canonicalize_opds_catalog_locator, catalog_crawls_disabled, duration_millis, opds_catalog_source_id, opds_page, prepare_catalog_subject_assignments, store_error, validate_opds_catalog_locator,
    AdaptiveCrawlWriteBatch, AdaptiveOriginThrottle, AdaptiveOriginThrottleState, Arc, CatalogCrawlGeneration, CatalogLinkTarget, CatalogMetadataBackfillIndexedUpdate, CatalogMetadataBackfillWriteTimings, CatalogPage, CatalogPageWrite,
    CatalogProcess, CatalogPublication, CatalogService, CatalogServiceError, CatalogSourceId, CatalogStore, CatalogStoreError, CrawlLocator, DiskCrawlFrontier, Duration, HashSet, Instant, Mutex, OpdsConditionalCatalog, OpdsEndpoint,
    OpdsError, OpdsHttpValidators, OriginThrottleFeedback, OriginThrottleSnapshot, PopularityRank, Url, CRAWL_ACTIVATION_BATCH, CRAWL_CYCLE, CRAWL_ORIGIN_DEFAULT_RATE_LIMIT_BACKOFF, CRAWL_ORIGIN_INITIAL_SPACING, CRAWL_ORIGIN_MAX_ATTEMPTS,
    CRAWL_ORIGIN_MAX_RATE_LIMIT_BACKOFF, CRAWL_ORIGIN_MAX_SPACING, CRAWL_ORIGIN_MIN_SPACING, CRAWL_ORIGIN_SUCCESS_WINDOW, CRAWL_ORIGIN_TRANSIENT_BACKOFF, CRAWL_PAGE_CONCURRENCY, CRAWL_PAGE_CONCURRENCY_MAX, CRAWL_PAUSE_SENTINEL,
    EPUB_EXTRACTION_SWEEP_INTERVAL, EXTRACTION_RECONCILIATION_BATCH, INACTIVE_GENERATION_CLEANUP_BATCH, METADATA_BACKFILL_BATCH_SIZE, PRIORITY_CRAWL_SOURCE, REGISTRY_REFRESH_INTERVAL,
};
use futures_util::StreamExt;

struct FetchedCrawlPage {
    write: CatalogPageWrite,
    links: Vec<CrawlLocator>,
}

enum IncrementalCrawlPage {
    Cached { links: Vec<CrawlLocator> },
    Fetched(FetchedCrawlPage),
    LimitReached,
}

impl AdaptiveOriginThrottle {
    fn new() -> Self {
        Self {
            state: Mutex::new(AdaptiveOriginThrottleState {
                in_flight: 0,
                concurrency_limit: CRAWL_PAGE_CONCURRENCY,
                success_streak: 0,
                minimum_spacing: CRAWL_ORIGIN_INITIAL_SPACING,
                next_request_at: Instant::now(),
                blocked_until: None,
                latency_ewma_ms: None,
            }),
            notify: tokio::sync::Notify::new(),
        }
    }

    async fn acquire(&self) {
        loop {
            let notified = self.notify.notified();
            let wait = {
                let mut state = self.state.lock().await;
                let now = Instant::now();
                if state.blocked_until.is_some_and(|until| until <= now) {
                    state.blocked_until = None;
                }
                if let Some(until) = state.blocked_until {
                    Some(until.saturating_duration_since(now))
                } else if now < state.next_request_at {
                    Some(state.next_request_at.saturating_duration_since(now))
                } else if state.in_flight < state.concurrency_limit {
                    state.in_flight += 1;
                    state.next_request_at = now + state.minimum_spacing;
                    return;
                } else {
                    None
                }
            };
            if let Some(wait) = wait {
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = notified => {}
                }
            } else {
                notified.await;
            }
        }
    }

    async fn complete(&self, feedback: OriginThrottleFeedback) -> (usize, usize, Duration, Option<Duration>) {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        let previous_limit = state.concurrency_limit;
        state.in_flight = state.in_flight.saturating_sub(1);
        state.record(feedback, now);
        let blocked_for = state.blocked_until.map(|until| until.saturating_duration_since(now));
        let snapshot = (previous_limit, state.concurrency_limit, state.minimum_spacing, blocked_for);
        drop(state);
        self.notify.notify_waiters();
        snapshot
    }
}

impl AdaptiveOriginThrottleState {
    pub(crate) fn record(&mut self, feedback: OriginThrottleFeedback, now: Instant) {
        match feedback {
            OriginThrottleFeedback::Success(elapsed) => {
                let elapsed_ms = elapsed.as_millis().min(u64::MAX as u128) as u64;
                let slow = self.latency_ewma_ms.is_some_and(|average| elapsed_ms > average.saturating_mul(2).max(1_500));
                self.latency_ewma_ms = Some(self.latency_ewma_ms.map_or(elapsed_ms, |average| average.saturating_mul(7).saturating_add(elapsed_ms) / 8));
                if slow {
                    self.success_streak = 0;
                    self.concurrency_limit = self.concurrency_limit.saturating_sub(1).max(1);
                    self.minimum_spacing = self.minimum_spacing.saturating_mul(2).min(CRAWL_ORIGIN_MAX_SPACING);
                } else {
                    self.success_streak = self.success_streak.saturating_add(1);
                    if self.success_streak >= CRAWL_ORIGIN_SUCCESS_WINDOW {
                        self.success_streak = 0;
                        self.concurrency_limit = self.concurrency_limit.saturating_add(1).min(CRAWL_PAGE_CONCURRENCY_MAX);
                        self.minimum_spacing = self.minimum_spacing.mul_f64(0.75).max(CRAWL_ORIGIN_MIN_SPACING);
                    }
                }
            }
            OriginThrottleFeedback::RateLimited(retry_after) => {
                self.success_streak = 0;
                self.concurrency_limit = 1;
                self.minimum_spacing = self.minimum_spacing.saturating_mul(2).max(Duration::from_secs(1)).min(CRAWL_ORIGIN_MAX_SPACING);
                let backoff = retry_after.unwrap_or(CRAWL_ORIGIN_DEFAULT_RATE_LIMIT_BACKOFF).min(CRAWL_ORIGIN_MAX_RATE_LIMIT_BACKOFF);
                let until = now + backoff;
                self.blocked_until = Some(self.blocked_until.map_or(until, |current| current.max(until)));
            }
            OriginThrottleFeedback::TransientFailure => {
                self.success_streak = 0;
                self.concurrency_limit = (self.concurrency_limit / 2).max(1);
                self.minimum_spacing = self.minimum_spacing.saturating_mul(2).min(CRAWL_ORIGIN_MAX_SPACING);
                let until = now + CRAWL_ORIGIN_TRANSIENT_BACKOFF;
                self.blocked_until = Some(self.blocked_until.map_or(until, |current| current.max(until)));
            }
            OriginThrottleFeedback::OtherFailure => self.success_streak = 0,
        }
    }
}

impl OriginThrottleFeedback {
    fn retryable(self) -> bool {
        matches!(self, Self::RateLimited(_) | Self::TransientFailure)
    }
}

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

    async fn origin_throttle(&self, url: &Url) -> Arc<AdaptiveOriginThrottle> {
        let origin = url.origin().ascii_serialization();
        let mut throttles = self.state.origin_throttles.lock().await;
        throttles.entry(origin).or_insert_with(|| Arc::new(AdaptiveOriginThrottle::new())).clone()
    }

    pub(crate) async fn origin_throttle_snapshot(&self, url: &Url) -> OriginThrottleSnapshot {
        let throttle = self.origin_throttle(url).await;
        let state = throttle.state.lock().await;
        OriginThrottleSnapshot {
            in_flight: state.in_flight,
            concurrency_limit: state.concurrency_limit,
            minimum_spacing_ms: state.minimum_spacing.as_millis().min(u64::MAX as u128) as u64,
            blocked_for_ms: state.blocked_until.map(|until| until.saturating_duration_since(Instant::now()).as_millis().min(u64::MAX as u128) as u64),
        }
    }

    async fn fetch_catalog_fresh_throttled(&self, origin_url: &Url, locator: &str) -> Result<opds::OpdsCatalog, CatalogServiceError> {
        let throttle = self.origin_throttle(origin_url).await;
        for attempt in 1..=CRAWL_ORIGIN_MAX_ATTEMPTS {
            throttle.acquire().await;
            let started = Instant::now();
            let result = self.state.opds.fetch_catalog_fresh(locator).await;
            let feedback = origin_throttle_feedback(&result, started.elapsed());
            self.record_origin_fetch(origin_url, &throttle, feedback).await;
            match result {
                Ok(catalog) => return Ok(catalog),
                Err(error) if feedback.retryable() && attempt < CRAWL_ORIGIN_MAX_ATTEMPTS => {
                    tracing::warn!(origin = %origin_url.origin().ascii_serialization(), attempt, %error, "retrying throttled catalogue request");
                }
                Err(error) => return Err(CatalogServiceError(error.to_string())),
            }
        }
        unreachable!("bounded catalogue attempts always return")
    }

    async fn fetch_catalog_conditional_throttled(&self, origin_url: &Url, locator: &str, validators: &OpdsHttpValidators) -> Result<OpdsConditionalCatalog, CatalogServiceError> {
        let throttle = self.origin_throttle(origin_url).await;
        for attempt in 1..=CRAWL_ORIGIN_MAX_ATTEMPTS {
            throttle.acquire().await;
            let started = Instant::now();
            let result = self.state.opds.fetch_catalog_conditional(locator, validators).await;
            let feedback = origin_throttle_feedback(&result, started.elapsed());
            self.record_origin_fetch(origin_url, &throttle, feedback).await;
            match result {
                Ok(catalog) => return Ok(catalog),
                Err(error) if feedback.retryable() && attempt < CRAWL_ORIGIN_MAX_ATTEMPTS => {
                    tracing::warn!(origin = %origin_url.origin().ascii_serialization(), attempt, %error, "retrying throttled conditional catalogue request");
                }
                Err(error) => return Err(CatalogServiceError(error.to_string())),
            }
        }
        unreachable!("bounded catalogue attempts always return")
    }

    async fn record_origin_fetch(&self, origin_url: &Url, throttle: &AdaptiveOriginThrottle, feedback: OriginThrottleFeedback) {
        let (previous_limit, concurrency_limit, minimum_spacing, blocked_for) = throttle.complete(feedback).await;
        if previous_limit != concurrency_limit || blocked_for.is_some() {
            tracing::info!(
                origin = %origin_url.origin().ascii_serialization(),
                concurrency = concurrency_limit,
                spacing_ms = minimum_spacing.as_millis(),
                blocked_ms = blocked_for.map(|delay| delay.as_millis()),
                "adapted catalogue origin throttle"
            );
        }
    }

    pub(crate) async fn source_update_lock(&self, source_id: &CatalogSourceId) -> Arc<Mutex<()>> {
        let mut locks = self.state.source_update_locks.lock().await;
        locks.entry(source_id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub(crate) async fn store_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogStore) -> Result<T, CatalogStoreError> + Send + 'static,
    {
        let store = self.state.store.clone();
        tokio::task::spawn_blocking(move || operation(&store)).await.map_err(|error| CatalogServiceError(format!("catalog store worker failed: {error}")))?.map_err(store_error)
    }

    pub(crate) async fn store_write_call<T, F>(&self, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogStore) -> Result<T, CatalogStoreError> + Send + 'static,
    {
        self.store_write_call_named("other", operation).await
    }

    pub(crate) async fn store_write_call_named<T, F>(&self, name: &'static str, operation: F) -> Result<T, CatalogServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&CatalogStore) -> Result<T, CatalogStoreError> + Send + 'static,
    {
        let wait_started = Instant::now();
        let _writer = self.state.store_write_lock.lock().await;
        let wait_ms = duration_millis(wait_started.elapsed());
        let hold_started = Instant::now();
        let store = self.state.store.clone();
        let result = tokio::task::spawn_blocking(move || operation(&store)).await.map_err(|error| CatalogServiceError(format!("catalog store worker failed: {error}"))).and_then(|result| result.map_err(store_error));
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
        let _writer = self.state.store_write_lock.lock().await;
        let writer_wait_ms = duration_millis(wait_started.elapsed());
        let hold_started = Instant::now();
        let store = self.state.store.clone();
        let result = tokio::task::spawn_blocking(move || store.finish_metadata_backfill_indexed_batch_timed(&updates, &prepared_assignments))
            .await
            .map_err(|error| CatalogServiceError(format!("catalog store worker failed: {error}")))
            .and_then(|result| result.map_err(store_error));
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
        let prepared = self.store_call(move |_| CatalogStore::prepare_page_writes(pages)).await?;
        let cache_crawl = crawl.clone();
        let started = Instant::now();
        self.store_write_call_named("crawl_page_write", move |cache| cache.stage_prepared_pages(&cache_crawl, &prepared)).await?;
        batch.record_commit(started.elapsed());
        for publications in publication_counts {
            self.record_crawl_page(source_id, publications).await;
        }
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
                .store_call(move |cache| {
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
        let prepared = self.store_call(move |_| CatalogStore::prepare_page_writes(pages)).await?;
        let cache_source = source_id.clone();
        let cache_root = source_url.to_owned();
        let started = Instant::now();
        self.store_write_call_named("crawl_page_write", move |cache| cache.put_prepared_pages(&cache_source, &cache_root, &prepared)).await?;
        batch.record_commit(started.elapsed());
        drop(_update);
        for publications in publication_counts {
            self.record_crawl_page(source_id, publications).await;
        }
        self.wake_process(CatalogProcess::CoverDownloads);
        tokio::task::yield_now().await;
        Ok(())
    }

    pub(crate) async fn cleanup_inactive_generations(&self, source_id: CatalogSourceId) {
        loop {
            let cache_source = source_id.clone();
            match self.store_write_call_named("cleanup", move |cache| cache.cleanup_inactive_generation_batch(&cache_source, INACTIVE_GENERATION_CLEANUP_BATCH)).await {
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
            let more = self.store_write_call_named("epub_reconcile", move |cache| cache.reconcile_active_epub_extractions_batch(&cache_source, EXTRACTION_RECONCILIATION_BATCH)).await?;
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
        F: FnOnce(CatalogStore) -> Result<T, CatalogServiceError> + Send + 'static,
    {
        let store = self.state.store.clone();
        tokio::task::spawn_blocking(move || operation(store)).await.map_err(|error| CatalogServiceError(format!("catalog store worker failed: {error}")))?
    }

    pub(crate) async fn crawl_endpoint(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.store_write_call_named("crawl_setup", move |cache| cache.ensure_source(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not reconcile crawl source: {error}")))?;
        let cache_source = source_id.clone();
        let has_pages = self.store_call(move |cache| cache.has_pages(&cache_source)).await.map_err(|error| CatalogServiceError(format!("could not inspect cached crawl pages: {error}")))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        let has_frontier = self.store_call(move |cache| cache.has_missing_page_frontier(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not inspect crawl frontier: {error}")))?;
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
        let crawl = self.store_write_call_named("crawl_begin", move |cache| cache.begin_crawl(&cache_source, &cache_root)).await.map_err(|error| CatalogServiceError(format!("could not start replacement crawl: {error}")))?;
        self.begin_crawl_progress(&source_id).await;
        let mut frontier = DiskCrawlFrontier::new(&endpoint.url, self.state.max_crawl_pages)?;
        let mut write_batch = AdaptiveCrawlWriteBatch::default();
        let result = async {
            loop {
                let batch = frontier.claim_batch()?;
                if batch.is_empty() {
                    break;
                }
                let mut pages = futures_util::stream::iter(batch.into_iter().map(|locator| self.fetch_replacement_crawl_page(&endpoint, &root_url, &source_id, locator))).buffered(CRAWL_PAGE_CONCURRENCY_MAX);
                while let Some(page) = pages.next().await {
                    let page = page?;
                    frontier.enqueue(page.links)?;
                    let publications = page_publication_count(&page.write.page) as usize;
                    let bytes = wire::encode(&page.write.page).map_err(|error| CatalogServiceError(error.to_string()))?.len();
                    write_batch.push(page.write, publications, bytes);
                    if write_batch.reached_capacity() {
                        self.flush_replacement_crawl_batch(&crawl, &source_id, &mut write_batch).await?;
                    }
                }
            }
            self.flush_replacement_crawl_batch(&crawl, &source_id, &mut write_batch).await?;
            loop {
                let cache_crawl = crawl.clone();
                let more = self.store_write_call_named("crawl_activation", move |cache| cache.prepare_crawl_activation_batch(&cache_crawl, CRAWL_ACTIVATION_BATCH)).await?;
                if !more {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let cache_crawl = crawl.clone();
            self.store_write_call_named("crawl_finalize", move |cache| cache.finalize_crawl_activation(&cache_crawl)).await?;
            self.reconcile_active_cover_downloads().await?;
            self.reconcile_active_epub_extractions(source_id.clone()).await?;
            self.cleanup_inactive_generations(source_id.clone()).await;
            Ok::<(), CatalogServiceError>(())
        }
        .await;
        if let Err(error) = &result {
            let message = error.to_string();
            let cache_crawl = crawl.clone();
            let _ = self.store_write_call_named("crawl_abandon", move |cache| cache.abandon_crawl_with_error(&cache_crawl, Some(&message))).await;
            self.cleanup_inactive_generations(source_id.clone()).await;
        }
        result
    }

    pub(crate) async fn crawl_endpoint_incrementally(&self, endpoint: OpdsEndpoint) -> Result<(), CatalogServiceError> {
        let source_id = opds_catalog_source_id(&endpoint.id);
        let root_url = Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_root = endpoint.url.clone();
        self.store_write_call_named("crawl_begin_incremental", move |cache| cache.begin_incremental_crawl(&cache_source, &cache_root)).await?;
        self.begin_crawl_progress(&source_id).await;
        let mut frontier = DiskCrawlFrontier::new(&endpoint.url, self.state.max_crawl_pages)?;
        let fetch_slots = Arc::new(tokio::sync::Semaphore::new(self.state.max_crawl_pages));
        let mut write_batch = AdaptiveCrawlWriteBatch::default();
        // The root is the first user-visible snapshot of an incremental crawl.
        // Publish it before fetching descendants so a slow child cannot make
        // the entire catalogue appear unavailable.
        let mut root_published = false;
        let result = async {
            loop {
                let batch = frontier.claim_batch()?;
                if batch.is_empty() {
                    break;
                }
                let mut limit_reached = false;
                let mut pages = futures_util::stream::iter(batch.into_iter().map(|locator| self.fetch_incremental_crawl_page(&endpoint, &root_url, &source_id, locator, fetch_slots.clone()))).buffered(CRAWL_PAGE_CONCURRENCY_MAX);
                while let Some(page) = pages.next().await {
                    match page? {
                        IncrementalCrawlPage::Cached { links } => frontier.enqueue(links)?,
                        IncrementalCrawlPage::Fetched(page) => {
                            frontier.enqueue(page.links)?;
                            let publications = page_publication_count(&page.write.page) as usize;
                            let bytes = wire::encode(&page.write.page).map_err(|error| CatalogServiceError(error.to_string()))?.len();
                            write_batch.push(page.write, publications, bytes);
                            if write_batch.should_flush_incremental() {
                                self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
                            }
                        }
                        IncrementalCrawlPage::LimitReached => limit_reached = true,
                    }
                }
                if limit_reached {
                    self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
                    return Ok(false);
                }
                if !root_published {
                    self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
                    root_published = true;
                }
            }
            self.flush_incremental_crawl_batch(&source_id, &endpoint.url, &mut write_batch).await?;
            Ok::<_, CatalogServiceError>(true)
        }
        .await;
        let complete = result.as_ref().copied().unwrap_or(false);
        let cache_source = source_id.clone();
        self.store_write_call_named("crawl_finish_incremental", move |cache| cache.finish_incremental_crawl(&cache_source, complete)).await?;
        self.reconcile_active_cover_downloads().await?;
        self.state.extraction_notify.notify_one();
        result.map(|_| ())
    }

    async fn fetch_replacement_crawl_page(&self, endpoint: &OpdsEndpoint, root_url: &Url, source_id: &CatalogSourceId, crawl_locator: CrawlLocator) -> Result<FetchedCrawlPage, CatalogServiceError> {
        let popular_listing = crawl_locator.popular_listing;
        let locator = crawl_locator.locator;
        validate_opds_catalog_locator(endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_locator = locator.clone();
        let cached = self.store_call(move |cache| cache.page(&cache_source, &cache_locator)).await?;
        // Cached leaves are immutable catalogue records and never contact the
        // upstream server during replacement crawls.
        if let Some(cached_page) = cached.as_ref().filter(|page| !page.provider_paginated) {
            if internal_page_links(root_url, &cached_page.page, popular_listing)?.is_empty() {
                let cache_source = source_id.clone();
                let mut page = cached_page.page.clone();
                page = self
                    .store_call(move |cache| {
                        cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                        Ok(page)
                    })
                    .await?;
                annotate_popular_listing(&mut page, popular_listing);
                return Ok(FetchedCrawlPage { write: CatalogPageWrite { page, etag: cached_page.etag.clone(), last_modified: cached_page.last_modified.clone(), provider_paginated: false }, links: Vec::new() });
            }
        }
        // A validator belongs only to the first upstream page, not to the
        // compacted logical collection. Re-fetch the whole provider chain when
        // the cached page was built from pagination.
        let request_validators = cached.as_ref().filter(|page| !page.provider_paginated).map(|page| OpdsHttpValidators { etag: page.etag.clone(), last_modified: page.last_modified.clone() }).unwrap_or_default();
        let (mut page, validators, provider_paginated) = match self.fetch_catalog_conditional_throttled(root_url, &locator, &request_validators).await? {
            OpdsConditionalCatalog::NotModified => {
                let cached = cached.ok_or_else(|| CatalogServiceError("upstream returned 304 for an uncached catalog page".to_owned()))?;
                if cached.provider_paginated {
                    return Err(CatalogServiceError("upstream returned 304 without a validator for a paginated catalogue".to_owned()));
                }
                (cached.page, request_validators, false)
            }
            OpdsConditionalCatalog::Modified { catalog, validators } => {
                validate_opds_catalog_locator(endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
                let page = opds_page(source_id.clone(), catalog);
                let (page, provider_paginated) = self.compact_provider_pagination(endpoint, root_url, source_id, page).await?;
                (page, validators, provider_paginated)
            }
        };
        let cache_source = source_id.clone();
        page = self
            .store_call(move |cache| {
                cache.enrich_page_from_epub_extractions(&cache_source, &mut page)?;
                Ok(page)
            })
            .await?;
        self.prepare_publication_cover_fallbacks(source_id, &mut page).await;
        annotate_popular_listing(&mut page, popular_listing);
        let links = internal_page_links(root_url, &page, popular_listing)?;
        Ok(FetchedCrawlPage { write: CatalogPageWrite { page, etag: validators.etag, last_modified: validators.last_modified, provider_paginated }, links })
    }

    async fn fetch_incremental_crawl_page(
        &self, endpoint: &OpdsEndpoint, root_url: &Url, source_id: &CatalogSourceId, crawl_locator: CrawlLocator, fetch_slots: Arc<tokio::sync::Semaphore>,
    ) -> Result<IncrementalCrawlPage, CatalogServiceError> {
        let popular_listing = crawl_locator.popular_listing;
        let locator = crawl_locator.locator;
        validate_opds_catalog_locator(endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
        let cache_source = source_id.clone();
        let cache_locator = locator.clone();
        if let Some(mut cached) = self.store_call(move |cache| cache.page(&cache_source, &cache_locator)).await? {
            let links = internal_page_links(root_url, &cached.page, popular_listing)?;
            if popular_listing && !page_has_popular_rank(&cached.page) {
                annotate_popular_listing(&mut cached.page, true);
                return Ok(IncrementalCrawlPage::Fetched(FetchedCrawlPage { write: CatalogPageWrite { page: cached.page, etag: cached.etag, last_modified: cached.last_modified, provider_paginated: cached.provider_paginated }, links }));
            }
            return Ok(IncrementalCrawlPage::Cached { links });
        }
        let Ok(fetch_slot) = fetch_slots.try_acquire_owned() else {
            return Ok(IncrementalCrawlPage::LimitReached);
        };
        fetch_slot.forget();
        let catalog = self.fetch_catalog_fresh_throttled(root_url, &locator).await?;
        validate_opds_catalog_locator(endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
        let page = opds_page(source_id.clone(), catalog);
        let (mut page, provider_paginated) = self.compact_provider_pagination(endpoint, root_url, source_id, page).await?;
        annotate_popular_listing(&mut page, popular_listing);
        let links = internal_page_links(root_url, &page, popular_listing)?;
        Ok(IncrementalCrawlPage::Fetched(FetchedCrawlPage { write: CatalogPageWrite { page, etag: None, last_modified: None, provider_paginated }, links }))
    }

    /// Consumes provider pagination while data is entering the cache. The
    /// returned page is the only representation staged or exposed; upstream
    /// continuation locators never become catalogue navigation pages.
    pub(crate) async fn compact_provider_pagination(&self, endpoint: &OpdsEndpoint, root_url: &Url, source_id: &CatalogSourceId, first_page: CatalogPage) -> Result<(CatalogPage, bool), CatalogServiceError> {
        let mut pages = vec![first_page];
        let mut visited = HashSet::new();
        visited.insert(pages[0].locator.clone());
        let mut next = pages[0].pagination.next.clone();
        let provider_paginated = next.is_some() || pages[0].pagination.first.is_some() || pages[0].pagination.previous.is_some() || pages[0].pagination.last.is_some();

        while let Some(locator) = next {
            if !same_origin(root_url, &locator)? {
                break;
            }
            validate_opds_catalog_locator(endpoint, &locator).map_err(|error| CatalogServiceError(error.to_string()))?;
            if !visited.insert(locator.clone()) {
                return Err(CatalogServiceError("catalogue provider pagination contains a cycle".to_owned()));
            }
            if pages.len() >= self.state.max_crawl_pages {
                return Err(CatalogServiceError(format!("catalogue pagination exceeded {} pages", self.state.max_crawl_pages)));
            }
            let catalog = self.fetch_catalog_fresh_throttled(root_url, &locator).await?;
            validate_opds_catalog_locator(endpoint, &catalog.source_url).map_err(|error| CatalogServiceError(error.to_string()))?;
            let page = opds_page(source_id.clone(), catalog);
            next = page.pagination.next.clone();
            pages.push(page);
        }

        Ok((compact_catalog_pages(pages), provider_paginated))
    }

    pub async fn run_refresh_loop(self) {
        loop {
            if self.process_enabled(CatalogProcess::CatalogueCrawling) && !catalog_crawls_disabled() {
                self.set_process_running(CatalogProcess::CatalogueCrawling, true);
                self.refresh_all_sources().await;
                self.set_process_running(CatalogProcess::CatalogueCrawling, false);
            } else {
                tracing::info!(sentinel = CRAWL_PAUSE_SENTINEL, "catalog crawls are paused");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                    _ = self.wait_for_process_wake(CatalogProcess::CatalogueCrawling) => {}
                }
                continue;
            }
            tokio::select! {
                _ = tokio::time::sleep(CRAWL_CYCLE) => {}
                _ = self.wait_for_process_wake(CatalogProcess::CatalogueCrawling) => {}
            }
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
            if !self.process_enabled(CatalogProcess::Enrichment) {
                self.set_process_running(CatalogProcess::Enrichment, false);
                self.wait_for_process_wake(CatalogProcess::Enrichment).await;
                continue;
            }
            self.set_process_running(CatalogProcess::Enrichment, true);
            self.resume_pending_epub_extractions_staged().await;
            self.set_process_running(CatalogProcess::Enrichment, false);
            tokio::select! {
                _ = tokio::time::sleep(EPUB_EXTRACTION_SWEEP_INTERVAL) => {}
                _ = self.state.extraction_notify.notified() => {}
                _ = self.wait_for_process_wake(CatalogProcess::Enrichment) => {}
            }
        }
    }

    pub async fn run_epub_download_loop(self) {
        let mut reconciliation_pending = true;
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                self.wait_for_process_wake(CatalogProcess::Enrichment).await;
                continue;
            }
            if reconciliation_pending {
                let source_ids = self.state.endpoints.read().await.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
                for source_id in source_ids {
                    if let Err(error) = self.reconcile_active_epub_extractions(source_id.clone()).await {
                        tracing::warn!(source = source_id.as_str(), %error, "could not reconcile EPUB extraction queue before downloads");
                    }
                }
                reconciliation_pending = false;
            }
            self.resume_pending_epub_downloads().await;
            tokio::select! {
                _ = tokio::time::sleep(EPUB_EXTRACTION_SWEEP_INTERVAL) => {}
                _ = self.state.extraction_notify.notified() => {}
                _ = self.wait_for_process_wake(CatalogProcess::Enrichment) => {}
            }
        }
    }

    pub async fn run_catalogue_enrichment_loop(self) {
        let mut startup_repairs_pending = true;
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                self.set_process_running(CatalogProcess::Enrichment, false);
                self.wait_for_process_wake(CatalogProcess::Enrichment).await;
                continue;
            }
            self.set_process_running(CatalogProcess::Enrichment, true);
            self.run_catalogue_enrichment_pass(startup_repairs_pending).await;
            startup_repairs_pending = false;
            self.set_process_running(CatalogProcess::Enrichment, false);
            tokio::select! {
                _ = tokio::time::sleep(EPUB_EXTRACTION_SWEEP_INTERVAL) => {}
                _ = self.wait_for_process_wake(CatalogProcess::Enrichment) => {}
            }
        }
    }

    pub(crate) async fn resume_metadata_index(&self) {
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                return;
            }
            match self.store_write_call_named("metadata_index_migrate", |cache| cache.migrate_metadata_index_batch(METADATA_BACKFILL_BATCH_SIZE)).await {
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
        match self.store_write_call_named("search_index_refresh", |cache| cache.refresh_search_index()).await {
            Ok(true) => tracing::info!("catalogue full-text search index is ready"),
            Ok(false) => {}
            Err(error) => tracing::error!(%error, "failed to refresh catalogue full-text search index"),
        }
    }

    pub async fn refresh_unified_subject_assignments(&self) {
        if let Err(error) = self.store_write_call_named("subject_refresh", |cache| cache.refresh_unified_subject_assignments()).await {
            tracing::error!(%error, "failed to refresh unified catalogue subject assignments");
        }
    }

    pub async fn refresh_subject_browse_index(&self) {
        match self.store_write_call_named("subject_browse_index_refresh", |cache| cache.refresh_subject_browse_index()).await {
            Ok(true) => tracing::info!("catalogue subject browse index is ready"),
            Ok(false) => {}
            Err(error) => tracing::error!(%error, "failed to refresh catalogue subject browse index"),
        }
    }

    pub async fn refresh_browse_serving_index(&self) {
        let mut changed = false;
        loop {
            match self.store_call(|cache| cache.refresh_browse_serving_index_batch()).await {
                Ok(true) => {
                    changed = true;
                    tokio::task::yield_now().await;
                }
                Ok(false) => {
                    if changed {
                        tracing::info!("catalogue browse serving index is ready");
                    }
                    return;
                }
                Err(error) => {
                    tracing::error!(%error, "failed to refresh catalogue browse serving index");
                    return;
                }
            }
        }
    }
    pub(crate) async fn refresh_all_sources(&self) {
        let mut endpoints = self.state.endpoints.read().await.clone();
        prioritize_startup_crawls(&mut endpoints);
        let source_ids = endpoints.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        if let Err(error) = self.store_write_call(move |cache| cache.queue_crawls(&source_ids)).await {
            tracing::warn!(%error, "could not persist the startup crawl queue");
        }
        let mut origin_crawls = tokio::task::JoinSet::new();
        for origin_endpoints in crawl_endpoint_groups(endpoints) {
            let service = self.clone();
            origin_crawls.spawn(async move {
                for endpoint in origin_endpoints {
                    if !service.process_enabled(CatalogProcess::CatalogueCrawling) {
                        tracing::info!("catalogue crawl process paused before the next source");
                        break;
                    }
                    let source_id = opds_catalog_source_id(&endpoint.id);
                    if let Err(error) = service.crawl_source(&source_id).await {
                        tracing::warn!(source = source_id.as_str(), %error, "catalog crawl failed; retaining the previous generation");
                    }
                }
            });
        }
        while let Some(result) = origin_crawls.join_next().await {
            if let Err(error) = result {
                tracing::error!(%error, "catalogue origin crawl task stopped unexpectedly");
            }
        }
    }
}

/// Runs distinct origins concurrently while preserving the crawler's existing
/// one-request-at-a-time behavior for every upstream origin. Registry order is
/// retained inside each group, including the priority-source ordering.
pub(crate) fn crawl_endpoint_groups(endpoints: Vec<OpdsEndpoint>) -> Vec<Vec<OpdsEndpoint>> {
    let mut groups: Vec<(String, Vec<OpdsEndpoint>)> = Vec::new();
    for endpoint in endpoints {
        let origin = Url::parse(&endpoint.url).map(|url| url.origin().ascii_serialization()).unwrap_or_else(|_| endpoint.url.clone());
        if let Some((_, group)) = groups.iter_mut().find(|(candidate, _)| candidate == &origin) {
            group.push(endpoint);
        } else {
            groups.push((origin, vec![endpoint]));
        }
    }
    groups.into_iter().map(|(_, endpoints)| endpoints).collect()
}

fn origin_throttle_feedback<T>(result: &Result<T, OpdsError>, elapsed: Duration) -> OriginThrottleFeedback {
    let Err(error) = result else { return OriginThrottleFeedback::Success(elapsed) };
    if error.retry_after().is_some() || error.http_status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
        return OriginThrottleFeedback::RateLimited(error.retry_after());
    }
    if error.http_status().is_some_and(|status| status.is_server_error() || status == reqwest::StatusCode::REQUEST_TIMEOUT)
        || matches!(error, OpdsError::Network(_) | OpdsError::TimedOut)
        || matches!(classify_catalog_failure(&error.to_string()), ("network_timeout" | "network_dns" | "network_tls" | "network_or_http", true))
    {
        OriginThrottleFeedback::TransientFailure
    } else {
        OriginThrottleFeedback::OtherFailure
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

pub(crate) fn reconcile_stored_sources(store: &CatalogStore, endpoints: &[OpdsEndpoint], cache_directories: Option<(&std::path::Path,&std::path::Path)>) -> Result<Vec<CatalogSourceId>, CatalogServiceError> {
    let retained = endpoints.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<HashSet<_>>();
    for endpoint in endpoints {
        store.ensure_source(&opds_catalog_source_id(&endpoint.id), &endpoint.url).map_err(store_error)?;
    }
    let mut removed = Vec::new();
    for source in store.inventory().map_err(store_error)?.sources {
        if !retained.contains(&source.source_id) {
            if let Some((acquisitions,covers))=cache_directories {
                crate::source_cleanup::purge_files(store,&source.source_id,acquisitions,covers)?;
            }
            store.remove_source(&source.source_id).map_err(store_error)?;
            tracing::info!(source=source.source_id.as_str(),publications=source.active_publication_count,"purged removed catalogue source");
            removed.push(source.source_id);
        }
    }
    Ok(removed)
}

pub(crate) fn page_publication_count(page: &CatalogPage) -> u64 {
    page.publications.len().saturating_add(page.groups.iter().map(|group| group.publications.len()).sum::<usize>()) as u64
}

fn compact_catalog_pages(pages: Vec<CatalogPage>) -> CatalogPage {
    let mut pages = pages.into_iter();
    let mut compacted = pages.next().expect("provider page compaction requires a first page");
    let mut top_level = HashSet::new();
    retain_unique_publications(&mut compacted.publications, &mut top_level);
    for group in &mut compacted.groups {
        let mut seen = HashSet::new();
        retain_unique_publications(&mut group.publications, &mut seen);
    }

    for mut page in pages {
        for link in page.navigation.drain(..) {
            if !compacted.navigation.contains(&link) {
                compacted.navigation.push(link);
            }
        }
        retain_unique_publications(&mut page.publications, &mut top_level);
        compacted.publications.extend(page.publications);

        for mut group in page.groups {
            if let Some(existing) = compacted.groups.iter_mut().find(|existing| existing.title == group.title && existing.page_locator == group.page_locator) {
                existing.number_of_items = existing.number_of_items.into_iter().chain(group.number_of_items).max();
                for link in group.navigation.drain(..) {
                    if !existing.navigation.contains(&link) {
                        existing.navigation.push(link);
                    }
                }
                let mut seen = existing.publications.iter().map(|publication| (publication.identity.edition_id.clone(), publication.identity.work_id.clone().unwrap_or_default())).collect::<HashSet<_>>();
                retain_unique_publications(&mut group.publications, &mut seen);
                existing.publications.extend(group.publications);
            } else {
                let mut seen = HashSet::new();
                retain_unique_publications(&mut group.publications, &mut seen);
                compacted.groups.push(group);
            }
        }

        for mut facet in page.facets {
            if let Some(existing) = compacted.facets.iter_mut().find(|existing| existing.title == facet.title) {
                for link in facet.links.drain(..) {
                    if !existing.links.contains(&link) {
                        existing.links.push(link);
                    }
                }
            } else {
                compacted.facets.push(facet);
            }
        }
    }

    compacted.pagination = Default::default();
    canonicalize_gutenberg_page(&mut compacted);
    compacted
}

fn retain_unique_publications(publications: &mut Vec<CatalogPublication>, seen: &mut HashSet<(String, String)>) {
    publications.retain(|publication| seen.insert((publication.identity.edition_id.clone(), publication.identity.work_id.clone().unwrap_or_default())));
}

pub(crate) fn internal_page_links(root: &Url, page: &CatalogPage, current_popular_listing: bool) -> Result<Vec<CrawlLocator>, CatalogServiceError> {
    let links = page.navigation.iter().chain(page.groups.iter().flat_map(|group| group.navigation.iter())).filter_map(|link| match &link.target {
        CatalogLinkTarget::Page { locator } => Some((locator.as_str(), link.relations.iter().any(|relation| relation == "http://opds-spec.org/sort/popular" || relation == "popular"))),
        CatalogLinkTarget::External { .. } => None,
    });
    let mut internal = Vec::new();
    for (locator, popular_listing) in links {
        if same_origin(root, locator)? {
            internal.push(CrawlLocator { locator: locator.to_owned(), popular_listing });
        }
    }
    for locator in page
        .groups
        .iter()
        .filter_map(|group| group.page_locator.as_deref())
        .chain([page.pagination.first.as_deref(), page.pagination.previous.as_deref(), page.pagination.next.as_deref(), page.pagination.last.as_deref()].into_iter().flatten())
    {
        if same_origin(root, locator)? {
            internal.push(CrawlLocator { locator: locator.to_owned(), popular_listing: current_popular_listing });
        }
    }
    Ok(internal)
}

pub(crate) fn annotate_popular_listing(page: &mut CatalogPage, popular_listing: bool) {
    if !popular_listing {
        return;
    }
    let source = format!("opds:sort/popular:{}", page.locator);
    for (index, publication) in page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())).enumerate() {
        let rank = PopularityRank { position: u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1), source: source.clone() };
        if !publication.metadata.popularity_ranks.contains(&rank) {
            publication.metadata.popularity_ranks.push(rank);
        }
    }
}

fn page_has_popular_rank(page: &CatalogPage) -> bool {
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).any(|publication| publication.metadata.popularity_ranks.iter().any(|rank| rank.source.starts_with("opds:sort/popular:")))
}

pub(crate) fn canonicalize_page_layout_locators(endpoint: &OpdsEndpoint, page: &mut catalog_protocol::CatalogPageLayout) -> Result<(), CatalogServiceError> {
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

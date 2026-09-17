use super::{
    apply_catalog_description_if_missing, apply_catalog_metadata, classify_catalog_failure, extraction_book_format, merge_catalog_epub_metadata, metadata_book_byte_limit, opds_catalog_source_id, prepare_catalog_subject_assignments,
    book_has_acquisition_url, CatalogAuthorCredit, CatalogEpubExtractionJob, CatalogEpubMetadata, CatalogProcess, CatalogBookIdentity, CatalogBookPopularity, CatalogService, CatalogServiceError, CatalogSourceId,
    duration_millis, Deserialize, HashSet, Identifier, Instant, LanguageTag, OpdsAcquisitionKind, OpdsBookFormat, OpdsDirectAcquisition, Path, PathBuf, BookSubject, Scheme, Serialize, Url, BACKGROUND_DOWNLOAD_LIMIT, CATALOG_METADATA_DOWNLOAD_TIMEOUT, MAX_CONCURRENT_EXTRACTIONS,
    EPUB_COVER_KEY_PREFIX, MAX_EMBEDDED_DESCRIPTION_BYTES, MAX_EMBEDDED_METADATA_TEXT_BYTES, MAX_EXTRACTION_RESULT_BYTES,
};
#[cfg(not(test))]
use super::{EXTRACTION_WORKER_CPU_SECONDS, EXTRACTION_WORKER_MEMORY_BYTES, EXTRACTION_WORKER_TIMEOUT};
#[cfg(not(test))]
use std::process::Stdio;
use tokio::task::JoinSet;
struct PreparedCatalogEnrichment {
    metadata: ExtractedCatalogMetadata,
    saved_metadata: Option<CatalogEpubMetadata>,
    subjects: Vec<BookSubject>,
    popularity: Option<CatalogBookPopularity>,
    external_description: Option<String>,
    cover_is_definitively_absent: bool,
    downloaded_bytes: u64,
}

impl CatalogService {
    /// Compatibility entry point for callers/tests that explicitly ask to
    /// drain the whole queue. The service runtime uses the two stage-specific
    /// loops below so downloads and extraction can overlap.
    pub(crate) async fn resume_pending_epub_extractions(&self) {
        self.resume_pending_epub_downloads().await;
        self.resume_pending_epub_extractions_staged().await;
    }

    pub(crate) async fn resume_pending_epub_extractions_staged(&self) {
        if !self.process_enabled(CatalogProcess::Enrichment) {
            return;
        }
        let source_ids = self.state.endpoints.read().await.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                return;
            }
            let mut jobs = Vec::with_capacity(MAX_CONCURRENT_EXTRACTIONS);
            while jobs.len() < MAX_CONCURRENT_EXTRACTIONS {
                let mut claimed = false;
                for source_id in &source_ids {
                    if jobs.len() >= MAX_CONCURRENT_EXTRACTIONS || !self.process_enabled(CatalogProcess::Enrichment) {
                        break;
                    }
                    let cache_source = source_id.clone();
                    match self.store_write_call(move |cache| cache.claim_next_staged_epub_extraction(&cache_source)).await {
                        Ok(Some(job)) => {
                            jobs.push(job);
                            claimed = true;
                        }
                        Ok(None) => {}
                        Err(error) => tracing::warn!(source = source_id.as_str(), %error, "could not claim a staged book extraction"),
                    }
                }
                if !claimed {
                    break;
                }
            }
            if jobs.is_empty() {
                break;
            }

            let mut workers = JoinSet::new();
            for job in jobs {
                let service = self.clone();
                workers.spawn(async move {
                    let failure_source = job.source_id.clone();
                    let failure_url = job.acquisition_url.clone();
                    let failure_attempt = job.attempt;
                    let failure_path = job.staged_path.clone();
                    let result = service.extract_staged_book(job).await;
                    (failure_source, failure_url, failure_attempt, failure_path, result)
                });
            }
            while let Some(result) = workers.join_next().await {
                let Ok((failure_source, failure_url, failure_attempt, failure_path, result)) = result else { continue };
                if let Err(error) = result {
                    tracing::warn!(source = failure_source.as_str(), %error, "could not extract staged catalogue book");
                    let message = error.to_string();
                    let (category, retryable) = classify_catalog_failure(&message);
                    let cache_source = failure_source.clone();
                    let cache_url = failure_url.clone();
                    let _ = self.store_write_call(move |cache| cache.fail_epub_extraction(&cache_source, &cache_url, failure_attempt, &message, category, retryable)).await;
                    if let Some(path) = failure_path {
                        let _ = tokio::fs::remove_file(path).await;
                    }
                }
            }
        }
    }

    /// Network workers only download and atomically hand files to the durable
    /// staged state. They never hold an extraction permit or source update
    /// lock, so slow OCR/ZIP parsing cannot starve acquisition throughput.
    pub(crate) async fn resume_pending_epub_downloads(&self) {
        if !self.process_enabled(CatalogProcess::Enrichment) {
            return;
        }
        let source_ids = self.state.endpoints.read().await.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        for source_id in &source_ids {
            let cache_source = source_id.clone();
            if let Err(error) = self.store_write_call(move |cache| cache.retry_failed_epub_extractions(&cache_source)).await {
                tracing::warn!(source = source_id.as_str(), %error, "could not schedule due book extraction retries");
            }
        }
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                return;
            }
            let mut jobs = Vec::new();
            // Admit at most one candidate per source in each round. Repeating
            // the rounds fills the larger async window without letting a
            // single source monopolize it when other sources have pending
            // work. Source-level HTTP throttling remains the authority for
            // the actual request rate.
            while jobs.len() < BACKGROUND_DOWNLOAD_LIMIT {
                let mut claimed = false;
                for source_id in &source_ids {
                    if jobs.len() >= BACKGROUND_DOWNLOAD_LIMIT {
                        break;
                    }
                    let cache_source = source_id.clone();
                    match self.store_write_call(move |cache| cache.claim_next_epub_download(&cache_source)).await {
                        Ok(Some(job)) => {
                            jobs.push(job);
                            claimed = true;
                        }
                        Ok(None) => {}
                        Err(error) => tracing::warn!(source = source_id.as_str(), %error, "could not claim a book download"),
                    }
                }
                if !claimed {
                    break;
                }
            }
            if jobs.is_empty() {
                return;
            }
            let mut workers = JoinSet::new();
            for job in jobs {
                let service = self.clone();
                workers.spawn(async move { (job.clone(), service.download_claimed_book(job).await) });
            }
            while let Some(result) = workers.join_next().await {
                if let Ok((job, Err(error))) = result {
                    tracing::warn!(source = job.source_id.as_str(), %error, "catalogue book download failed");
                    let message = error.to_string();
                    let (category, retryable) = classify_catalog_failure(&message);
                    let cache_source = job.source_id.clone();
                    let cache_url = job.acquisition_url.clone();
                    let _ = self.store_write_call(move |cache| cache.fail_epub_extraction(&cache_source, &cache_url, job.attempt, &message, category, retryable)).await;
                }
            }
        }
    }

    pub(crate) async fn enrich_next_book(&self, source_id: &CatalogSourceId) -> bool {
        // Bound claimed rows and completed temporary files together. Jobs that
        // cannot enter the disk-backed pipeline remain durably pending.
        let _queued_permit = match self.state.queued_download_slots.acquire().await {
            Ok(permit) => permit,
            Err(error) => {
                tracing::warn!(source = source_id.as_str(), %error, "catalogue download queue closed");
                return false;
            }
        };
        let cache_source = source_id.clone();
        let job = match self.store_write_call(move |cache| cache.claim_next_epub_extraction(&cache_source)).await {
            Ok(Some(job)) => job,
            Ok(None) => return false,
            Err(error) => {
                tracing::warn!(source = source_id.as_str(), %error, "could not claim a queued book extraction");
                return false;
            }
        };
        if let Err(error) = self.enrich_claimed_book(job).await {
            tracing::warn!(source = source_id.as_str(), %error, "could not enrich active catalogue book from embedded metadata");
        }
        true
    }

    pub(crate) async fn enrich_active_book(&self, source_id: &CatalogSourceId, identity: CatalogBookIdentity, acquisition: OpdsDirectAcquisition, embedded_cover_key: Option<String>) -> Result<(), CatalogServiceError> {
        let _queued_permit = self.state.queued_download_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let acquisition_url = acquisition.url.clone();
        let cache_source = source_id.clone();
        let cache_url = acquisition_url.clone();
        let Some(attempt) = self.store_write_call(move |cache| cache.begin_epub_extraction(&cache_source, &cache_url)).await? else {
            return Ok(());
        };
        self.enrich_claimed_book_with(source_id, identity, acquisition, embedded_cover_key, attempt).await
    }

    pub(crate) async fn enrich_claimed_book(&self, job: CatalogEpubExtractionJob) -> Result<(), CatalogServiceError> {
        if job.staged_path.is_some() {
            return self.extract_staged_book(job).await;
        }
        let book_format = extraction_book_format(job.book_format).ok_or_else(|| CatalogServiceError("queued book has an unsupported extraction format".to_owned()))?;
        let digest = blake3::hash(job.acquisition_url.as_bytes()).to_hex();
        let acquisition = OpdsDirectAcquisition { url: job.acquisition_url, kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None };
        self.enrich_claimed_book_with(&job.source_id, job.book, acquisition, job.cover_key, job.attempt).await
    }

    async fn download_claimed_book(&self, job: CatalogEpubExtractionJob) -> Result<(), CatalogServiceError> {
        let book_format = extraction_book_format(job.book_format).ok_or_else(|| CatalogServiceError("queued book has an unsupported extraction format".to_owned()))?;
        let digest = blake3::hash(format!("{}\0{}\0{}", job.source_id.as_str(), job.acquisition_url, job.attempt).as_bytes()).to_hex();
        let acquisition = OpdsDirectAcquisition { url: job.acquisition_url.clone(), kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None };
        let downloaded = self.download_catalog_metadata(&acquisition, job.cover_key.is_some(), true).await?;
        tokio::fs::create_dir_all(&self.state.acquisition_directory).await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let staged_path = self.state.acquisition_directory.join(format!("staged-{digest}.{}", book_format.extension()));
        let temporary_path = downloaded.path().to_owned();
        tokio::fs::rename(&temporary_path, &staged_path).await.map_err(|error| CatalogServiceError(format!("could not persist staged acquisition: {error}")))?;
        let staged_path_string = staged_path.to_string_lossy().into_owned();
        let cache_source = job.source_id.clone();
        let cache_url = job.acquisition_url.clone();
        if !self.store_write_call(move |cache| cache.stage_epub_download(&cache_source, &cache_url, job.attempt, &staged_path_string, downloaded.is_metadata_only(), downloaded.transferred_bytes())).await? {
            let _ = tokio::fs::remove_file(&staged_path).await;
            return Err(CatalogServiceError("EPUB download claim was lost".to_owned()));
        }
        self.state.extraction_notify.notify_waiters();
        Ok(())
    }

    async fn extract_staged_book(&self, job: CatalogEpubExtractionJob) -> Result<(), CatalogServiceError> {
        let staged_path = job.staged_path.clone().ok_or_else(|| CatalogServiceError("staged extraction has no local path".to_owned()))?;
        let path = PathBuf::from(&staged_path);
        let book_format = extraction_book_format(job.book_format).ok_or_else(|| CatalogServiceError("queued book has an unsupported extraction format".to_owned()))?;
        let permit = self.state.extraction_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let mut extracted = extract_catalog_metadata_isolated(&path, book_format, job.cover_key.is_some()).await?;
        drop(permit);
        let bytes_source = job.source_id.clone();
        let bytes_url = job.acquisition_url.clone();
        let mut downloaded_bytes = self.store_call(move |cache| cache.epub_extraction_downloaded_bytes(&bytes_source, &bytes_url, job.attempt)).await?.unwrap_or(0);
        let acquisition = OpdsDirectAcquisition {
            url: job.acquisition_url.clone(),
            kind: OpdsAcquisitionKind::OpenAccess,
            book_format,
            suggested_file_name: path.file_name().and_then(|name| name.to_str()).unwrap_or("staged.epub").to_owned(),
            price: None,
        };
        if job.staged_metadata_only && job.cover_key.is_some() && extracted.thumbnail.is_none() {
            // The ranged archive did not contain usable artwork. This rare
            // fallback remains network-bounded, but it no longer occupies the
            // download queue while ordinary extraction runs.
            let full = self.download_catalog_metadata(&acquisition, true, false).await?;
            downloaded_bytes = downloaded_bytes.saturating_add(full.transferred_bytes());
            let permit = self.state.extraction_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
            extracted = extract_catalog_metadata_isolated(full.path(), book_format, true).await?;
            drop(permit);
        }
        self.state.extraction_metrics.record(&extracted.timings);
        let prepared = self.prepare_extracted_catalog_enrichment(&job.source_id, &acquisition, job.cover_key.as_deref(), job.attempt, extracted, downloaded_bytes).await?;
        let result = self.commit_catalog_enrichment(&job.source_id, job.book, &job.acquisition_url, job.attempt, prepared).await;
        let _ = tokio::fs::remove_file(path).await;
        result
    }

    pub(crate) async fn enrich_claimed_book_with(
        &self, source_id: &CatalogSourceId, identity: CatalogBookIdentity, acquisition: OpdsDirectAcquisition, embedded_cover_key: Option<String>, attempt: u64,
    ) -> Result<(), CatalogServiceError> {
        let acquisition_url = acquisition.url.clone();
        let result = self.run_claimed_book_enrichment(source_id, identity, &acquisition, embedded_cover_key.as_deref(), attempt).await;
        if let Err(error) = &result {
            let cache_source = source_id.clone();
            let cache_url = acquisition_url.clone();
            let message = error.to_string();
            let (category, retryable) = classify_catalog_failure(&message);
            if let Err(state_error) = self.store_write_call(move |cache| cache.fail_epub_extraction(&cache_source, &cache_url, attempt, &message, category, retryable)).await {
                tracing::warn!(source = source_id.as_str(), %state_error, "could not persist failed book extraction");
            }
        }
        if result.is_ok() {
            self.wake_process(CatalogProcess::Enrichment);
        }
        self.state.extraction_notify.notify_waiters();
        result
    }

    async fn run_claimed_book_enrichment(
        &self, source_id: &CatalogSourceId, identity: CatalogBookIdentity, acquisition: &OpdsDirectAcquisition, embedded_cover_key: Option<&str>, attempt: u64,
    ) -> Result<(), CatalogServiceError> {
        let prepared = self.prepare_catalog_enrichment(source_id, acquisition, embedded_cover_key, attempt).await?;
        self.commit_catalog_enrichment(source_id, identity, &acquisition.url, attempt, prepared).await
    }

    async fn download_and_extract_catalog_data(
        &self, source_id: &CatalogSourceId, acquisition: &OpdsDirectAcquisition, extract_cover: bool, attempt: u64,
    ) -> Result<(ExtractedCatalogData, opds::DownloadedOpdsBook, u64), CatalogServiceError> {
        let mut downloaded = self.download_catalog_metadata(acquisition, extract_cover, true).await?;
        let mut downloaded_bytes = downloaded.transferred_bytes();
        loop {
            let cache_source = source_id.clone();
            let cache_url = acquisition.url.clone();
            if !self.store_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, attempt, "extracting")).await? {
                return Err(CatalogServiceError("EPUB extraction claim was lost".to_owned()));
            }
            let extraction_permit = self.state.extraction_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
            let path = downloaded.path().to_owned();
            let extracted = extract_catalog_metadata_isolated(&path, acquisition.book_format, extract_cover).await?;
            if downloaded.is_metadata_only() && extract_cover && extracted.thumbnail.is_none() {
                // Ranged metadata succeeded but the declared artwork was
                // absent or unsuitable. Retry this book in full.
                drop(extraction_permit);
                drop(downloaded);
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                if !self.store_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, attempt, "downloading")).await? {
                    return Err(CatalogServiceError("EPUB extraction claim was lost".to_owned()));
                }
                downloaded = self.download_catalog_metadata(acquisition, extract_cover, false).await?;
                downloaded_bytes = downloaded_bytes.saturating_add(downloaded.transferred_bytes());
                continue;
            }
            drop(extraction_permit);
            return Ok((extracted, downloaded, downloaded_bytes));
        }
    }

    async fn prepare_catalog_enrichment(&self, source_id: &CatalogSourceId, acquisition: &OpdsDirectAcquisition, embedded_cover_key: Option<&str>, attempt: u64) -> Result<PreparedCatalogEnrichment, CatalogServiceError> {
        let (extracted, _downloaded, downloaded_bytes) = self.download_and_extract_catalog_data(source_id, acquisition, embedded_cover_key.is_some(), attempt).await?;
        self.prepare_extracted_catalog_enrichment(source_id, acquisition, embedded_cover_key, attempt, extracted, downloaded_bytes).await
    }

    async fn prepare_extracted_catalog_enrichment(
        &self, source_id: &CatalogSourceId, acquisition: &OpdsDirectAcquisition, embedded_cover_key: Option<&str>, attempt: u64, extracted: ExtractedCatalogData, downloaded_bytes: u64,
    ) -> Result<PreparedCatalogEnrichment, CatalogServiceError> {
        let cache_source = source_id.clone();
        let cache_url = acquisition.url.clone();
        if !self.store_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, attempt, "enriching")).await? {
            return Err(CatalogServiceError("EPUB extraction claim was lost".to_owned()));
        }

        let ExtractedCatalogData { metadata, isbns: _, subjects, saved_metadata, thumbnail, .. } = extracted;
        let cover_is_definitively_absent = self.resolve_extracted_cover(source_id, embedded_cover_key, thumbnail).await;
        // Keep extraction strictly cheap. ISBN/title-author resolution and
        // remote classification are scheduled by the catalogue backfill only
        // after the durable extraction queue has drained for this snapshot.
        Ok(PreparedCatalogEnrichment { metadata, saved_metadata, subjects, popularity: None, external_description: None, cover_is_definitively_absent, downloaded_bytes })
    }

    async fn resolve_extracted_cover(&self, source_id: &CatalogSourceId, embedded_cover_key: Option<&str>, thumbnail: Option<Vec<u8>>) -> bool {
        match (embedded_cover_key, thumbnail) {
            (Some(key), Some(thumbnail)) => {
                self.state.covers.remember_cover_url(source_id, key);
                if let Err(error) = self.state.covers.cover_with(source_id, key, async move { Ok(thumbnail) }).await {
                    tracing::warn!(source = source_id.as_str(), %error, "could not cache embedded book cover during metadata enrichment");
                }
                false
            }
            (Some(key), None) if is_synthetic_cover_key(key) => true,
            (Some(key), None) => match self.state.covers.cached_cover(key).await {
                Ok(_) => false,
                Err(_) => match self.state.covers.cover(source_id, key).await {
                    Ok(_) => false,
                    Err(error) if error.is_unsuitable_cover_dimensions() => true,
                    Err(_) => false,
                },
            },
            (None, _) => true,
        }
    }

    async fn commit_catalog_enrichment(&self, source_id: &CatalogSourceId, identity: CatalogBookIdentity, acquisition_url: &str, attempt: u64, prepared: PreparedCatalogEnrichment) -> Result<(), CatalogServiceError> {
        let extracted_subjects = prepared.subjects;
        // Subject projection may initialize the embedded taxonomy, so do it
        // before taking either the source update lock or SQLite writer lock.
        let _ = prepare_catalog_subject_assignments(&extracted_subjects);
        // Activation and the final enriched update for one source must not
        // overlap. Re-read after locking so this targets the newest generation.
        let update_lock = self.source_update_lock(source_id).await;
        let _update = update_lock.lock().await;
        let cache_source = source_id.clone();
        let cache_identity = identity;
        let mut book = self.store_call(move |cache| cache.active_book(&cache_source, &cache_identity)).await?.ok_or_else(|| CatalogServiceError("EPUB book is no longer active".to_owned()))?;
        if !book_has_acquisition_url(&book, acquisition_url) {
            return Err(CatalogServiceError("EPUB acquisition is no longer active".to_owned()));
        }
        apply_catalog_metadata(&mut book, prepared.metadata);
        if let Some(metadata) = prepared.saved_metadata.as_ref() {
            merge_catalog_epub_metadata(&mut book, metadata);
        }
        merge_catalog_book_subjects(&mut book, &extracted_subjects);
        let cache_subjects = book
            .metadata
            .subjects
            .iter()
            .map(|subject| BookSubject::new(None, &subject.name, &subject.source, subject.authority.clone(), subject.code.clone()).map_err(|error| CatalogServiceError(error.to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        let assignments = prepare_catalog_subject_assignments(&cache_subjects);
        apply_catalog_description_if_missing(&mut book, prepared.external_description);
        if prepared.cover_is_definitively_absent {
            book.image_url = None;
        }
        let toc = prepared.saved_metadata.as_ref().map(|metadata| metadata.toc.clone()).unwrap_or_default();
        let cache_source = source_id.clone();
        let cache_book = book.clone();
        let cache_popularity = prepared.popularity;
        let cache_claim_url = acquisition_url.to_owned();
        if !self.store_write_call(move |cache| cache.enrich_active_book_prepared(&cache_source, &cache_book, &cache_subjects, &assignments, &toc, cache_popularity.as_ref(), Some((&cache_claim_url, attempt)))).await? {
            return Err(CatalogServiceError("EPUB book is no longer active".to_owned()));
        }
        let cache_source = source_id.clone();
        let cache_url = acquisition_url.to_owned();
        if !self.store_write_call(move |cache| cache.finish_epub_extraction(&cache_source, &cache_url, attempt, prepared.downloaded_bytes, &book, prepared.saved_metadata.as_ref())).await? {
            return Err(CatalogServiceError("EPUB extraction claim was lost".to_owned()));
        }
        Ok(())
    }

    pub(crate) async fn download_catalog_metadata(&self, acquisition: &OpdsDirectAcquisition, include_cover: bool, prefer_range: bool) -> Result<opds::DownloadedOpdsBook, CatalogServiceError> {
        let _download_permit = self.state.background_download_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let operation = async {
            let range_origin = Url::parse(&acquisition.url).ok().map(|url| url.origin().ascii_serialization());
            let range_disabled = match range_origin.as_deref() {
                Some(origin) => self.state.epub_range_origins.lock().await.get(origin).is_some_and(|supported| !supported),
                None => false,
            };
            if prefer_range && acquisition.book_format == OpdsBookFormat::Epub && !range_disabled {
                match self.state.opds.download_epub_metadata_with_limit(acquisition, &self.state.acquisition_directory, metadata_book_byte_limit(acquisition.book_format), include_cover).await {
                    Ok(downloaded) => {
                        if let Some(origin) = range_origin {
                            self.state.epub_range_origins.lock().await.insert(origin, true);
                        }
                        return Ok(downloaded);
                    }
                    Err(error) => {
                        if matches!(error, opds::OpdsError::RangeUnsupported) {
                            if let Some(origin) = range_origin {
                                self.state.epub_range_origins.lock().await.insert(origin, false);
                            }
                        }
                        tracing::debug!(url = acquisition.url, %error, "ranged EPUB metadata acquisition fell back to a full download");
                    }
                }
            }
            self.state.opds.download_book_with_limit(acquisition, &self.state.acquisition_directory, metadata_book_byte_limit(acquisition.book_format)).await.map_err(|error| CatalogServiceError(error.to_string()))
        };
        tokio::time::timeout(CATALOG_METADATA_DOWNLOAD_TIMEOUT, operation).await.map_err(|_| CatalogServiceError(format!("book download exceeded {} seconds", CATALOG_METADATA_DOWNLOAD_TIMEOUT.as_secs())))?
    }
}

pub(crate) fn merge_catalog_book_subjects(book: &mut catalog_model::CatalogBook, subjects: &[BookSubject]) {
    for subject in subjects {
        let candidate = subject.clone();
        if !book.metadata.subjects.contains(&candidate) {
            book.metadata.subjects.push(candidate);
        }
    }
}

pub(crate) fn is_synthetic_cover_key(value: &str) -> bool {
    value.starts_with(EPUB_COVER_KEY_PREFIX)
}

pub(crate) fn generate_book_thumbnail(path: &Path) -> Result<Option<Vec<u8>>, String> {
    thumbnail::generate_thumbnail(path).map_err(|error| error.to_string())
}

#[derive(Deserialize, Serialize)]
pub(crate) struct ExtractedCatalogData {
    pub(crate) metadata: ExtractedCatalogMetadata,
    pub(crate) isbns: Vec<String>,
    pub(crate) subjects: Vec<BookSubject>,
    pub(crate) saved_metadata: Option<CatalogEpubMetadata>,
    pub(crate) thumbnail: Option<Vec<u8>>,
    #[serde(default)]
    pub(crate) timings: CatalogExtractionTimings,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct CatalogExtractionTimings {
    pub(crate) container_ms: u64,
    pub(crate) evidence_ms: u64,
    pub(crate) toc_ms: u64,
    pub(crate) thumbnail_ms: u64,
    pub(crate) worker_ms: u64,
}

/// The only data OCR is allowed to return to the enrichment layer. In
/// particular, OCR cannot carry embedded metadata or a table of contents.
#[derive(Deserialize, Serialize)]
pub(crate) struct ExtractedCatalogOcrData {
    pub(crate) subjects: Vec<BookSubject>,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct ExtractedCatalogMetadata {
    pub(crate) title: Option<String>,
    pub(crate) authors: Vec<CatalogAuthorCredit>,
    pub(crate) languages: Vec<LanguageTag>,
    pub(crate) description: Option<String>,
}

pub(crate) fn extract_catalog_metadata(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    match book_format {
        OpdsBookFormat::Epub => extract_epub_catalog_metadata(path, extract_cover),
        OpdsBookFormat::Mobi => extract_mobi_catalog_metadata(path, extract_cover),
        OpdsBookFormat::Pdf => extract_pdf_catalog_metadata(path, extract_cover),
        _ => Err(format!("catalog metadata extraction is unsupported for {}", book_format.extension())),
    }
}

#[cfg(test)]
pub(crate) async fn extract_catalog_metadata_isolated(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, CatalogServiceError> {
    extract_catalog_mode_isolated(path, book_format, if extract_cover { "cover" } else { "metadata" }).await
}

pub(crate) async fn extract_catalog_ocr_isolated(path: &Path, book_format: OpdsBookFormat) -> Result<ExtractedCatalogOcrData, CatalogServiceError> {
    #[cfg(test)]
    {
        let path = path.to_owned();
        return tokio::task::spawn_blocking(move || extract_catalog_ocr(&path, book_format)).await.map_err(|error| CatalogServiceError(error.to_string()))?.map_err(CatalogServiceError);
    }
    #[cfg(not(test))]
    {
        let bytes = run_isolated_extraction_worker(path, book_format, "ocr").await?;
        return wire::decode(&bytes, MAX_EXTRACTION_RESULT_BYTES).map_err(|error| CatalogServiceError(error.to_string()));
    }
}

#[cfg(test)]
pub(crate) async fn extract_catalog_mode_isolated(path: &Path, book_format: OpdsBookFormat, mode: &str) -> Result<ExtractedCatalogData, CatalogServiceError> {
    let path = path.to_owned();
    let pages = mode == "pages";
    let extract_cover = mode == "cover";
    let started = Instant::now();
    let mut result = tokio::task::spawn_blocking(move || if pages { extract_catalog_pages(&path, book_format) } else { extract_catalog_metadata(&path, book_format, extract_cover) })
        .await
        .map_err(|error| CatalogServiceError(error.to_string()))?
        .map_err(CatalogServiceError)?;
    result.timings.worker_ms = duration_millis(started.elapsed());
    Ok(result)
}

#[cfg(not(test))]
pub(crate) async fn extract_catalog_metadata_isolated(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, CatalogServiceError> {
    extract_catalog_mode_isolated(path, book_format, if extract_cover { "cover" } else { "metadata" }).await
}

#[cfg(not(test))]
pub(crate) async fn extract_catalog_mode_isolated(path: &Path, book_format: OpdsBookFormat, mode: &str) -> Result<ExtractedCatalogData, CatalogServiceError> {
    if mode == "ocr" {
        return Err(CatalogServiceError("OCR must use extract_catalog_ocr_isolated".to_owned()));
    }
    let started = Instant::now();
    let bytes = run_isolated_extraction_worker(path, book_format, mode).await?;
    let mut result: ExtractedCatalogData = wire::decode(&bytes, MAX_EXTRACTION_RESULT_BYTES).map_err(|error| CatalogServiceError(error.to_string()))?;
    result.timings.worker_ms = duration_millis(started.elapsed());
    Ok(result)
}

#[cfg(not(test))]
async fn run_isolated_extraction_worker(path: &Path, book_format: OpdsBookFormat, mode: &str) -> Result<Vec<u8>, CatalogServiceError> {
    let executable = std::env::current_exe().map_err(|error| CatalogServiceError(error.to_string()))?;
    let output = tempfile::NamedTempFile::new().map_err(|error| CatalogServiceError(error.to_string()))?;
    let output_path = output.path().to_owned();
    let mut command = tokio::process::Command::new("/usr/bin/prlimit");
    command
        .arg(format!("--as={EXTRACTION_WORKER_MEMORY_BYTES}"))
        .arg(format!("--cpu={EXTRACTION_WORKER_CPU_SECONDS}"))
        .arg("--")
        .arg(executable)
        .arg("--catalog-extraction-worker")
        .arg(path)
        .arg(book_format.extension())
        .arg(mode)
        .arg(&output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(EXTRACTION_WORKER_TIMEOUT, command.status())
        .await
        .map_err(|_| CatalogServiceError("book extraction worker exceeded its time limit".to_owned()))?
        .map_err(|error| CatalogServiceError(format!("could not start book extraction worker: {error}")))?;
    if !status.success() {
        return Err(CatalogServiceError(format!("book extraction worker exceeded a resource limit or failed with {status}")));
    }
    let length = tokio::fs::metadata(&output_path).await.map_err(|error| CatalogServiceError(error.to_string()))?.len();
    if length > MAX_EXTRACTION_RESULT_BYTES as u64 {
        return Err(CatalogServiceError(format!("book extraction result exceeded the {MAX_EXTRACTION_RESULT_BYTES}-byte size limit")));
    }
    tokio::fs::read(&output_path).await.map_err(|error| CatalogServiceError(error.to_string()))
}

/// Internal subprocess entry point. Expensive and native format parsers run
/// here under an address-space and CPU limit so one malformed book can
/// never make the catalogue HTTP process unavailable.
pub fn run_catalog_extraction_worker(input: &Path, format: &str, mode: &str, output: &Path) -> Result<(), String> {
    let book_format = match format {
        "epub" => OpdsBookFormat::Epub,
        "mobi" => OpdsBookFormat::Mobi,
        "pdf" => OpdsBookFormat::Pdf,
        _ => return Err(format!("unsupported extraction worker format: {format}")),
    };
    let extract_cover = match mode {
        "cover" => true,
        "metadata" | "pages" | "ocr" => false,
        _ => return Err(format!("unsupported extraction worker mode: {mode}")),
    };
    let input_length = std::fs::metadata(input).map_err(|error| error.to_string())?.len();
    let input_limit = metadata_book_byte_limit(book_format);
    if input_length > input_limit {
        return Err(format!("book exceeds the {input_limit}-byte extraction size limit"));
    }
    let bytes = if mode == "ocr" {
        wire::encode(&extract_catalog_ocr(input, book_format)?)
    } else if mode == "pages" {
        wire::encode(&extract_catalog_pages(input, book_format)?)
    } else {
        wire::encode(&extract_catalog_metadata(input, book_format, extract_cover)?)
    };
    let bytes = bytes.map_err(|error| error.to_string())?;
    if bytes.len() > MAX_EXTRACTION_RESULT_BYTES {
        return Err(format!("book extraction result exceeded the {MAX_EXTRACTION_RESULT_BYTES}-byte size limit"));
    }
    std::fs::write(output, bytes).map_err(|error| error.to_string())
}

fn extract_catalog_ocr(path: &Path, book_format: OpdsBookFormat) -> Result<ExtractedCatalogOcrData, String> {
    match book_format {
        OpdsBookFormat::Epub => {
            let book = book_inspection::inspect_epub_ocr_reader(path, std::fs::File::open(path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
            Ok(ExtractedCatalogOcrData { subjects: book.book.subjects.clone() })
        }
        OpdsBookFormat::Pdf => {
            let book = book_inspection::inspect_pdf_ocr_reader(path, std::fs::File::open(path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
            Ok(ExtractedCatalogOcrData { subjects: book.metadata.book.subjects.clone() })
        }
        OpdsBookFormat::Mobi => Err("catalogue OCR is not supported for MOBI".to_owned()),
        _ => Err(format!("catalogue OCR is unsupported for {}", book_format.extension())),
    }
}

pub(crate) fn extract_epub_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    // Normal page evidence is still part of the cheap extraction wave: it
    // reads bounded EPUB text and never performs OCR. Keeping this here also
    // preserves checksum-validated back-matter ISBN/LCC evidence for the
    // later title/author and taxonomy stages. OCR remains isolated below.
    let inspected = book_inspection::inspect_epub(path).map_err(|error| error.to_string())?;
    let inspection_timings = inspected.timings;
    let book = inspected.metadata;
    let title = bounded_embedded_text(&book.title, MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors = book
        .authors()
        .map(|author| CatalogAuthorCredit {
            name: author.name().to_owned(),
            name_localizations: Vec::new(),
            sort_name: author.sort_name().map(str::to_owned),
            sort_name_localizations: Vec::new(),
            identifiers: Vec::new(),
            provider_link: None,
        })
        .collect();
    let languages = book.book.languages.clone();
    let description = bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true);
    let isbns = book.book.identifiers.iter().filter(|identifier| identifier.scheme() == &Scheme::Isbn).filter_map(|identifier| identifier.canonical_value()).collect::<HashSet<_>>().into_iter().collect();
    let subjects = book.book.subjects.clone();
    let metadata = ExtractedCatalogMetadata { title, authors, languages, description };
    let saved_metadata = Some(CatalogEpubMetadata { book, toc: inspected.toc });
    // Cover corruption must not discard otherwise valid bibliographic data.
    let thumbnail_started = Instant::now();
    let thumbnail = if extract_cover { generate_book_thumbnail(path).ok().flatten() } else { None };
    let thumbnail_ms = duration_millis(thumbnail_started.elapsed());
    Ok(ExtractedCatalogData {
        metadata,
        isbns,
        subjects,
        saved_metadata,
        thumbnail,
        timings: CatalogExtractionTimings {
            container_ms: inspection_timings.container_ms,
            evidence_ms: inspection_timings.evidence_ms,
            toc_ms: inspection_timings.toc_ms,
            thumbnail_ms,
            worker_ms: 0,
        },
    })
}

pub(crate) fn extract_mobi_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let book = book_inspection::inspect_mobi_metadata_reader(path, std::fs::File::open(path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let metadata = ExtractedCatalogMetadata {
        title: bounded_embedded_text(&book.title, MAX_EMBEDDED_METADATA_TEXT_BYTES, false),
        authors: book
            .authors()
            .map(|author| CatalogAuthorCredit {
                name: author.name().to_owned(),
                name_localizations: Vec::new(),
                sort_name: author.sort_name().map(str::to_owned),
                sort_name_localizations: Vec::new(),
                identifiers: Vec::new(),
                provider_link: None,
            })
            .collect(),
        languages: book.book.languages.clone(),
        description: bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true),
    };
    let isbns = book.book.identifiers.iter().filter(|identifier| identifier.scheme() == &Scheme::Isbn).filter_map(|identifier| identifier.canonical_value()).collect();
    let subjects = book.book.subjects.clone();
    let thumbnail = if extract_cover { generate_book_thumbnail(path).ok().flatten() } else { None };
    Ok(ExtractedCatalogData { metadata, isbns, subjects, saved_metadata: Some(CatalogEpubMetadata { book, toc: Vec::new() }), thumbnail, timings: CatalogExtractionTimings::default() })
}

#[cfg(test)]
pub(crate) fn mobi_catalog_metadata(mobi: &mobi::MobiMetadata) -> (ExtractedCatalogMetadata, Vec<String>, Vec<BookSubject>) {
    let title = bounded_embedded_text(&mobi.title(), MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors = mobi
        .author()
        .and_then(|author| bounded_embedded_text(&author, MAX_EMBEDDED_METADATA_TEXT_BYTES, false))
        .map(|name| CatalogAuthorCredit { name, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None })
        .into_iter()
        .collect();
    let description = mobi.description().and_then(|description| bounded_embedded_text(&description, MAX_EMBEDDED_DESCRIPTION_BYTES, true));
    let isbns = mobi.isbn().and_then(|isbn| book_model::canonical_isbn(&isbn)).into_iter().collect();
    let subjects = mobi
        .subjects()
        .unwrap_or_default()
        .into_iter()
        .take(256)
        .filter_map(|subject| {
            let subject = bounded_embedded_text(&subject, MAX_EMBEDDED_METADATA_TEXT_BYTES, false)?;
            BookSubject::new(None, subject, "mobi:subject", None, None).ok()
        })
        .collect();
    (ExtractedCatalogMetadata { title, authors, languages: Vec::new(), description }, isbns, subjects)
}

pub(crate) fn extract_pdf_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let book = book_inspection::inspect_pdf_metadata(path).map_err(|error| error.to_string())?.metadata;
    let title = bounded_embedded_text(&book.title, MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors = book
        .authors()
        .map(|author| CatalogAuthorCredit {
            name: author.name().to_owned(),
            name_localizations: Vec::new(),
            sort_name: author.sort_name().map(str::to_owned),
            sort_name_localizations: Vec::new(),
            identifiers: Vec::new(),
            provider_link: None,
        })
        .collect();
    let languages = book.book.languages.clone();
    let description = bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true);
    let isbns = book.book.identifiers.iter().filter(|identifier| identifier.scheme() == &Scheme::Isbn).filter_map(Identifier::canonical_value).collect::<HashSet<_>>().into_iter().collect();
    let subjects = book.book.subjects.clone();
    // PDF rasterization is not metadata extraction. It has a much larger and
    // less predictable native working set, so covers are handled only by the
    // independently bounded cover pipeline.
    let _ = extract_cover;
    let thumbnail = None;
    let saved_metadata = Some(CatalogEpubMetadata { book, toc: Vec::new() });
    Ok(ExtractedCatalogData { metadata: ExtractedCatalogMetadata { title, authors, languages, description }, isbns, subjects, saved_metadata, thumbnail, timings: CatalogExtractionTimings::default() })
}

pub(crate) fn bounded_embedded_text(value: &str, max_bytes: usize, multiline: bool) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || value.chars().any(|character| character.is_control() && !(multiline && matches!(character, '\n' | '\r' | '\t'))) {
        return None;
    }
    Some(if multiline { value.to_owned() } else { value.split_whitespace().collect::<Vec<_>>().join(" ") })
}

fn extract_catalog_pages(path: &Path, format: OpdsBookFormat) -> Result<ExtractedCatalogData, String> {
    let book = match format {
        OpdsBookFormat::Epub => book_inspection::inspect_epub_pages_reader(path, std::fs::File::open(path).map_err(|e| e.to_string())?),
        OpdsBookFormat::Pdf => book_inspection::inspect_pdf_pages_reader(path, std::fs::File::open(path).map_err(|e| e.to_string())?).map(|p| p.metadata),
        _ => return Err("page inspection requires EPUB or PDF".into()),
    }
    .map_err(|e| e.to_string())?;
    Ok(ExtractedCatalogData { metadata: Default::default(), isbns: Vec::new(), subjects: book.book.subjects.clone(), saved_metadata: Some(CatalogEpubMetadata { book, toc: Vec::new() }), thumbnail: None, timings: CatalogExtractionTimings::default() })
}

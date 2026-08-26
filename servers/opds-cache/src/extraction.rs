use super::{
    apply_catalog_description_if_missing, apply_catalog_metadata, canonical_isbn, classify_catalog_failure, extraction_book_format, merge_catalog_epub_metadata, merge_catalog_popularity, metadata_publication_byte_limit,
    opds_catalog_source_id, prepare_catalog_subject_assignments, publication_has_acquisition_url, CatalogAuthorCredit, CatalogEpubExtractionJob, CatalogEpubMetadata, CatalogProcess, CatalogPublicationIdentity, CatalogPublicationPopularity,
    CatalogService, CatalogServiceError, CatalogSourceId, Deserialize, HashSet, Identifier, LanguageTag, OpdsAcquisitionKind, OpdsBookFormat, OpdsDirectAcquisition, Path, PublicationSubject, Scheme, Serialize, Url,
    CATALOG_METADATA_DOWNLOAD_TIMEOUT, EPUB_COVER_KEY_PREFIX, MAX_EMBEDDED_DESCRIPTION_BYTES, MAX_EMBEDDED_METADATA_TEXT_BYTES, MAX_EXTRACTION_RESULT_BYTES,
};
#[cfg(not(test))]
use super::{EXTRACTION_WORKER_CPU_SECONDS, EXTRACTION_WORKER_MEMORY_BYTES, EXTRACTION_WORKER_TIMEOUT};
#[cfg(not(test))]
use std::process::Stdio;
struct PreparedCatalogEnrichment {
    metadata: ExtractedCatalogMetadata,
    saved_metadata: Option<CatalogEpubMetadata>,
    subjects: Vec<PublicationSubject>,
    popularity: Option<CatalogPublicationPopularity>,
    external_description: Option<String>,
    cover_is_definitively_absent: bool,
    downloaded_bytes: u64,
}

impl CatalogService {
    pub(crate) async fn resume_pending_epub_extractions(&self) {
        if !self.process_enabled(CatalogProcess::EpubEnrichment) {
            return;
        }
        let source_ids = self.state.endpoints.read().await.iter().map(|endpoint| opds_catalog_source_id(&endpoint.id)).collect::<Vec<_>>();
        for source_id in &source_ids {
            let cache_source = source_id.clone();
            if let Err(error) = self.cache_write_call(move |cache| cache.retry_failed_epub_extractions(&cache_source)).await {
                tracing::warn!(source = source_id.as_str(), %error, "could not schedule due publication extraction retries");
            }
        }
        loop {
            if !self.process_enabled(CatalogProcess::EpubEnrichment) {
                return;
            }
            let mut progressed = false;
            for source_id in &source_ids {
                if !self.process_enabled(CatalogProcess::EpubEnrichment) {
                    return;
                }
                progressed |= self.enrich_next_publication(source_id).await;
            }
            if !progressed {
                break;
            }
        }
    }

    pub(crate) async fn enrich_next_publication(&self, source_id: &CatalogSourceId) -> bool {
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
        let job = match self.cache_write_call(move |cache| cache.claim_next_epub_extraction(&cache_source)).await {
            Ok(Some(job)) => job,
            Ok(None) => return false,
            Err(error) => {
                tracing::warn!(source = source_id.as_str(), %error, "could not claim a queued publication extraction");
                return false;
            }
        };
        if let Err(error) = self.enrich_claimed_publication(job).await {
            tracing::warn!(source = source_id.as_str(), %error, "could not enrich active catalogue publication from embedded metadata");
        }
        true
    }

    pub(crate) async fn enrich_active_publication(&self, source_id: &CatalogSourceId, identity: CatalogPublicationIdentity, acquisition: OpdsDirectAcquisition, embedded_cover_key: Option<String>) -> Result<(), CatalogServiceError> {
        let _queued_permit = self.state.queued_download_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let acquisition_url = acquisition.url.clone();
        let cache_source = source_id.clone();
        let cache_url = acquisition_url.clone();
        let claimed = self.cache_write_call(move |cache| cache.begin_epub_extraction(&cache_source, &cache_url)).await?;
        if !claimed {
            return Ok(());
        }
        self.enrich_claimed_publication_with(source_id, identity, acquisition, embedded_cover_key).await
    }

    pub(crate) async fn enrich_claimed_publication(&self, job: CatalogEpubExtractionJob) -> Result<(), CatalogServiceError> {
        let book_format = extraction_book_format(job.book_format).ok_or_else(|| CatalogServiceError("queued publication has an unsupported extraction format".to_owned()))?;
        let digest = blake3::hash(job.acquisition_url.as_bytes()).to_hex();
        let acquisition = OpdsDirectAcquisition { url: job.acquisition_url, kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None };
        self.enrich_claimed_publication_with(&job.source_id, job.publication, acquisition, job.cover_key).await
    }

    pub(crate) async fn enrich_claimed_publication_with(&self, source_id: &CatalogSourceId, identity: CatalogPublicationIdentity, acquisition: OpdsDirectAcquisition, embedded_cover_key: Option<String>) -> Result<(), CatalogServiceError> {
        let acquisition_url = acquisition.url.clone();
        let result = self.run_claimed_publication_enrichment(source_id, identity, &acquisition, embedded_cover_key.as_deref()).await;
        if let Err(error) = &result {
            let cache_source = source_id.clone();
            let cache_url = acquisition_url.clone();
            let message = error.to_string();
            let (category, retryable) = classify_catalog_failure(&message);
            if let Err(state_error) = self.cache_write_call(move |cache| cache.fail_epub_extraction(&cache_source, &cache_url, &message, category, retryable)).await {
                tracing::warn!(source = source_id.as_str(), %state_error, "could not persist failed publication extraction");
            }
        }
        if result.is_ok() {
            self.wake_process(CatalogProcess::CatalogueEnrichment);
        }
        self.state.extraction_notify.notify_waiters();
        result
    }

    async fn run_claimed_publication_enrichment(&self, source_id: &CatalogSourceId, identity: CatalogPublicationIdentity, acquisition: &OpdsDirectAcquisition, embedded_cover_key: Option<&str>) -> Result<(), CatalogServiceError> {
        let prepared = self.prepare_catalog_enrichment(source_id, acquisition, embedded_cover_key).await?;
        self.commit_catalog_enrichment(source_id, identity, &acquisition.url, prepared).await
    }

    async fn download_and_extract_catalog_data(&self, source_id: &CatalogSourceId, acquisition: &OpdsDirectAcquisition, extract_cover: bool) -> Result<(ExtractedCatalogData, u64), CatalogServiceError> {
        let mut downloaded = self.download_catalog_metadata(acquisition, extract_cover, true).await?;
        let mut downloaded_bytes = downloaded.transferred_bytes();
        loop {
            let cache_source = source_id.clone();
            let cache_url = acquisition.url.clone();
            self.cache_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, "extracting")).await?;
            let extraction_permit = self.state.extraction_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
            let path = downloaded.path().to_owned();
            let extracted = extract_catalog_metadata_isolated(&path, acquisition.book_format, extract_cover).await?;
            if downloaded.is_metadata_only() && extract_cover && extracted.thumbnail.is_none() {
                // Ranged metadata succeeded but the declared artwork was
                // absent or unsuitable. Retry this publication in full.
                drop(extraction_permit);
                drop(downloaded);
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                self.cache_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, "downloading")).await?;
                downloaded = self.download_catalog_metadata(acquisition, extract_cover, false).await?;
                downloaded_bytes = downloaded_bytes.saturating_add(downloaded.transferred_bytes());
                continue;
            }
            drop(extraction_permit);
            drop(downloaded);
            return Ok((extracted, downloaded_bytes));
        }
    }

    async fn prepare_catalog_enrichment(&self, source_id: &CatalogSourceId, acquisition: &OpdsDirectAcquisition, embedded_cover_key: Option<&str>) -> Result<PreparedCatalogEnrichment, CatalogServiceError> {
        let (extracted, downloaded_bytes) = self.download_and_extract_catalog_data(source_id, acquisition, embedded_cover_key.is_some()).await?;
        let cache_source = source_id.clone();
        let cache_url = acquisition.url.clone();
        self.cache_write_call(move |cache| cache.set_epub_extraction_phase(&cache_source, &cache_url, "enriching")).await?;

        let ExtractedCatalogData { metadata, mut isbns, mut subjects, mut saved_metadata, thumbnail } = extracted;
        let cover_is_definitively_absent = self.resolve_extracted_cover(source_id, embedded_cover_key, thumbnail).await;
        if isbns.is_empty() {
            if let Some(saved) = saved_metadata.as_mut() {
                match self.infer_catalog_epub_isbn(saved).await {
                    Ok(Some(isbn)) => isbns.push(isbn),
                    Ok(None) => {}
                    Err(error) => tracing::warn!(source = source_id.as_str(), %error, "could not infer catalogue EPUB ISBN from metadata snapshot"),
                }
            }
        }
        let (popularity, external_description) = self.extend_with_external_metadata(source_id, &isbns, &mut subjects).await;
        Ok(PreparedCatalogEnrichment { metadata, saved_metadata, subjects, popularity, external_description, cover_is_definitively_absent, downloaded_bytes })
    }

    async fn resolve_extracted_cover(&self, source_id: &CatalogSourceId, embedded_cover_key: Option<&str>, thumbnail: Option<Vec<u8>>) -> bool {
        match (embedded_cover_key, thumbnail) {
            (Some(key), Some(thumbnail)) => {
                self.state.covers.remember_cover_url(source_id, key);
                if let Err(error) = self.state.covers.cover_with(source_id, key, async move { Ok(thumbnail) }).await {
                    tracing::warn!(source = source_id.as_str(), %error, "could not cache embedded publication cover during metadata enrichment");
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

    async fn extend_with_external_metadata(&self, source_id: &CatalogSourceId, isbns: &[String], subjects: &mut Vec<PublicationSubject>) -> (Option<CatalogPublicationPopularity>, Option<String>) {
        let mut popularity = None;
        let mut external_description = None;
        if !isbns.is_empty() {
            match self.metadata_matches(isbns).await {
                Ok(external) => {
                    for isbn in isbns {
                        if let Some(enriched) = external.get(isbn) {
                            subjects.extend(enriched.subjects.iter().cloned());
                            if let Some(candidate) = &enriched.popularity {
                                merge_catalog_popularity(&mut popularity, candidate);
                            }
                            if external_description.is_none() {
                                external_description = enriched.description.clone();
                            }
                        }
                    }
                    subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
                    subjects.dedup();
                }
                Err(error) => tracing::warn!(source = source_id.as_str(), %error, "could not enrich catalogue subjects from metadata snapshot"),
            }
        }
        (popularity, external_description)
    }

    async fn commit_catalog_enrichment(&self, source_id: &CatalogSourceId, identity: CatalogPublicationIdentity, acquisition_url: &str, prepared: PreparedCatalogEnrichment) -> Result<(), CatalogServiceError> {
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
        let mut publication = self.cache_call(move |cache| cache.active_publication(&cache_source, &cache_identity)).await?.ok_or_else(|| CatalogServiceError("EPUB publication is no longer active".to_owned()))?;
        if !publication_has_acquisition_url(&publication, acquisition_url) {
            return Err(CatalogServiceError("EPUB acquisition is no longer active".to_owned()));
        }
        apply_catalog_metadata(&mut publication, prepared.metadata);
        if let Some(metadata) = prepared.saved_metadata.as_ref() {
            merge_catalog_epub_metadata(&mut publication, metadata);
        }
        merge_catalog_publication_subjects(&mut publication, &extracted_subjects);
        let cache_subjects = publication
            .metadata
            .subjects
            .iter()
            .map(|subject| PublicationSubject::new(None, &subject.name, &subject.source, subject.authority.clone(), subject.code.clone()).map_err(|error| CatalogServiceError(error.to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        let assignments = prepare_catalog_subject_assignments(&cache_subjects);
        apply_catalog_description_if_missing(&mut publication, prepared.external_description);
        if prepared.cover_is_definitively_absent {
            publication.image_url = None;
        }
        let toc = prepared.saved_metadata.as_ref().map(|metadata| metadata.toc.clone()).unwrap_or_default();
        let cache_source = source_id.clone();
        let cache_publication = publication.clone();
        let cache_popularity = prepared.popularity;
        if !self.cache_write_call(move |cache| cache.enrich_active_publication_prepared(&cache_source, &cache_publication, &cache_subjects, &assignments, &toc, cache_popularity.as_ref())).await? {
            return Err(CatalogServiceError("EPUB publication is no longer active".to_owned()));
        }
        let cache_source = source_id.clone();
        let cache_url = acquisition_url.to_owned();
        self.cache_write_call(move |cache| cache.finish_epub_extraction(&cache_source, &cache_url, prepared.downloaded_bytes, &publication, prepared.saved_metadata.as_ref())).await
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
                match self.state.opds.download_epub_metadata_with_limit(acquisition, &self.state.acquisition_directory, metadata_publication_byte_limit(acquisition.book_format), include_cover).await {
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
            self.state.opds.download_book_with_limit(acquisition, &self.state.acquisition_directory, metadata_publication_byte_limit(acquisition.book_format)).await.map_err(|error| CatalogServiceError(error.to_string()))
        };
        tokio::time::timeout(CATALOG_METADATA_DOWNLOAD_TIMEOUT, operation).await.map_err(|_| CatalogServiceError(format!("publication download exceeded {} seconds", CATALOG_METADATA_DOWNLOAD_TIMEOUT.as_secs())))?
    }
}

fn merge_catalog_publication_subjects(publication: &mut catalog_contract::CatalogPublication, subjects: &[PublicationSubject]) {
    for subject in subjects {
        let candidate = subject.clone();
        if !publication.metadata.subjects.contains(&candidate) {
            publication.metadata.subjects.push(candidate);
        }
    }
}

pub(crate) fn is_synthetic_cover_key(value: &str) -> bool {
    value.starts_with(EPUB_COVER_KEY_PREFIX)
}

pub(crate) fn generate_publication_thumbnail(path: &Path) -> Result<Option<Vec<u8>>, String> {
    thumbnail::generate_thumbnail(path).map_err(|error| error.to_string())
}

#[derive(Deserialize, Serialize)]
pub(crate) struct ExtractedCatalogData {
    pub(crate) metadata: ExtractedCatalogMetadata,
    pub(crate) isbns: Vec<String>,
    pub(crate) subjects: Vec<PublicationSubject>,
    pub(crate) saved_metadata: Option<CatalogEpubMetadata>,
    pub(crate) thumbnail: Option<Vec<u8>>,
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
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || extract_catalog_metadata(&path, book_format, extract_cover)).await.map_err(|error| CatalogServiceError(error.to_string()))?.map_err(CatalogServiceError)
}

#[cfg(not(test))]
pub(crate) async fn extract_catalog_metadata_isolated(path: &Path, book_format: OpdsBookFormat, extract_cover: bool) -> Result<ExtractedCatalogData, CatalogServiceError> {
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
        .arg(if extract_cover { "cover" } else { "metadata" })
        .arg(&output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(EXTRACTION_WORKER_TIMEOUT, command.status())
        .await
        .map_err(|_| CatalogServiceError("publication extraction worker exceeded its time limit".to_owned()))?
        .map_err(|error| CatalogServiceError(format!("could not start publication extraction worker: {error}")))?;
    if !status.success() {
        return Err(CatalogServiceError(format!("publication extraction worker exceeded a resource limit or failed with {status}")));
    }
    let length = tokio::fs::metadata(&output_path).await.map_err(|error| CatalogServiceError(error.to_string()))?.len();
    if length > MAX_EXTRACTION_RESULT_BYTES as u64 {
        return Err(CatalogServiceError(format!("publication extraction result exceeded the {MAX_EXTRACTION_RESULT_BYTES}-byte size limit")));
    }
    let bytes = tokio::fs::read(&output_path).await.map_err(|error| CatalogServiceError(error.to_string()))?;
    wire::decode(&bytes, MAX_EXTRACTION_RESULT_BYTES).map_err(|error| CatalogServiceError(error.to_string()))
}

/// Internal subprocess entry point. Expensive and native format parsers run
/// here under an address-space and CPU limit so one malformed publication can
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
        "metadata" => false,
        _ => return Err(format!("unsupported extraction worker mode: {mode}")),
    };
    let input_length = std::fs::metadata(input).map_err(|error| error.to_string())?.len();
    let input_limit = metadata_publication_byte_limit(book_format);
    if input_length > input_limit {
        return Err(format!("publication exceeds the {input_limit}-byte extraction size limit"));
    }
    let extracted = extract_catalog_metadata(input, book_format, extract_cover)?;
    let bytes = wire::encode(&extracted).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_EXTRACTION_RESULT_BYTES {
        return Err(format!("publication extraction result exceeded the {MAX_EXTRACTION_RESULT_BYTES}-byte size limit"));
    }
    std::fs::write(output, bytes).map_err(|error| error.to_string())
}

pub(crate) fn extract_epub_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let inspected = book_metadata::inspect_epub(path).map_err(|error| error.to_string())?;
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
    let languages = book.publication.languages.clone();
    let description = bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true);
    let isbns = book.publication.identifiers.iter().filter(|identifier| identifier.scheme() == &Scheme::Isbn).filter_map(|identifier| identifier.canonical_value()).collect::<HashSet<_>>().into_iter().collect();
    let subjects = book.publication.subjects.clone();
    let metadata = ExtractedCatalogMetadata { title, authors, languages, description };
    let saved_metadata = Some(CatalogEpubMetadata { book, toc: inspected.toc });
    // Cover corruption must not discard otherwise valid bibliographic data.
    let thumbnail = if extract_cover { generate_publication_thumbnail(path).ok().flatten() } else { None };
    Ok(ExtractedCatalogData { metadata, isbns, subjects, saved_metadata, thumbnail })
}

pub(crate) fn extract_mobi_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let mobi = mobi::MobiMetadata::from_path(path).map_err(|error| error.to_string())?;
    let (metadata, isbns, subjects) = mobi_catalog_metadata(&mobi);
    let thumbnail = if extract_cover { generate_publication_thumbnail(path).ok().flatten() } else { None };
    Ok(ExtractedCatalogData { metadata, isbns, subjects, saved_metadata: None, thumbnail })
}

pub(crate) fn mobi_catalog_metadata(mobi: &mobi::MobiMetadata) -> (ExtractedCatalogMetadata, Vec<String>, Vec<PublicationSubject>) {
    let title = bounded_embedded_text(&mobi.title(), MAX_EMBEDDED_METADATA_TEXT_BYTES, false);
    let authors = mobi
        .author()
        .and_then(|author| bounded_embedded_text(&author, MAX_EMBEDDED_METADATA_TEXT_BYTES, false))
        .map(|name| CatalogAuthorCredit { name, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None })
        .into_iter()
        .collect();
    let description = mobi.description().and_then(|description| bounded_embedded_text(&description, MAX_EMBEDDED_DESCRIPTION_BYTES, true));
    let isbns = mobi.isbn().and_then(|isbn| canonical_isbn(&isbn)).into_iter().collect();
    let subjects = mobi
        .subjects()
        .unwrap_or_default()
        .into_iter()
        .take(256)
        .filter_map(|subject| {
            let subject = bounded_embedded_text(&subject, MAX_EMBEDDED_METADATA_TEXT_BYTES, false)?;
            PublicationSubject::new(None, subject, "mobi:subject", None, None).ok()
        })
        .collect();
    (ExtractedCatalogMetadata { title, authors, languages: Vec::new(), description }, isbns, subjects)
}

pub(crate) fn extract_pdf_catalog_metadata(path: &Path, extract_cover: bool) -> Result<ExtractedCatalogData, String> {
    let book = book_metadata::inspect_pdf(path).map_err(|error| error.to_string())?.metadata;
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
    let languages = book.publication.languages.clone();
    let description = bounded_embedded_text(&book.description, MAX_EMBEDDED_DESCRIPTION_BYTES, true);
    let isbns = book.publication.identifiers.iter().filter(|identifier| identifier.scheme() == &Scheme::Isbn).filter_map(Identifier::canonical_value).collect::<HashSet<_>>().into_iter().collect();
    let subjects = book.publication.subjects.clone();
    // PDF rasterization is not metadata extraction. It has a much larger and
    // less predictable native working set, so covers are handled only by the
    // independently bounded cover pipeline.
    let _ = extract_cover;
    let thumbnail = None;
    let saved_metadata = Some(CatalogEpubMetadata { book, toc: Vec::new() });
    Ok(ExtractedCatalogData { metadata: ExtractedCatalogMetadata { title, authors, languages, description }, isbns, subjects, saved_metadata, thumbnail })
}

pub(crate) fn bounded_embedded_text(value: &str, max_bytes: usize, multiline: bool) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || value.chars().any(|character| character.is_control() && !(multiline && matches!(character, '\n' | '\r' | '\t'))) {
        return None;
    }
    Some(if multiline { value.to_owned() } else { value.split_whitespace().collect::<Vec<_>>().join(" ") })
}

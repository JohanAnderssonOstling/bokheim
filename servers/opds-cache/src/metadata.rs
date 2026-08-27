use super::{
    CatalogBookFormat, CatalogDescription, CatalogEpubMetadata, CatalogMetadataBackfillIndexedJob, CatalogMetadataBackfillIndexedUpdate, CatalogMetadataMatch, CatalogPublication, CatalogPublicationAction, CatalogPublicationPopularity,
    CatalogService, CatalogServiceError, CatalogSourceId, ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityRequest, EditionIdentityResponse, EditionIdentityStatus, HashMap, HashSet, Identifier, LookupStatus,
    MetadataEnrichmentRequest, OpdsAcquisitionKind, OpdsBookFormat, OpdsDirectAcquisition, PublicationSubject, RichWorkMetadataMatch, Scheme, Scope, EPUB_COVER_KEY_PREFIX, MAX_EPUB_METADATA_PUBLICATION_BYTES,
    MAX_MOBI_METADATA_PUBLICATION_BYTES, MAX_PDF_METADATA_PUBLICATION_BYTES,
};
use crate::extraction::ExtractedCatalogMetadata;
use catalog_model::{AgeRange, Audience, CatalogContributorCredit, CatalogPublisherCredit};
impl CatalogService {
    pub(crate) async fn enrich_saved_metadata_batch(&self, jobs: &[CatalogMetadataBackfillIndexedJob]) -> Result<Vec<CatalogMetadataBackfillIndexedUpdate>, CatalogServiceError> {
        let isbn_groups = jobs.iter().map(|job| job.index.isbns.clone()).collect::<Vec<_>>();
        let mut identity_results = vec![None; jobs.len()];

        for index in 0..jobs.len() {
            if isbn_groups[index].is_empty() && identity_results[index].is_none() {
                identity_results[index] =
                    Some(metadata_contract::EditionIdentityResult { query_id: index.to_string(), status: EditionIdentityStatus::InsufficientMetadata, matched: None, candidates: Vec::new(), candidates_truncated: false });
            }
        }

        let all_isbns = isbn_groups.iter().flatten().cloned().collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
        let (snapshot_date, matches_by_isbn) = self.metadata_matches_by_isbn(&all_isbns).await?;
        let mut updates = Vec::with_capacity(jobs.len());
        for ((job, isbns), identity_result) in jobs.iter().zip(isbn_groups).zip(identity_results) {
            let records = isbns.iter().filter_map(|isbn| matches_by_isbn.get(isbn)).flatten().collect::<Vec<_>>();
            let matched = catalog_metadata_match(records, &snapshot_date);
            let mut subjects = matched.subjects;
            subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
            subjects.dedup();
            let mut index = job.index.clone();
            if let Some(isbn) = identity_result.as_ref().and_then(|result| result.matched.as_ref()).map(|matched| matched.canonical_isbn13.as_str()) {
                if !index.isbns.iter().any(|candidate| candidate == isbn) {
                    index.isbns.push(isbn.to_owned());
                }
            }
            updates.push(CatalogMetadataBackfillIndexedUpdate {
                source_id: job.source_id.clone(),
                publication: job.publication.clone(),
                acquisition_url: job.acquisition_url.clone(),
                subjects,
                popularity: matched.popularity,
                index,
                identity_result,
            });
        }
        Ok(updates)
    }

    pub(crate) async fn metadata_matches_by_isbn(&self, isbns: &[String]) -> Result<(String, HashMap<String, Vec<RichWorkMetadataMatch>>), CatalogServiceError> {
        if isbns.is_empty() {
            return Ok(("unknown".to_owned(), HashMap::new()));
        }
        let Some(endpoint) = &self.state.metadata_endpoint else { return Ok(("unknown".to_owned(), HashMap::new())) };
        let mut by_isbn = HashMap::<String, Vec<RichWorkMetadataMatch>>::new();
        let mut snapshot_date = None;
        for batch in isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let mut endpoint = endpoint.clone();
            endpoint.set_path("/v2/classifications");
            let body = metadata_contract::encode_v2_request(&MetadataEnrichmentRequest { isbns: batch.to_vec() }).map_err(|error| CatalogServiceError(error.to_string()))?;
            let response = self
                .state
                .metadata_client
                .post(endpoint)
                .header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)
                .header(reqwest::header::ACCEPT, metadata_contract::MEDIA_TYPE_V2)
                .body(body)
                .send()
                .await
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            if !response.status().is_success() {
                return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
            }
            let bytes = response.bytes().await.map_err(|error| CatalogServiceError(error.to_string()))?;
            if bytes.len() > metadata_contract::MAX_RESPONSE_BYTES {
                return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
            }
            let response = metadata_contract::decode_v2_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))?;
            if snapshot_date.as_ref().is_some_and(|date| date != &response.snapshot.dump_date) {
                return Err(CatalogServiceError("metadata service changed snapshots during one catalogue enrichment request".to_owned()));
            }
            snapshot_date = Some(response.snapshot.dump_date);
            for result in response.results.into_iter().filter(|result| matches!(result.status, LookupStatus::Matched | LookupStatus::Ambiguous) && !result.matches.is_empty()) {
                let matches = result.matches;
                by_isbn.entry(result.requested_isbn).or_default().extend(matches.iter().cloned());
                if let Some(canonical) = result.canonical_isbn13 {
                    by_isbn.entry(canonical).or_default().extend(matches);
                }
            }
        }
        Ok((snapshot_date.unwrap_or_else(|| "unknown".to_owned()), by_isbn))
    }

    pub(crate) async fn infer_catalog_epub_isbn(&self, metadata: &mut CatalogEpubMetadata) -> Result<Option<String>, CatalogServiceError> {
        let authors = metadata.book.authors().map(|author| author.name().to_owned()).collect::<Vec<_>>();
        let publishers = metadata.book.publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect::<Vec<_>>();
        let publication_year = publication_model::unique_publication_year(metadata.book.publication.dates.iter().map(|date| date.value()));
        if metadata.book.title.trim().is_empty() {
            return Ok(None);
        }
        let response =
            self.resolve_edition_identities(EditionIdentityRequest { queries: vec![EditionIdentityQuery { query_id: "catalogue-epub".to_owned(), title: metadata.book.title.clone(), authors, publishers, publication_year }] }).await?;
        let Some(result) = response.results.into_iter().find(|result| result.query_id == "catalogue-epub") else { return Ok(None) };
        if result.status != EditionIdentityStatus::Matched {
            return Ok(None);
        }
        let Some(matched) = result.matched else { return Ok(None) };
        add_catalog_identity_isbn(metadata, &matched.canonical_isbn13)?;
        Ok(Some(matched.canonical_isbn13))
    }

    pub(crate) async fn metadata_matches(&self, isbns: &[String]) -> Result<HashMap<String, CatalogMetadataMatch>, CatalogServiceError> {
        let Some(endpoint) = &self.state.metadata_endpoint else { return Ok(HashMap::new()) };
        let mut matched_records = Vec::new();
        let mut enrichment_keys = isbns.iter().cloned().collect::<HashSet<_>>();
        let mut snapshot_date = None;
        for batch in isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let mut endpoint = endpoint.clone();
            endpoint.set_path("/v2/classifications");
            let body = metadata_contract::encode_v2_request(&MetadataEnrichmentRequest { isbns: batch.to_vec() }).map_err(|error| CatalogServiceError(error.to_string()))?;
            let mut response = self
                .state
                .metadata_client
                .post(endpoint.clone())
                .header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2)
                .header(reqwest::header::ACCEPT, metadata_contract::MEDIA_TYPE_V2)
                .body(body)
                .send()
                .await
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            if !response.status().is_success() {
                return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
            }
            if response.content_length().is_some_and(|length| length > metadata_contract::MAX_RESPONSE_BYTES as u64) {
                return Err(CatalogServiceError("metadata service response exceeded its declared size limit".to_owned()));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| CatalogServiceError(error.to_string()))? {
                if bytes.len().saturating_add(chunk.len()) > metadata_contract::MAX_RESPONSE_BYTES {
                    return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
                }
                bytes.extend_from_slice(&chunk);
            }
            let response = metadata_contract::decode_v2_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))?;
            if snapshot_date.as_ref().is_some_and(|date| date != &response.snapshot.dump_date) {
                return Err(CatalogServiceError("metadata service changed snapshots during one catalogue enrichment request".to_owned()));
            }
            snapshot_date = Some(response.snapshot.dump_date);
            for result in response.results.into_iter().filter(|result| matches!(result.status, LookupStatus::Matched | LookupStatus::Ambiguous) && !result.matches.is_empty()) {
                enrichment_keys.insert(result.requested_isbn);
                if let Some(canonical) = result.canonical_isbn13 {
                    enrichment_keys.insert(canonical);
                }
                matched_records.extend(result.matches);
            }
        }
        if matched_records.is_empty() {
            return Ok(HashMap::new());
        }
        let matched = catalog_metadata_match(matched_records.iter().collect(), &snapshot_date.unwrap_or_else(|| "unknown".to_owned()));
        Ok(enrichment_keys.into_iter().map(|isbn| (isbn, matched.clone())).collect())
    }

    pub async fn resolve_edition_identities(&self, request: EditionIdentityRequest) -> Result<EditionIdentityResponse, CatalogServiceError> {
        let Some(metadata_endpoint) = &self.state.metadata_endpoint else { return Err(CatalogServiceError("Open Library metadata enrichment is not configured".to_owned())) };
        let mut endpoint = metadata_endpoint.clone();
        endpoint.set_path("/v2/edition-identities");
        let body = metadata_contract::encode_v2_edition_identity_request(&request).map_err(|error| CatalogServiceError(error.to_string()))?;
        let response = self.state.metadata_client.post(endpoint).header(reqwest::header::CONTENT_TYPE, metadata_contract::MEDIA_TYPE_V2).body(body).send().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogServiceError(format!("metadata service returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > metadata_contract::MAX_RESPONSE_BYTES as u64) {
            return Err(CatalogServiceError("metadata service response exceeded its declared size limit".to_owned()));
        }
        let bytes = response.bytes().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        if bytes.len() > metadata_contract::MAX_RESPONSE_BYTES {
            return Err(CatalogServiceError("metadata service response exceeded its size limit".to_owned()));
        }
        metadata_contract::decode_v2_edition_identity_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| CatalogServiceError(error.to_string()))
    }
}

pub(crate) fn publication_cover_fallback(source_id: &CatalogSourceId, publication: &mut CatalogPublication) -> Option<(String, OpdsDirectAcquisition)> {
    let request = publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if extraction_book_format(request.book_format).is_some() => Some(request),
        _ => None,
    })?;
    let book_format = extraction_book_format(request.book_format)?;
    let digest = blake3::hash(format!("{}\n{}", source_id.as_str(), request.download_url).as_bytes()).to_hex();
    let key = publication.image_url.clone().unwrap_or_else(|| format!("{EPUB_COVER_KEY_PREFIX}{digest}"));
    publication.image_url = Some(key.clone());
    Some((key, OpdsDirectAcquisition { url: request.download_url.clone(), kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None }))
}

pub(crate) fn publication_metadata_acquisition(publication: &CatalogPublication) -> Option<OpdsDirectAcquisition> {
    let request = publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if extraction_book_format(request.book_format).is_some() => Some(request),
        _ => None,
    })?;
    let book_format = extraction_book_format(request.book_format)?;
    Some(OpdsDirectAcquisition {
        url: request.download_url.clone(),
        kind: OpdsAcquisitionKind::OpenAccess,
        book_format,
        suggested_file_name: format!("{}.{}", blake3::hash(request.download_url.as_bytes()).to_hex(), book_format.extension()),
        price: None,
    })
}

pub(crate) fn extraction_book_format(format: CatalogBookFormat) -> Option<OpdsBookFormat> {
    match format {
        CatalogBookFormat::Epub => Some(OpdsBookFormat::Epub),
        CatalogBookFormat::Pdf => Some(OpdsBookFormat::Pdf),
        CatalogBookFormat::Mobi => Some(OpdsBookFormat::Mobi),
        _ => None,
    }
}

pub(crate) fn metadata_publication_byte_limit(format: OpdsBookFormat) -> u64 {
    match format {
        OpdsBookFormat::Epub => MAX_EPUB_METADATA_PUBLICATION_BYTES,
        OpdsBookFormat::Mobi => MAX_MOBI_METADATA_PUBLICATION_BYTES,
        OpdsBookFormat::Pdf => MAX_PDF_METADATA_PUBLICATION_BYTES,
        _ => 0,
    }
}

pub(crate) fn publication_has_acquisition_url(publication: &CatalogPublication, acquisition_url: &str) -> bool {
    publication.actions.iter().any(|action| matches!(action, CatalogPublicationAction::Import { request } if extraction_book_format(request.book_format).is_some() && request.download_url == acquisition_url))
}

pub(crate) fn apply_catalog_metadata(publication: &mut CatalogPublication, metadata: ExtractedCatalogMetadata) {
    if let Some(title) = metadata.title {
        publication.title = title;
    }
    if !metadata.authors.is_empty() {
        publication.authors = metadata.authors;
    }
    if !metadata.languages.is_empty() {
        publication.languages = metadata.languages;
    }
    if publication.description.is_none() {
        if let Some(text) = metadata.description {
            publication.description = Some(CatalogDescription::PlainText { text });
        }
    }
}

pub(crate) fn merge_catalog_epub_metadata(publication: &mut CatalogPublication, metadata: &CatalogEpubMetadata) {
    let book = &metadata.book;
    let rich = &book.publication;

    for date in &rich.dates {
        push_unique(&mut publication.metadata.dates, date.clone());
    }
    if publication.metadata.published.is_none() {
        publication.metadata.published =
            rich.dates.iter().find(|date| date.event().is_some_and(|event| matches!(metadata_token(event).as_str(), "publication" | "published"))).or_else(|| rich.dates.first()).map(|date| date.value().to_owned());
    }
    if publication.metadata.modified.is_none() {
        publication.metadata.modified = rich.dates.iter().find(|date| date.event().is_some_and(|event| matches!(metadata_token(event).as_str(), "modification" | "modified"))).map(|date| date.value().to_owned());
    }
    for publisher in &rich.publishers {
        push_unique(
            &mut publication.metadata.publishers,
            CatalogPublisherCredit { name: publisher.name().as_str().to_owned(), name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None },
        );
    }
    for identifier in &rich.identifiers {
        push_unique(&mut publication.metadata.identifiers, identifier.clone());
    }
    for title in &rich.alternate_titles {
        let candidate = title.clone();
        if publication.metadata.subtitle.is_none() && candidate.title_type.as_deref().is_some_and(|kind| metadata_token(kind) == "subtitle") {
            publication.metadata.subtitle = Some(candidate.value.clone());
        }
        push_unique(&mut publication.metadata.alternate_titles, candidate);
    }
    for collection in &rich.collections {
        push_unique(&mut publication.metadata.collections, collection.clone());
    }
    for contributor in &book.contributors {
        if contributor.role_code().0 == *b"aut" {
            continue;
        }
        let role = String::from_utf8_lossy(&contributor.role_code().0).into_owned();
        push_unique(
            &mut publication.metadata.contributors,
            CatalogContributorCredit {
                name: contributor.name().to_owned(),
                name_localizations: Vec::new(),
                sort_name: contributor.sort_name().map(str::to_owned),
                sort_name_localizations: Vec::new(),
                identifiers: Vec::new(),
                provider_link: None,
                roles: vec![role],
            },
        );
    }
    append_unique_strings(&mut publication.metadata.rights, &rich.rights);
    append_unique_strings(&mut publication.metadata.sources, &rich.sources);
    append_unique_strings(&mut publication.metadata.types, &rich.types);
    append_unique_strings(&mut publication.metadata.formats, &rich.formats);
    append_unique_strings(&mut publication.metadata.relations, &rich.relations);
    append_unique_strings(&mut publication.metadata.coverage, &rich.coverage);
    for property in &rich.properties {
        let candidate = property.clone();
        let token = metadata_token(property.property());
        if matches!(token.as_str(), "audience" | "audiencetype") {
            push_unique(&mut publication.metadata.audiences, Audience { value: property.value().to_owned(), source: property.source().to_owned() });
        }
        if matches!(token.as_str(), "typicalagerange" | "agerange") {
            push_unique(&mut publication.metadata.age_ranges, AgeRange { value: property.value().to_owned(), source: property.source().to_owned() });
        }
        push_unique(&mut publication.metadata.properties, candidate);
    }
}

fn metadata_token(value: &str) -> String {
    value.rsplit([':', '#', '/']).next().unwrap_or(value).chars().filter(|character| character.is_ascii_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn push_unique<T: PartialEq>(target: &mut Vec<T>, candidate: T) {
    if !target.contains(&candidate) {
        target.push(candidate);
    }
}

fn append_unique_strings(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

pub(crate) fn apply_catalog_description_if_missing(publication: &mut CatalogPublication, description: Option<String>) {
    if publication.description.is_none() {
        if let Some(text) = description {
            publication.description = Some(CatalogDescription::PlainText { text });
        }
    }
}

pub(crate) fn merge_catalog_popularity(current: &mut Option<CatalogPublicationPopularity>, candidate: &CatalogPublicationPopularity) {
    let candidate_key = (candidate.reading_log_count(), candidate.rating_count);
    if current.as_ref().is_none_or(|current| candidate_key > (current.reading_log_count(), current.rating_count)) {
        *current = Some(candidate.clone());
    }
}

pub(crate) fn catalog_metadata_match(records: Vec<&RichWorkMetadataMatch>, snapshot_date: &str) -> CatalogMetadataMatch {
    // The metadata service already applies the library client's unified
    // classification-consensus policy before serializing every match. Treat
    // those classifications as authoritative here instead of constructing a
    // second, several-hundred-megabyte taxonomy runtime in the catalogue
    // process merely to filter the same evidence again.
    let merged = metadata_contract::merge_rich_work_matches(records, |_, _| Vec::new());
    let subjects = merged
        .classifications
        .iter()
        .filter_map(|classification| {
            let authority = match classification.scheme {
                ClassificationScheme::LibraryOfCongress => "lcc",
                ClassificationScheme::DeweyDecimal => "ddc",
                ClassificationScheme::Bisac => "bisac",
            };
            let provenance = match classification.source {
                ClassificationSource::ExactEdition => "exact-edition",
                ClassificationSource::Work => "work",
            };
            let source = format!("openlibrary:{snapshot_date}:{provenance}");
            PublicationSubject::new(None, &classification.notation, source, Some(authority.to_owned()), Some(classification.notation.clone())).ok()
        })
        .collect();
    let popularity = merged.popularity.map(|popularity| CatalogPublicationPopularity {
        want_to_read_count: popularity.want_to_read_count,
        currently_reading_count: popularity.currently_reading_count,
        already_read_count: popularity.already_read_count,
        rating_count: popularity.rating_count,
        rating_sum: popularity.rating_sum,
    });
    CatalogMetadataMatch { subjects, popularity, description: merged.description }
}

pub(crate) fn add_catalog_identity_isbn(metadata: &mut CatalogEpubMetadata, canonical_isbn13: &str) -> Result<(), CatalogServiceError> {
    let identifier = Identifier::new(canonical_isbn13, Scheme::Isbn, Scope::Edition, "openlibrary:edition-match").map_err(|error| CatalogServiceError(error.to_string()))?;
    metadata.book.publication.identifiers.retain(|existing| existing.scheme() != &Scheme::Isbn);
    metadata.book.publication.identifiers.push(identifier);
    Ok(())
}

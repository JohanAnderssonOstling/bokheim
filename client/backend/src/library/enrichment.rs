use crate::app::BackendError;
use crate::library::database as db;
use crate::library::LibraryBackend;
use crate::ContentHash;
use library_model::PublicationSubject;
use library_model::{AuthorAuthority, AuthorName, ExternalAuthorId};
use library_model::{EditionIdentityReview, EditionIdentityReviewCandidate, EditionIdentityReviewStatus};
use metadata_contract::{
    Classification, ClassificationScheme, ClassificationSource, EditionClassificationConsensus, EditionIdentityQuery, EditionIdentityRequest, EditionIdentityStatus, EntitySource, LookupStatus, MetadataEnrichmentRequest, PersonEntity,
    PlaceEntity, RichWorkMetadataMatch,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const OPEN_LIBRARY_AUTHOR_PROVIDER: &str = "openlibrary_authors_v2";
// v16 applies one final edition vote across every inferred ISBN/work match so
// a singleton exact-edition value cannot re-enter through a small work.
const OPEN_LIBRARY_RICH_PROVIDER: &str = "openlibrary_rich_v16";
const OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER: &str = "openlibrary_edition_identity_v4";
const LIBRARYTHING_THINGISBN_PROVIDER: &str = "librarything_thingisbn_v1";
const MAX_LIBRARYTHING_SOURCE_ISBNS_PER_BOOK: usize = 8;
const MAX_LIBRARYTHING_SIBLING_ISBNS_PER_BOOK: usize = 128;
// Edition matching can involve large candidate intersections in the dump.
// Smaller requests keep one pathological classic from timing out an entire
// 128-book retry batch and leave less work to repeat after interruption.
const EDITION_IDENTITY_BATCH_SIZE: usize = 8;
const MAX_DIRECT_CLASSIFICATIONS_WITHOUT_CONSENSUS: usize = 12;

#[derive(Default)]
struct LibraryThingWorkEvidence {
    popularity: metadata_contract::WorkPopularity,
    isbn_hits: usize,
    classifications: BTreeSet<(ClassificationScheme, String)>,
    exact_classifications_by_edition: BTreeMap<String, BTreeSet<(ClassificationScheme, String)>>,
}

fn edition_identity_review_candidate(candidate: metadata_contract::EditionIdentityMatch) -> EditionIdentityReviewCandidate {
    let classifications = candidate
        .classifications
        .iter()
        .flat_map(|classification| match classification.scheme {
            ClassificationScheme::LibraryOfCongress => subject_projection::unified_lcc_subject_paths(&classification.notation),
            ClassificationScheme::DeweyDecimal => subject_projection::unified_ddc_subject_paths(&classification.notation),
            ClassificationScheme::Bisac => subject_projection::unified_subject_paths(subject_projection::BISAC_SYSTEM_ID, &classification.notation),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    EditionIdentityReviewCandidate {
        isbn13: candidate.canonical_isbn13,
        edition_id: candidate.open_library_edition_id,
        work_id: candidate.open_library_work_id,
        title: candidate.title,
        authors: candidate.authors,
        publishers: candidate.publishers,
        publication_year: candidate.publication_year,
        matched_publisher: candidate.matched_publisher,
        matched_publication_year: candidate.matched_publication_year,
        classifications,
    }
}

fn candidate_classification_rank(candidate: &EditionIdentityReviewCandidate) -> (usize, usize, usize) {
    let depths = candidate.classifications.iter().map(|path| path.split('/').filter(|part| !part.trim().is_empty()).count()).collect::<Vec<_>>();
    (depths.len(), depths.iter().sum(), depths.into_iter().max().unwrap_or(0))
}

impl LibraryBackend {
    pub async fn prepared_edition_identity_reviews(&self) -> Result<Vec<EditionIdentityReview>, BackendError> {
        self.refresh_edition_identity_candidate_classifications().await?;
        self.edition_identity_reviews()
    }

    fn edition_identity_reviews(&self) -> Result<Vec<EditionIdentityReview>, BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        db::edition_identity_reviews(&connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER)
            .map(|reviews| {
                reviews
                    .into_iter()
                    .filter_map(|review| {
                        let status = match review.status.as_str() {
                            "ambiguous" => EditionIdentityReviewStatus::Ambiguous,
                            "no_match" => EditionIdentityReviewStatus::NoMatch,
                            "failed" => EditionIdentityReviewStatus::Failed,
                            _ => return None,
                        };
                        let mut candidates = review.candidates.into_iter().map(edition_identity_review_candidate).collect::<Vec<_>>();
                        candidates.sort_by(|left, right| candidate_classification_rank(right).cmp(&candidate_classification_rank(left)));
                        let evidence = book_metadata::edition_identity_evidence(&review.title);
                        let inferred_title = (evidence.title.trim() != review.title.trim()).then_some(evidence.title);
                        let inferred_author = evidence.author.filter(|author| !review.authors.iter().any(|existing| existing.eq_ignore_ascii_case(author)));
                        let inferred_publication_year = review.publication_year.is_none().then_some(evidence.publication_year).flatten();
                        Some(EditionIdentityReview {
                            content_hash: review.content_hash,
                            title: review.title,
                            authors: review.authors,
                            publishers: review.publishers,
                            publication_year: review.publication_year,
                            inferred_title,
                            inferred_author,
                            inferred_publication_year,
                            status,
                            detail: review.detail,
                            candidates,
                            candidates_truncated: review.candidates_truncated,
                        })
                    })
                    .collect()
            })
            .map_err(BackendError::operation)
    }

    /// Fills classification evidence omitted by older edition-identity
    /// servers by looking up the stored candidate ISBNs through the rich batch
    /// endpoint. The Open Library work id keeps classifications from a
    /// different work with the same ISBN out of the review candidate.
    async fn refresh_edition_identity_candidate_classifications(&self) -> Result<usize, BackendError> {
        let pending = {
            let connection = self.db.get().map_err(BackendError::operation)?;
            db::edition_identity_reviews(&connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER)
                .map_err(BackendError::operation)?
                .into_iter()
                .filter(|review| review.status == "ambiguous")
                .flat_map(|review| review.candidates.into_iter().filter(|candidate| candidate.classifications.is_empty()).map(move |candidate| (review.content_hash.clone(), candidate)))
                .collect::<Vec<_>>()
        };
        if pending.is_empty() {
            return Ok(0);
        }

        let mut isbns = pending.iter().map(|(_, candidate)| candidate.canonical_isbn13.clone()).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        isbns.sort();
        let mut matches_by_isbn = HashMap::<String, Vec<RichWorkMetadataMatch>>::new();
        for batch in isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let response = self.app()?.rich_metadata_enrichment(&MetadataEnrichmentRequest { isbns: batch.to_vec() }).await?;
            for result in response.results {
                let requested = library_model::canonical_isbn(&result.requested_isbn).unwrap_or(result.requested_isbn);
                matches_by_isbn.insert(requested, result.matches);
            }
        }

        let mut hydrated = Vec::new();
        for (content_hash, mut candidate) in pending {
            let Some(matches) = matches_by_isbn.get(&candidate.canonical_isbn13) else { continue };
            let matched = candidate
                .open_library_work_id
                .as_deref()
                .and_then(|work_id| matches.iter().find(|matched| matched.open_library_work_id.as_deref() == Some(work_id)))
                .or_else(|| matches.iter().find(|matched| matched.exact_edition_ids.iter().any(|edition_id| edition_id == &candidate.open_library_edition_id)))
                .or_else(|| (matches.len() == 1).then(|| &matches[0]));
            let Some(matched) = matched else { continue };
            if matched.classifications.is_empty() {
                continue;
            }
            candidate.classifications = matched.classifications.clone();
            hydrated.push((content_hash, candidate));
        }

        let mut connection = self.db.get().map_err(BackendError::operation)?;
        db::update_edition_identity_candidate_classifications(&mut connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &hydrated).map_err(BackendError::operation)
    }

    pub fn select_edition_identity_candidate(&self, content_hash: ContentHash, edition_id: &str, isbn13: &str) -> Result<bool, BackendError> {
        let mut connection = self.db.get().map_err(BackendError::operation)?;
        db::apply_reviewed_edition_identity(&mut connection, content_hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, edition_id, isbn13).map_err(BackendError::operation)
    }

    pub fn apply_manual_edition_identity(&self, content_hash: ContentHash, isbn: &str) -> Result<bool, BackendError> {
        let mut connection = self.db.get().map_err(BackendError::operation)?;
        db::apply_manual_edition_identity(&mut connection, content_hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, isbn).map_err(BackendError::operation)
    }

    pub fn reject_edition_identity_candidates(&self, content_hash: ContentHash) -> Result<(), BackendError> {
        let mut connection = self.db.get().map_err(BackendError::operation)?;
        db::reject_edition_identity_candidates(&mut connection, content_hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER).map_err(BackendError::operation)
    }

    pub async fn retry_edition_identity(&self, content_hash: ContentHash) -> Result<(), BackendError> {
        let candidate = {
            let connection = self.db.get().map_err(BackendError::operation)?;
            db::edition_identity_candidate(&connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &content_hash).map_err(BackendError::operation)?
        };
        let candidate = candidate.ok_or_else(|| BackendError::operation(std::io::Error::new(std::io::ErrorKind::NotFound, "book is no longer eligible for ISBN inference")))?;
        self.resolve_open_library_edition_identities(vec![candidate]).await?;
        Ok(())
    }

    pub async fn retry_failed_edition_identities(&self) -> Result<usize, BackendError> {
        let candidates = {
            let connection = self.db.get().map_err(BackendError::operation)?;
            let failed = db::edition_identity_reviews(&connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER)
                .map_err(BackendError::operation)?
                .into_iter()
                .filter(|review| review.status == "failed")
                .map(|review| review.content_hash)
                .collect::<HashSet<_>>();
            db::edition_identity_candidates(&connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, usize::MAX, true)
                .map_err(BackendError::operation)?
                .into_iter()
                .filter(|candidate| failed.contains(&candidate.content_hash))
                .collect::<Vec<_>>()
        };
        self.resolve_open_library_edition_identities(candidates).await
    }

    /// Adds an ISBN only when dump evidence identifies one exact edition. An
    /// embedded/scanned ISBN always wins because books that already have one
    /// are excluded both while selecting and while applying candidates.
    pub(crate) async fn infer_open_library_isbns(&self, limit: usize, retry_failed: bool) -> Result<usize, BackendError> {
        let candidates = {
            let connection = self.db.get().map_err(BackendError::operation)?;
            db::edition_identity_candidates(&connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, limit, retry_failed).map_err(BackendError::operation)?
        };
        self.resolve_open_library_edition_identities(candidates).await
    }

    async fn resolve_open_library_edition_identities(&self, candidates: Vec<db::EditionIdentityCandidate>) -> Result<usize, BackendError> {
        let mut updated = 0;
        let mut unresolved = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if let Some(isbn) = book_metadata::isbn_from_filename_like_title(&candidate.title) {
                let mut connection = self.db.get().map_err(BackendError::operation)?;
                updated += usize::from(db::apply_filename_isbn(&mut connection, candidate.content_hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &isbn).map_err(BackendError::operation)?);
            } else {
                unresolved.push(candidate);
            }
        }
        for batch in unresolved.chunks(EDITION_IDENTITY_BATCH_SIZE.min(metadata_contract::MAX_EDITION_QUERIES_PER_REQUEST)) {
            let queries = batch
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    let evidence = book_metadata::edition_identity_evidence(&candidate.title);
                    let mut authors = candidate.authors.clone();
                    if let Some(author) = evidence.author {
                        if !authors.iter().any(|existing| existing.eq_ignore_ascii_case(&author)) {
                            authors.push(author);
                        }
                    }
                    EditionIdentityQuery { query_id: index.to_string(), title: evidence.title, authors, publishers: candidate.publishers.clone(), publication_year: candidate.publication_year.or(evidence.publication_year) }
                })
                .collect();
            let response = match self.app()?.resolve_edition_identities(&EditionIdentityRequest { queries }).await {
                Ok(response) => response,
                Err(error) => {
                    let failures = batch.iter().map(|candidate| (candidate.content_hash.clone(), candidate.title.clone())).collect::<Vec<_>>();
                    let mut connection = self.db.get().map_err(BackendError::operation)?;
                    db::record_external_metadata_failures(&mut connection, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &failures, &error.to_string()).map_err(BackendError::operation)?;
                    log::warn!("Open Library edition matching skipped a batch of {} books after an error: {error}", batch.len());
                    continue;
                }
            };
            let results = response.results.into_iter().map(|result| (result.query_id.clone(), result)).collect::<HashMap<_, _>>();
            for (index, candidate) in batch.iter().enumerate() {
                let result = results.get(&index.to_string());
                let (status, isbn, identifier, detail) = match result {
                    None => ("failed", None, index.to_string(), Some("metadata response omitted the edition query".to_owned())),
                    Some(result) if result.status == EditionIdentityStatus::Matched => match &result.matched {
                        Some(matched) => {
                            let discriminator = match (matched.matched_publisher, matched.matched_publication_year) {
                                (true, true) => "; publisher/year tie-break",
                                (true, false) => "; publisher tie-break",
                                (false, true) => "; year tie-break",
                                (false, false) => "",
                            };
                            ("updated", Some(matched.canonical_isbn13.as_str()), matched.open_library_edition_id.clone(), Some(format!("snapshot {}; title/author match{discriminator}", response.snapshot.dump_date)))
                        }
                        None => ("failed", None, index.to_string(), Some("matched response omitted edition evidence".to_owned())),
                    },
                    Some(result) if result.status == EditionIdentityStatus::Ambiguous => ("ambiguous", None, index.to_string(), result.candidates_truncated.then(|| "candidate list was truncated".to_owned())),
                    Some(result) if result.status == EditionIdentityStatus::NoMatch || result.status == EditionIdentityStatus::InsufficientMetadata => ("no_match", None, index.to_string(), None),
                    Some(_) => ("failed", None, index.to_string(), Some("unsupported edition identity result".to_owned())),
                };
                let review_candidates = result.map(|result| result.candidates.as_slice()).unwrap_or_default();
                let mut connection = self.db.get().map_err(BackendError::operation)?;
                // Ambiguous matches are review evidence, not publication
                // identity. Keep every candidate for the review UI but never
                // attach all of their ISBNs to the local book automatically.
                let applied =
                    db::apply_edition_identity(&mut connection, candidate.content_hash.clone(), OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &identifier, isbn, status, detail.as_deref(), review_candidates).map_err(BackendError::operation)?;
                updated += usize::from(applied);
            }
        }
        Ok(updated)
    }

    /// Batch-enriches ISBN books with classifications, subject places, and
    /// subject people. Canonical subject references are synchronized with the
    /// book; resolved entity labels and the place graph remain a rebuildable
    /// local cache for navigation.
    pub(crate) async fn enrich_rich_metadata(&self, limit: usize, retry_failed: bool) -> Result<usize, BackendError> {
        let candidates = {
            let connection = self.db.get().map_err(BackendError::operation)?;
            db::rich_enrichment_candidates(&connection, OPEN_LIBRARY_RICH_PROVIDER, limit, retry_failed).map_err(BackendError::operation)?
        };
        let mut requested_isbns = candidates.iter().flat_map(|candidate| candidate.isbns.iter().cloned()).collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
        requested_isbns.sort();
        let mut results = HashMap::new();
        let mut place_index = HashMap::new();
        let mut person_index = HashMap::new();
        for isbns in requested_isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let response = match self.app()?.rich_metadata_enrichment(&MetadataEnrichmentRequest { isbns: isbns.to_vec() }).await {
                Ok(response) => response,
                Err(error) => {
                    let failures = candidates.iter().map(|candidate| (candidate.content_hash.clone(), candidate.isbns.join(","))).collect::<Vec<_>>();
                    let mut connection = self.db.get().map_err(BackendError::operation)?;
                    db::record_external_metadata_failures(&mut connection, OPEN_LIBRARY_RICH_PROVIDER, &failures, &error.to_string()).map_err(BackendError::operation)?;
                    return Err(error);
                }
            };
            place_index.extend(response.places.into_iter().map(|entity| (entity.wikidata_id.clone(), entity)));
            person_index.extend(response.people.into_iter().map(|entity| (entity.wikidata_id.clone(), entity)));
            for result in response.results {
                let requested = library_model::canonical_isbn(&result.requested_isbn).unwrap_or_else(|| result.requested_isbn.clone());
                if let Some(isbn13) = result.canonical_isbn13.as_ref().filter(|isbn13| isbn13.as_str() != requested.as_str()) {
                    results.insert(isbn13.clone(), result.clone());
                }
                results.insert(requested, result);
            }
        }
        let mut updated = 0_usize;
        let mut taxonomy_candidates = Vec::new();
        for candidate in candidates {
            let mut matched_records = Vec::new();
            let mut alternative_classification_consensus = EditionClassificationConsensus::default();
            let mut matched_isbn_count = 0_usize;
            let mut omitted = false;
            for isbn in &candidate.isbns {
                match results.get(isbn) {
                    None => omitted = true,
                    Some(result) if !result.matches.is_empty() && (result.status == LookupStatus::Matched || result.status == LookupStatus::Ambiguous) => {
                        matched_isbn_count += 1;
                        let mut matches = result.matches.clone();
                        if !candidate.exact_isbns.contains(isbn) {
                            alternative_classification_consensus.record_alternative_matches(&mut matches, isbn);
                        }
                        matched_records.extend(matches);
                    }
                    Some(_) => {}
                }
            }
            let global_alternative_classifications = alternative_classification_consensus.classifications();
            let original_has_classifications = !global_alternative_classifications.is_empty() || matched_records.iter().any(|matched| !matched.classifications.is_empty());
            let original_work_ids = matched_records.iter().filter_map(|matched| matched.open_library_work_id.clone()).collect::<BTreeSet<_>>();
            let librarything_attempted = self.app()?.has_librarything() && !original_has_classifications;
            let librarything_classifications = if librarything_attempted { self.librarything_classification_fallback(&candidate, &original_work_ids).await? } else { Vec::new() };
            let use_librarything_consensus = librarything_attempted && !librarything_classifications.is_empty();
            let mut replace_previous_work_classifications = use_librarything_consensus;
            let identifier = candidate.isbns.join(",");
            let (status, matched, places, people, subjects, detail) = if matched_records.is_empty() && librarything_classifications.is_empty() {
                if omitted {
                    ("failed", None, Vec::new(), Vec::new(), Vec::new(), Some("metadata response omitted one or more requested ISBNs".to_owned()))
                } else {
                    ("no_match", None, Vec::new(), Vec::new(), Vec::new(), Some(format!("checked {} ISBNs", candidate.isbns.len())))
                }
            } else {
                let work_count = matched_records.iter().filter_map(|matched| matched.open_library_work_id.as_deref()).collect::<BTreeSet<_>>().len();
                let mut matched = if matched_records.is_empty() {
                    RichWorkMetadataMatch {
                        open_library_work_id: None,
                        exact_edition_ids: Vec::new(),
                        classifications: Vec::new(),
                        authors: Vec::new(),
                        places: Vec::new(),
                        people: Vec::new(),
                        popularity: None,
                        description: None,
                        subjects: Vec::new(),
                    }
                } else {
                    merge_rich_matches(matched_records.iter().collect())
                };
                matched.classifications.extend(global_alternative_classifications);
                matched.classifications.sort();
                matched.classifications.dedup();
                let merged_direct_classification_count = matched.classifications.len();
                let direct_consensus_replacement = original_has_classifications && candidate.classification_count > MAX_DIRECT_CLASSIFICATIONS_WITHOUT_CONSENSUS && merged_direct_classification_count < candidate.classification_count;
                replace_previous_work_classifications |= direct_consensus_replacement;
                let librarything_classification_count = librarything_classifications.len();
                if use_librarything_consensus {
                    // The direct response repeats the complete work/sibling
                    // union. Preserve classifications on the user's exact
                    // edition, but replace inferred work values with the
                    // independently sampled edition consensus.
                    matched.classifications.retain(|classification| classification.source == ClassificationSource::ExactEdition);
                }
                matched.classifications.extend(librarything_classifications);
                matched.classifications.sort();
                matched.classifications.dedup();
                let (places, people) = entities_for_match(&matched, &place_index, &person_index);
                let subjects = rich_subjects(&matched)?;
                let fallback_detail = if direct_consensus_replacement {
                    format!("; replaced the previous oversized work union with {merged_direct_classification_count} dump-native edition-consensus classifications")
                } else if librarything_classification_count == 0 {
                    String::new()
                } else if original_has_classifications {
                    format!("; replaced the work-wide classification union with {librarything_classification_count} classifications from LibraryThing-assisted edition consensus")
                } else {
                    format!("; added {librarything_classification_count} classifications through LibraryThing-assisted edition consensus")
                };
                let detail = Some(format!("checked {} ISBNs; {matched_isbn_count} matched across {work_count} Open Library work records; merged all matches{fallback_detail}", candidate.isbns.len()));
                ("updated", Some(matched), places, people, subjects, detail)
            };
            let mut connection = self.db.get().map_err(BackendError::operation)?;
            db::apply_rich_enrichment(&mut connection, candidate.content_hash.clone(), OPEN_LIBRARY_RICH_PROVIDER, &identifier, matched.as_ref(), &places, &people, subjects, status, detail.as_deref(), replace_previous_work_classifications)
                .map_err(BackendError::operation)?;
            if status == "updated" {
                updated += 1;
                taxonomy_candidates.push(candidate.content_hash);
            }
        }
        if let Err(error) = self.app()?.hydrate_taxonomy_for_books(self, &taxonomy_candidates).await {
            log::warn!("rich metadata was applied, but missing taxonomy codes could not be hydrated: {error}");
        }
        Ok(updated)
    }

    async fn librarything_classification_fallback(&self, candidate: &db::RichEnrichmentCandidate, original_work_ids: &BTreeSet<String>) -> Result<Vec<Classification>, BackendError> {
        let app = self.app()?;
        if !app.has_librarything() {
            return Ok(Vec::new());
        }

        let mut sibling_isbns = BTreeSet::new();
        let mut request_errors = Vec::new();
        let source_isbns = candidate.exact_isbns.iter().chain(candidate.isbns.iter().filter(|isbn| !candidate.exact_isbns.contains(*isbn))).take(MAX_LIBRARYTHING_SOURCE_ISBNS_PER_BOOK).collect::<Vec<_>>();
        for isbn in source_isbns.iter().copied() {
            match app.librarything_sibling_isbns(isbn).await {
                Ok(isbns) => sibling_isbns.extend(isbns),
                Err(error) => request_errors.push(error.to_string()),
            }
        }
        for isbn in source_isbns.iter().copied() {
            sibling_isbns.remove(isbn);
        }
        let discovered_count = sibling_isbns.len();
        let sibling_isbns = sibling_isbns.into_iter().take(MAX_LIBRARYTHING_SIBLING_ISBNS_PER_BOOK).collect::<Vec<_>>();

        let mut work_evidence = BTreeMap::<String, LibraryThingWorkEvidence>::new();
        let mut metadata_errors = Vec::new();
        for batch in sibling_isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            match app.rich_metadata_enrichment(&MetadataEnrichmentRequest { isbns: batch.to_vec() }).await {
                Ok(response) => {
                    for result in response.results {
                        for matched in result.matches {
                            let work_key = matched.open_library_work_id.clone().unwrap_or_else(|| format!("edition:{}", matched.exact_edition_ids.join(",")));
                            let evidence = work_evidence.entry(work_key).or_default();
                            evidence.isbn_hits = evidence.isbn_hits.saturating_add(1);
                            if matched.popularity.as_ref().is_some_and(|popularity| popularity.evidence_rank() > evidence.popularity.evidence_rank()) {
                                evidence.popularity = matched.popularity.clone().unwrap_or_default();
                            }
                            let edition_key = if matched.exact_edition_ids.is_empty() { format!("isbn:{}", result.requested_isbn) } else { matched.exact_edition_ids.join(",") };
                            for classification in matched.classifications {
                                let key = (classification.scheme, classification.notation);
                                if classification.source == ClassificationSource::ExactEdition {
                                    evidence.exact_classifications_by_edition.entry(edition_key.clone()).or_default().insert(key.clone());
                                }
                                evidence.classifications.insert(key);
                            }
                        }
                    }
                }
                Err(error) => metadata_errors.push(error.to_string()),
            }
        }
        let work_count = work_evidence.len();
        let eligible = if original_work_ids.is_empty() {
            work_evidence
                .into_iter()
                .max_by(|(left_key, left), (right_key, right)| {
                    (left.popularity.evidence_rank(), left.isbn_hits, std::cmp::Reverse(left.classifications.len()), left_key).cmp(&(
                        right.popularity.evidence_rank(),
                        right.isbn_hits,
                        std::cmp::Reverse(right.classifications.len()),
                        right_key,
                    ))
                })
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            work_evidence.into_iter().filter(|(work_id, _)| original_work_ids.contains(work_id)).collect::<Vec<_>>()
        };
        let eligible_work_count = eligible.len();
        let distinct_classification_count = eligible.iter().flat_map(|(_, evidence)| evidence.classifications.iter()).collect::<BTreeSet<_>>().len();
        // LibraryThing clusters can be broad, while every metadata response
        // repeats the complete sibling-edition union for its work. Count only
        // classifications attached to independently queried exact editions.
        let mut classifications = eligible.iter().flat_map(|(_, evidence)| metadata_contract::exact_edition_classification_consensus(evidence.exact_classifications_by_edition.values().map(BTreeSet::iter))).collect::<Vec<_>>();
        classifications.sort();
        classifications.dedup();
        let truncated = discovered_count.saturating_sub(sibling_isbns.len());
        let mut details = vec![format!(
            "queried {} source ISBNs; LibraryThing returned {discovered_count} distinct additional ISBNs; checked {}; considered {work_count} Open Library works; used {eligible_work_count} works linked by the book's ISBNs; retained {} of {distinct_classification_count} normalized classifications supported by at least two independent editions per work and system",
            source_isbns.len(),
            sibling_isbns.len(),
            classifications.len(),
        )];
        if truncated > 0 {
            details.push(format!("{truncated} sibling ISBNs were skipped by the per-book safety limit"));
        }
        if candidate.isbns.len() > source_isbns.len() {
            details.push(format!("{} source ISBNs were skipped by the per-book safety limit", candidate.isbns.len() - source_isbns.len()));
        }
        if !request_errors.is_empty() {
            details.push(format!("{} LibraryThing requests failed: {}", request_errors.len(), request_errors.join("; ")));
        }
        if !metadata_errors.is_empty() {
            details.push(format!("{} sibling metadata batches failed: {}", metadata_errors.len(), metadata_errors.join("; ")));
        }
        let status = if !classifications.is_empty() {
            "updated"
        } else if !request_errors.is_empty() || !metadata_errors.is_empty() {
            "failed"
        } else {
            "no_match"
        };
        let identifier = candidate.isbns.join(",");
        let detail = details.join("; ");
        let connection = self.db.get().map_err(BackendError::operation)?;
        db::record_external_metadata_attempt(&connection, &candidate.content_hash, LIBRARYTHING_THINGISBN_PROVIDER, &identifier, status, Some(&detail)).map_err(BackendError::operation)?;
        Ok(classifications)
    }

    /// Resolves scanned author credits through the public catalogue metadata
    /// endpoint. Requests contain ISBNs only; library ids, titles, paths, and
    /// credited names remain on the device.
    pub(crate) async fn enrich_open_library_authors(&self, limit: usize, retry_failed: bool) -> Result<usize, BackendError> {
        let candidates = {
            let connection = self.db.get().map_err(BackendError::operation)?;
            db::author_enrichment_candidates(&connection, OPEN_LIBRARY_AUTHOR_PROVIDER, limit, retry_failed).map_err(BackendError::operation)?
        };
        let mut updated = 0_usize;
        for batch in candidates.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let isbns = batch.iter().map(|candidate| candidate.isbn.clone()).collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
            let response = match self.app()?.metadata_enrichment(&MetadataEnrichmentRequest { isbns }).await {
                Ok(response) => response,
                Err(error) => {
                    let failures = batch.iter().map(|candidate| (candidate.content_hash.clone(), candidate.isbn.clone())).collect::<Vec<_>>();
                    let mut connection = self.db.get().map_err(BackendError::operation)?;
                    db::record_external_metadata_failures(&mut connection, OPEN_LIBRARY_AUTHOR_PROVIDER, &failures, &error.to_string()).map_err(BackendError::operation)?;
                    return Err(error);
                }
            };
            let results = response.results.into_iter().map(|result| (result.requested_isbn.clone(), result)).collect::<HashMap<_, _>>();
            for candidate in batch {
                let result = results.get(&candidate.isbn).or_else(|| {
                    let canonical = library_model::canonical_isbn(&candidate.isbn)?;
                    results.values().find(|result| result.canonical_isbn13.as_deref() == Some(canonical.as_str()))
                });
                let (status, resolutions, detail) = match result {
                    None => ("failed", Vec::new(), Some("metadata response omitted the requested ISBN".to_owned())),
                    Some(result) if result.status == LookupStatus::NoMatch || result.status == LookupStatus::InvalidIsbn => ("no_match", Vec::new(), None),
                    Some(result) if result.matches.is_empty() => ("no_match", Vec::new(), Some("ISBN returned no Open Library work metadata".to_owned())),
                    Some(result) => {
                        let work_count = result.matches.iter().filter_map(|matched| matched.open_library_work_id.as_deref()).collect::<BTreeSet<_>>().len();
                        let merged = merge_rich_matches(result.matches.iter().collect());
                        let (status, resolutions, detail) = match_author_resolutions(candidate, &merged.authors);
                        let detail = detail.or_else(|| (work_count > 1).then(|| format!("merged author evidence from {work_count} Open Library work records")));
                        (status, resolutions, detail)
                    }
                };
                let applied = {
                    let mut connection = self.db.get().map_err(BackendError::operation)?;
                    db::apply_author_enrichment(&mut connection, candidate.content_hash.clone(), OPEN_LIBRARY_AUTHOR_PROVIDER, &candidate.isbn, &resolutions, status, detail.as_deref()).map_err(BackendError::operation)?
                };
                updated += applied;
            }
        }
        Ok(updated)
    }
}

fn merge_rich_matches(matches: Vec<&RichWorkMetadataMatch>) -> RichWorkMetadataMatch {
    metadata_contract::merge_rich_work_matches(matches, classification_similarity_keys)
}

fn classification_similarity_keys(scheme: ClassificationScheme, notation: &str) -> Vec<String> {
    let system_id = match scheme {
        ClassificationScheme::LibraryOfCongress => subject_projection::LCC_SYSTEM_ID,
        ClassificationScheme::DeweyDecimal => subject_projection::DDC_SYSTEM_ID,
        ClassificationScheme::Bisac => subject_projection::BISAC_SYSTEM_ID,
    };
    subject_projection::unified_subject_similarity_keys(system_id, notation)
}

fn rich_subjects(matched: &RichWorkMetadataMatch) -> Result<Vec<PublicationSubject>, BackendError> {
    let mut subjects = Vec::new();
    for heading in &matched.subjects {
        let name = heading.components.join(" / ");
        subjects.push(PublicationSubject::new(heading.authority_uri.clone(), name, &heading.source, Some(heading.authority.clone()), None).map_err(BackendError::operation)?);
    }
    for classification in &matched.classifications {
        let authority = match classification.scheme {
            ClassificationScheme::LibraryOfCongress => "lcc",
            ClassificationScheme::DeweyDecimal => "ddc",
            ClassificationScheme::Bisac => "bisac",
        };
        let source = match (matched.open_library_work_id.is_some(), classification.source) {
            (true, ClassificationSource::ExactEdition) => "openlibrary:metadata:exact-edition",
            (true, ClassificationSource::Work) => "openlibrary:metadata:work",
            (false, ClassificationSource::ExactEdition) => "metadata:exact-edition",
            (false, ClassificationSource::Work) => "metadata:work",
        };
        subjects.push(PublicationSubject::new(None, &classification.notation, source, Some(authority.to_owned()), Some(classification.notation.clone())).map_err(BackendError::operation)?);
    }
    for (authority, references) in [("wikidata-place", &matched.places), ("wikidata-person", &matched.people)] {
        for reference in references {
            let source = match reference.source {
                EntitySource::ExactEdition => "openlibrary:metadata:exact-edition",
                EntitySource::Work => "openlibrary:metadata:work",
            };
            subjects.push(PublicationSubject::new(None, &reference.name, source, Some(authority.to_owned()), reference.wikidata_id.clone()).map_err(BackendError::operation)?);
        }
    }
    subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
    subjects.dedup();
    Ok(subjects)
}

fn entities_for_match(matched: &RichWorkMetadataMatch, places: &HashMap<String, PlaceEntity>, people: &HashMap<String, PersonEntity>) -> (Vec<PlaceEntity>, Vec<PersonEntity>) {
    let mut pending = matched.places.iter().filter_map(|reference| reference.wikidata_id.clone()).collect::<Vec<_>>();
    let mut place_ids = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !place_ids.insert(id.clone()) {
            continue;
        }
        if let Some(place) = places.get(&id) {
            pending.extend(place.parent_ids.iter().chain(&place.country_ids).cloned());
        }
    }
    let places = place_ids.into_iter().filter_map(|id| places.get(&id).cloned()).collect();
    let people = matched.people.iter().filter_map(|reference| reference.wikidata_id.as_ref()).collect::<BTreeSet<_>>().into_iter().filter_map(|id| people.get(id).cloned()).collect();
    (places, people)
}

fn match_author_resolutions(candidate: &db::AuthorEnrichmentCandidate, remote: &[metadata_contract::AuthorMetadata]) -> (&'static str, Vec<(usize, String, ExternalAuthorId)>, Option<String>) {
    let mut remote_by_name = HashMap::<String, Vec<&metadata_contract::AuthorMetadata>>::new();
    for author in remote {
        let Some(name) = author.name.as_deref().and_then(|name| AuthorName::parse(name).ok()) else { continue };
        remote_by_name.entry(name.match_key().as_str().to_owned()).or_default().push(author);
    }
    let mut used = HashSet::new();
    let mut resolutions = Vec::new();
    for credit in &candidate.authors {
        let Some(name) = AuthorName::parse(&credit.name).ok() else { continue };
        let Some(matches) = remote_by_name.get(name.match_key().as_str()).filter(|matches| matches.len() == 1) else { continue };
        let matched = matches[0];
        if !used.insert(matched.open_library_author_id.as_str()) {
            continue;
        }
        let Ok(external_id) = ExternalAuthorId::parse(AuthorAuthority::OpenLibrary, &matched.open_library_author_id) else { continue };
        resolutions.push((credit.position, matched.name.clone().unwrap_or_else(|| credit.name.clone()), external_id));
    }
    if resolutions.is_empty() {
        ("ambiguous", resolutions, Some("Open Library authors could not be matched uniquely to the scanned author credits".to_owned()))
    } else {
        let unresolved = candidate.authors.len().saturating_sub(resolutions.len());
        let detail = (unresolved > 0).then(|| format!("resolved {} author credits; {unresolved} remained ambiguous", resolutions.len()));
        ("updated", resolutions, detail)
    }
}

use crate::app::BackendError;
use crate::library::database as db;
use crate::library::LibraryBackend;
use crate::ContentHash;
use book_metadata::external::{ExternalClassificationScheme, ExternalMetadataProviders, ExternalSubjectScheme};
use book_metadata::library_of_congress::LibraryOfCongressProvider;
use library_model::PublicationSubject;
use library_model::{AuthorAuthority, AuthorName, ExternalAuthorId};
use metadata_contract::{
    AuthorSource, Classification, ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityRequest, EditionIdentityStatus, EntitySource, LookupStatus, MetadataEnrichmentRequest, PersonEntity, PlaceEntity,
    RichWorkMetadataMatch,
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

fn popularity_score(popularity: &metadata_contract::WorkPopularity) -> (u64, u64, u64) {
    (popularity.reading_log_count(), popularity.rating_count, popularity.rating_sum)
}

/// LibraryThing's thingISBN clusters can be broad, while every metadata
/// response also contains the complete sibling-edition union for its Open
/// Library work. Count only classifications attached to the independently
/// queried edition as votes; otherwise repeating the same work union for 128
/// ISBNs would turn every outlier into apparent unanimous evidence.
fn librarything_classification_consensus(evidence: &LibraryThingWorkEvidence) -> Vec<Classification> {
    let mut support = BTreeMap::<(ClassificationScheme, String), usize>::new();
    // An edition can contain the same classification in several formatting
    // variants. Normalize and count it no more than once for that edition.
    for classifications in evidence.exact_classifications_by_edition.values() {
        let normalized = classifications.iter().filter_map(|(scheme, notation)| metadata_contract::classification_consensus_notation(*scheme, notation).map(|notation| (*scheme, notation))).collect::<BTreeSet<_>>();
        for classification in normalized {
            *support.entry(classification).or_default() += 1;
        }
    }

    support.into_iter().filter_map(|((scheme, notation), votes)| (votes >= 2).then_some(Classification { scheme, notation, source: ClassificationSource::Work })).collect()
}

fn collect_alternative_edition_evidence(matches: &mut [RichWorkMetadataMatch], requested_isbn: &str, support: &mut BTreeMap<(ClassificationScheme, String), BTreeSet<String>>) {
    for matched in matches {
        let edition_group = if matched.exact_edition_ids.is_empty() {
            format!("isbn:{requested_isbn}")
        } else {
            let mut edition_ids = matched.exact_edition_ids.clone();
            edition_ids.sort();
            edition_ids.dedup();
            edition_ids.join(",")
        };
        matched.classifications.retain(|classification| {
            if classification.source == ClassificationSource::ExactEdition {
                if let Some(notation) = metadata_contract::classification_consensus_notation(classification.scheme, &classification.notation) {
                    support.entry((classification.scheme, notation)).or_default().insert(edition_group.clone());
                }
                false
            } else {
                true
            }
        });
        for author in &mut matched.authors {
            if author.source == AuthorSource::ExactEdition {
                author.source = AuthorSource::Work;
            }
        }
        for entity in matched.places.iter_mut().chain(&mut matched.people) {
            if entity.source == EntitySource::ExactEdition {
                entity.source = EntitySource::Work;
            }
        }
    }
}

fn global_alternative_edition_consensus(support: BTreeMap<(ClassificationScheme, String), BTreeSet<String>>) -> Vec<Classification> {
    support.into_iter().filter_map(|((scheme, notation), edition_groups)| (edition_groups.len() >= 2).then_some(Classification { scheme, notation, source: ClassificationSource::Work })).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EditionIdentityReviewStatus {
    Ambiguous,
    NoMatch,
    Failed,
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct EditionIdentityReviewCandidate {
    pub isbn13: String,
    pub edition_id: String,
    pub work_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub matched_publisher: bool,
    pub matched_publication_year: bool,
    /// External classifications resolved to paths in Bokheim's unified
    /// taxonomy. Raw provider codes stay out of the review interface.
    pub classifications: Vec<String>,
}

impl From<metadata_contract::EditionIdentityMatch> for EditionIdentityReviewCandidate {
    fn from(candidate: metadata_contract::EditionIdentityMatch) -> Self {
        let classifications = candidate
            .classifications
            .iter()
            .flat_map(|classification| match classification.scheme {
                ClassificationScheme::LibraryOfCongress => library_model::unified_lcc_subject_paths(&classification.notation),
                ClassificationScheme::DeweyDecimal => library_model::unified_ddc_subject_paths(&classification.notation),
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
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
}

fn candidate_classification_rank(candidate: &EditionIdentityReviewCandidate) -> (usize, usize, usize) {
    let depths = candidate.classifications.iter().map(|path| path.split('/').filter(|part| !part.trim().is_empty()).count()).collect::<Vec<_>>();
    (depths.len(), depths.iter().sum(), depths.into_iter().max().unwrap_or(0))
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct EditionIdentityReview {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub inferred_title: Option<String>,
    pub inferred_author: Option<String>,
    pub inferred_publication_year: Option<i32>,
    pub status: EditionIdentityReviewStatus,
    pub detail: Option<String>,
    pub candidates: Vec<EditionIdentityReviewCandidate>,
    pub candidates_truncated: bool,
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct MissingSubjectsBook {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub title: String,
    pub isbn: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SubjectEnrichmentStatus {
    Updated,
    NoMatch,
    Ambiguous,
    Failed,
}

impl SubjectEnrichmentStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::NoMatch => "no_match",
            Self::Ambiguous => "ambiguous",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct SubjectEnrichmentOutcome {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub provider_id: String,
    pub status: SubjectEnrichmentStatus,
    pub imported_subjects: usize,
    pub detail: Option<String>,
}

impl LibraryBackend {
    pub fn edition_identity_reviews(&self) -> Result<Vec<EditionIdentityReview>, BackendError> {
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
                        let mut candidates = review.candidates.into_iter().map(Into::into).collect::<Vec<_>>();
                        candidates.sort_by(|left, right| candidate_classification_rank(right).cmp(&candidate_classification_rank(left)));
                        let (query_title, inferred_author, inferred_year) = edition_identity_title_author_and_year(&review.title);
                        let inferred_title = (query_title.trim() != review.title.trim()).then_some(query_title);
                        let inferred_author = inferred_author.filter(|author| !review.authors.iter().any(|existing| existing.eq_ignore_ascii_case(author)));
                        let inferred_publication_year = review.publication_year.is_none().then_some(inferred_year).flatten();
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
    pub async fn refresh_edition_identity_candidate_classifications(&self) -> Result<usize, BackendError> {
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
            if let Some(isbn) = isbn_from_filename_like_title(&candidate.title) {
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
                    let (title, inferred_author, inferred_year) = edition_identity_title_author_and_year(&candidate.title);
                    let mut authors = candidate.authors.clone();
                    if let Some(author) = inferred_author {
                        if !authors.iter().any(|existing| existing.eq_ignore_ascii_case(&author)) {
                            authors.push(author);
                        }
                    }
                    EditionIdentityQuery { query_id: index.to_string(), title, authors, publishers: candidate.publishers.clone(), publication_year: candidate.publication_year.or(inferred_year) }
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
    pub(crate) async fn enrich_open_library_rich(&self, limit: usize, retry_failed: bool) -> Result<usize, BackendError> {
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
            let mut alternative_classification_support = BTreeMap::new();
            let mut matched_isbn_count = 0_usize;
            let mut omitted = false;
            for isbn in &candidate.isbns {
                match results.get(isbn) {
                    None => omitted = true,
                    Some(result) if !result.matches.is_empty() && (result.status == LookupStatus::Matched || result.status == LookupStatus::Ambiguous) => {
                        matched_isbn_count += 1;
                        let mut matches = result.matches.clone();
                        if !candidate.exact_isbns.contains(isbn) {
                            collect_alternative_edition_evidence(&mut matches, isbn, &mut alternative_classification_support);
                        }
                        matched_records.extend(matches);
                    }
                    Some(_) => {}
                }
            }
            let global_alternative_classifications = global_alternative_edition_consensus(alternative_classification_support);
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
                    RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications: Vec::new(), authors: Vec::new(), places: Vec::new(), people: Vec::new(), popularity: None }
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
                            if matched.popularity.as_ref().is_some_and(|popularity| popularity_score(popularity) > popularity_score(&evidence.popularity)) {
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
                    (popularity_score(&left.popularity), left.isbn_hits, std::cmp::Reverse(left.classifications.len()), left_key).cmp(&(
                        popularity_score(&right.popularity),
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
        let mut classifications = eligible.iter().flat_map(|(_, evidence)| librarything_classification_consensus(evidence)).collect::<Vec<_>>();
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

    pub fn books_missing_subjects(&self, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<MissingSubjectsBook>, BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        let candidates = db::subject_enrichment_candidates(&connection, provider_id, limit, retry_failed).map_err(BackendError::operation)?;
        Ok(candidates.into_iter().map(|candidate| MissingSubjectsBook { content_hash: candidate.content_hash, title: candidate.title, isbn: candidate.isbn }).collect())
    }

    /// Enriches books in provider order. Once a provider adds subjects, later
    /// providers no longer see that book as missing; failures and empty results
    /// remain eligible for the next provider but are not retried automatically.
    pub async fn enrich_missing_subjects(&self, providers: &ExternalMetadataProviders, limit_per_provider: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentOutcome>, BackendError> {
        let mut outcomes = Vec::new();
        let mut taxonomy_candidates = HashSet::new();
        for provider_id in providers.provider_ids() {
            let candidates = {
                let connection = self.db.get().map_err(BackendError::operation)?;
                db::subject_enrichment_candidates(&connection, provider_id.as_str(), limit_per_provider, retry_failed).map_err(BackendError::operation)?
            };
            eprintln!("SUBJECT_ENRICHMENT provider={} candidates={}", provider_id.as_str(), candidates.len());
            for candidate in candidates {
                eprintln!("SUBJECT_ENRICHMENT lookup provider={} isbn={} title={:?}", provider_id.as_str(), candidate.isbn, candidate.title);
                let lookup = providers.lookup_isbn_with(&provider_id, &candidate.isbn).await.expect("provider IDs originate from this registry");
                let (status, subjects, detail) = match lookup.result {
                    Err(error) => (SubjectEnrichmentStatus::Failed, Vec::new(), Some(error.to_string())),
                    Ok(lookup) => {
                        let mut matching = lookup.records.into_iter().filter(|record| record.isbns.iter().any(|isbn| lookup.queried_isbns.contains(isbn))).collect::<Vec<_>>();
                        match matching.len() {
                            0 => (SubjectEnrichmentStatus::NoMatch, Vec::new(), None),
                            1 => {
                                let subjects = provider_subjects(provider_id.as_str(), matching.pop().expect("one matching record"))?;
                                if subjects.is_empty() {
                                    (SubjectEnrichmentStatus::NoMatch, subjects, Some("matching record contained no supported subjects or classifications".to_owned()))
                                } else {
                                    (SubjectEnrichmentStatus::Updated, subjects, None)
                                }
                            }
                            count => (SubjectEnrichmentStatus::Ambiguous, Vec::new(), Some(format!("provider returned {count} records for the same ISBN"))),
                        }
                    }
                };
                let imported_subjects = {
                    let mut connection = self.db.get().map_err(BackendError::operation)?;
                    db::apply_external_subjects(&mut connection, candidate.content_hash.clone(), provider_id.as_str(), &candidate.isbn, subjects, status.as_str(), detail.as_deref()).map_err(BackendError::operation)?
                };
                let outcome = SubjectEnrichmentOutcome { content_hash: candidate.content_hash, provider_id: provider_id.as_str().to_owned(), status, imported_subjects, detail };
                if outcome.imported_subjects > 0 {
                    taxonomy_candidates.insert(outcome.content_hash.clone());
                }
                eprintln!("SUBJECT_ENRICHMENT result provider={} isbn={} status={:?} imported_subjects={} detail={:?}", outcome.provider_id, candidate.isbn, outcome.status, outcome.imported_subjects, outcome.detail);
                outcomes.push(outcome);
            }
        }
        if let Err(error) = self.app()?.hydrate_taxonomy_for_books(self, &taxonomy_candidates.into_iter().collect::<Vec<_>>()).await {
            log::warn!("subject metadata was applied, but missing taxonomy codes could not be hydrated: {error}");
        }
        Ok(outcomes)
    }

    pub async fn enrich_missing_subjects_from_library_of_congress(&self, limit: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentOutcome>, BackendError> {
        let providers = ExternalMetadataProviders::new().with(LibraryOfCongressProvider::new().map_err(BackendError::operation)?);
        self.enrich_missing_subjects(&providers, limit, retry_failed).await
    }
}

fn title_without_parenthetical_segments(title: &str) -> String {
    let mut depth = 0_u32;
    title
        .chars()
        .filter(|character| match character {
            '(' => {
                depth = depth.saturating_add(1);
                false
            }
            ')' if depth > 0 => {
                depth -= 1;
                false
            }
            _ => depth == 0,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extracts conservative identity evidence from filename-like titles. Keep it
/// as query evidence only: scanned metadata remains untouched until an Open
/// Library identity is selected.
fn edition_identity_title_author_and_year(title: &str) -> (String, Option<String>, Option<i32>) {
    let trimmed = title.trim();
    let segments = parenthetical_segments(trimmed);
    let mut inferred_author = None;
    let mut inferred_year = None;
    for segment in &segments {
        inferred_year = inferred_year.or_else(|| plausible_publication_year(segment));
        if inferred_author.is_some() {
            continue;
        }
        let lower = segment.to_lowercase();
        let (author, explicit) = if lower.starts_with("by-") || lower.starts_with("by_") || lower.starts_with("by ") { (&segment[3..], true) } else { (segment.as_str(), false) };
        let author = author.replace(['-', '_'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
        if !author.is_empty() && (explicit || looks_like_person_name(&author)) {
            inferred_author = Some(author);
        }
    }
    let stripped = title_without_parenthetical_segments(trimmed);
    let clean_tokens = stripped
        .split_whitespace()
        .filter(|token| {
            let key = token.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
            !key.is_empty() && !matches!(key.as_str(), "itebooks" | "ibooker" | "zliborg" | "zlibrary")
        })
        .collect::<Vec<_>>();
    let cleaned = clean_tokens.join(" ").trim_matches(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | '–' | '—')).trim().to_owned();
    let mut title = if cleaned.is_empty() { stripped } else { cleaned };
    if let Some(by) = rfind_ascii_case_insensitive(&title, " by ") {
        let inline_author = title[by + 4..].trim();
        if looks_like_person_name(inline_author) {
            inferred_author.get_or_insert_with(|| inline_author.to_owned());
            title = title[..by].trim_matches(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | '–' | '—')).to_owned();
        }
    }
    (title, inferred_author, inferred_year)
}

/// Returns one unambiguous, checksum-valid ISBN embedded in a filename-like
/// title. Hyphenated ISBNs are accepted, while multiple distinct ISBNs fall
/// back to normal edition matching rather than choosing one arbitrarily.
fn isbn_from_filename_like_title(title: &str) -> Option<String> {
    let mut candidate = String::new();
    let mut isbns = BTreeSet::new();
    let finish_candidate = |candidate: &mut String, isbns: &mut BTreeSet<String>| {
        if let Some(isbn) = library_model::canonical_isbn(candidate) {
            isbns.insert(isbn);
        }
        candidate.clear();
    };
    for character in title.chars() {
        if character.is_ascii_digit() || matches!(character, 'x' | 'X' | '-') {
            candidate.push(character);
        } else if !candidate.is_empty() {
            finish_candidate(&mut candidate, &mut isbns);
        }
    }
    if !candidate.is_empty() {
        finish_candidate(&mut candidate, &mut isbns);
    }
    (isbns.len() == 1).then(|| isbns.into_iter().next()).flatten()
}

fn rfind_ascii_case_insensitive(value: &str, needle: &str) -> Option<usize> {
    value.as_bytes().windows(needle.len()).rposition(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn parenthetical_segments(value: &str) -> Vec<String> {
    let mut depth = 0_u32;
    let mut segment = String::new();
    let mut segments = Vec::new();
    for character in value.chars() {
        match character {
            '(' => {
                if depth > 0 {
                    segment.push(character);
                }
                depth = depth.saturating_add(1);
            }
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let value = segment.trim();
                    if !value.is_empty() {
                        segments.push(value.to_owned());
                    }
                    segment.clear();
                } else {
                    segment.push(character);
                }
            }
            _ if depth > 0 => segment.push(character),
            _ => {}
        }
    }
    segments
}

fn plausible_publication_year(value: &str) -> Option<i32> {
    value.split(|character: char| !character.is_ascii_digit()).filter(|part| part.len() == 4).filter_map(|part| part.parse::<i32>().ok()).find(|year| (1450..=2100).contains(year))
}

fn looks_like_person_name(value: &str) -> bool {
    const PARTICLES: &[&str] = &["al", "bin", "da", "de", "del", "den", "der", "di", "du", "la", "le", "van", "von"];
    const NON_NAMES: &[&str] = &["abridged", "book", "edition", "guide", "illustrated", "introduction", "manual", "novel", "revised", "series", "textbook", "translation", "unabridged", "volume"];
    let words = value.split_whitespace().collect::<Vec<_>>();
    if !(2..=5).contains(&words.len()) || words.iter().any(|word| NON_NAMES.contains(&word.to_lowercase().as_str())) {
        return false;
    }
    words.iter().all(|word| {
        let clean = word.trim_matches(|character: char| matches!(character, '.' | ',' | '\'' | '’'));
        if clean.is_empty() || !clean.chars().all(|character| character.is_alphabetic() || matches!(character, '-' | '\'' | '’')) {
            return false;
        }
        PARTICLES.contains(&clean.to_lowercase().as_str()) || clean.chars().next().is_some_and(char::is_uppercase)
    })
}

fn merge_rich_matches(matches: Vec<&RichWorkMetadataMatch>) -> RichWorkMetadataMatch {
    metadata_contract::merge_rich_work_matches(matches, classification_similarity_keys)
}

fn classification_similarity_keys(scheme: ClassificationScheme, notation: &str) -> Vec<String> {
    let system_id = match scheme {
        ClassificationScheme::LibraryOfCongress => library_model::LCC_SYSTEM_ID,
        ClassificationScheme::DeweyDecimal => library_model::DDC_SYSTEM_ID,
    };
    library_model::unified_subject_similarity_keys(system_id, notation)
}

fn rich_subjects(matched: &RichWorkMetadataMatch) -> Result<Vec<PublicationSubject>, BackendError> {
    let mut subjects = Vec::new();
    for classification in &matched.classifications {
        let authority = match classification.scheme {
            ClassificationScheme::LibraryOfCongress => "lcc",
            ClassificationScheme::DeweyDecimal => "ddc",
        };
        let source = match classification.source {
            ClassificationSource::ExactEdition => "openlibrary:metadata:exact-edition",
            ClassificationSource::Work => "openlibrary:metadata:work",
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

fn provider_subjects(provider_id: &str, record: book_metadata::external::ExternalMetadataRecord) -> Result<Vec<PublicationSubject>, BackendError> {
    let mut subjects = Vec::new();
    for heading in record.subjects {
        let name = heading.display_path();
        let authority = match heading.scheme {
            ExternalSubjectScheme::Lcsh => "lcsh".to_owned(),
            ExternalSubjectScheme::Lcgft => "lcgft".to_owned(),
            ExternalSubjectScheme::Other(authority) => authority,
        };
        subjects.push(PublicationSubject::new(heading.authority_uri, name, provider_id, Some(authority), None).map_err(BackendError::operation)?);
    }
    for classification in record.classifications {
        let authority = match classification.scheme {
            ExternalClassificationScheme::Lcc => "lcc".to_owned(),
            ExternalClassificationScheme::Ddc => "ddc".to_owned(),
            ExternalClassificationScheme::Other(authority) => authority,
        };
        subjects.push(PublicationSubject::new(None, &classification.code, provider_id, Some(authority), Some(classification.code.clone())).map_err(BackendError::operation)?);
    }
    subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
    subjects.dedup();
    Ok(subjects)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_alternative_edition_evidence, edition_identity_title_author_and_year, global_alternative_edition_consensus, isbn_from_filename_like_title, librarything_classification_consensus, merge_rich_matches, LibraryThingWorkEvidence,
    };
    use metadata_contract::{Classification, ClassificationScheme, ClassificationSource, EntityReference, EntitySource, RichWorkMetadataMatch};
    use std::collections::BTreeSet;

    #[test]
    fn edition_identity_extracts_explicit_filename_author() {
        assert_eq!(edition_identity_title_author_and_year("An Illustrated Guide (by-Kenneth-Ray-Stubbs)"), ("An Illustrated Guide".to_owned(), Some("Kenneth Ray Stubbs".to_owned()), None));
    }

    #[test]
    fn edition_identity_extracts_plain_person_parenthesis_conservatively() {
        assert_eq!(edition_identity_title_author_and_year("An Illustrated Guide (Kenneth Ray Stubbs)"), ("An Illustrated Guide".to_owned(), Some("Kenneth Ray Stubbs".to_owned()), None));
        assert_eq!(edition_identity_title_author_and_year("Physical Chemistry (Second Edition)"), ("Physical Chemistry".to_owned(), None, None));
    }

    #[test]
    fn edition_identity_extracts_noisy_filename_title_author_and_year() {
        assert_eq!(edition_identity_title_author_and_year("(it-ebooks-2019) it-ebooks - Algorithms (Jeff Erickson)-iBooker it-ebooks (2019)(1)"), ("Algorithms".to_owned(), Some("Jeff Erickson".to_owned()), Some(2019)));
    }

    #[test]
    fn edition_identity_extracts_inline_by_author_before_source_tag() {
        assert_eq!(edition_identity_title_author_and_year("A Concise History of France by Roger Price (z-lib.org)"), ("A Concise History of France".to_owned(), Some("Roger Price".to_owned()), None));
    }

    #[test]
    fn edition_identity_extracts_checksum_valid_filename_isbn() {
        assert_eq!(isbn_from_filename_like_title("9780198571001.pdf").as_deref(), Some("9780198571001"));
        assert_eq!(isbn_from_filename_like_title("book-978-0-19-857100-1 scan.pdf").as_deref(), Some("9780198571001"));
        assert_eq!(isbn_from_filename_like_title("9780198571002.pdf"), None);
    }

    #[test]
    fn librarything_consensus_keeps_multiple_supported_branches_per_scheme() {
        let mut evidence = LibraryThingWorkEvidence::default();
        let editions = [
            ("OL1M", [(ClassificationScheme::LibraryOfCongress, "QA76.6"), (ClassificationScheme::DeweyDecimal, "005.1")]),
            ("OL2M", [(ClassificationScheme::LibraryOfCongress, "QA76.6.A1 2001"), (ClassificationScheme::DeweyDecimal, "005.1")]),
            ("OL3M", [(ClassificationScheme::LibraryOfCongress, "PN1993"), (ClassificationScheme::DeweyDecimal, "791.43")]),
            ("OL4M", [(ClassificationScheme::LibraryOfCongress, "PN1993.A2"), (ClassificationScheme::DeweyDecimal, "791.43")]),
            ("OL5M", [(ClassificationScheme::LibraryOfCongress, "B580"), (ClassificationScheme::DeweyDecimal, "999")]),
        ];
        for (edition, classifications) in editions {
            evidence.exact_classifications_by_edition.insert(edition.to_owned(), classifications.into_iter().map(|(scheme, notation)| (scheme, notation.to_owned())).collect());
        }
        // Work-wide union data repeats in every ISBN response and must not
        // become a vote without exact-edition evidence.
        evidence.classifications.insert((ClassificationScheme::LibraryOfCongress, "Z999".to_owned()));

        let retained = librarything_classification_consensus(&evidence);
        let notations = retained.iter().map(|classification| classification.notation.as_str()).collect::<BTreeSet<_>>();
        assert!(notations.contains("QA76.6"));
        assert!(notations.contains("PN1993"));
        assert!(notations.contains("005.1"));
        assert!(notations.contains("791.43"));
        assert!(!notations.contains("B580"));
        assert!(!notations.contains("999"));
        assert!(!notations.contains("Z999"));
    }

    #[test]
    fn librarything_consensus_collapses_odyssey_edition_call_numbers() {
        assert_eq!(metadata_contract::classification_consensus_notation(ClassificationScheme::LibraryOfCongress, "PA4025.A5 R6 1999").as_deref(), Some("PA4025"));
        assert_eq!(metadata_contract::classification_consensus_notation(ClassificationScheme::LibraryOfCongress, "PA4025.A5W56 2018eb").as_deref(), Some("PA4025"));
        assert_eq!(metadata_contract::classification_consensus_notation(ClassificationScheme::DeweyDecimal, "883/.01").as_deref(), Some("883.01"));
    }

    #[test]
    fn global_consensus_removes_singletons_across_alternative_work_matches() {
        let matched = |work: &str, edition: &str, notation: &str| RichWorkMetadataMatch {
            open_library_work_id: Some(work.to_owned()),
            exact_edition_ids: vec![edition.to_owned()],
            classifications: vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: notation.to_owned(), source: ClassificationSource::ExactEdition }],
            authors: Vec::new(),
            places: Vec::new(),
            people: Vec::new(),
            popularity: None,
        };
        let mut support = std::collections::BTreeMap::new();
        let mut first = vec![matched("OL1W", "OL1M", "PR4034.P7 1995")];
        let mut second = vec![matched("OL2W", "OL2M", "PR4034 .P7 2006")];
        let mut singleton = vec![matched("OL3W", "OL3M", "PS3558.A4758 P75t 1999")];
        collect_alternative_edition_evidence(&mut first, "9780000000001", &mut support);
        collect_alternative_edition_evidence(&mut second, "9780000000002", &mut support);
        collect_alternative_edition_evidence(&mut singleton, "9780000000003", &mut support);

        assert!(first[0].classifications.is_empty());
        assert_eq!(global_alternative_edition_consensus(support), vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "PR4034".to_owned(), source: ClassificationSource::Work }]);
    }

    #[test]
    fn rich_enrichment_merges_metadata_from_split_work_records() {
        let first = RichWorkMetadataMatch {
            open_library_work_id: Some("OL1W".to_owned()),
            exact_edition_ids: vec!["OL1M".to_owned()],
            classifications: vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "GN496".to_owned(), source: ClassificationSource::ExactEdition }],
            authors: Vec::new(),
            places: Vec::new(),
            people: Vec::new(),
            popularity: None,
        };
        let second = RichWorkMetadataMatch {
            open_library_work_id: Some("OL2W".to_owned()),
            exact_edition_ids: vec!["OL2M".to_owned()],
            classifications: vec![Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "306".to_owned(), source: ClassificationSource::Work }],
            authors: Vec::new(),
            places: vec![EntityReference { wikidata_id: None, name: "Developing countries".to_owned(), source: EntitySource::Work }],
            people: Vec::new(),
            popularity: None,
        };

        let merged = merge_rich_matches(vec![&first, &second]);
        assert_eq!(merged.exact_edition_ids, ["OL1M", "OL2M"]);
        assert_eq!(merged.classifications.len(), 2);
        assert_eq!(merged.places.len(), 1);
    }
}

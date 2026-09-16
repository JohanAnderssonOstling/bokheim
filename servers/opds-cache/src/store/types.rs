use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedCatalogPage {
    pub page: CatalogPage,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub provider_paginated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSourceState {
    pub epoch: String,
    pub active_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedCatalogPublication {
    pub publication: CatalogPublication,
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSearchMatches {
    pub title_matches: Vec<SequencedCatalogPublication>,
    pub author_matches: Vec<SequencedCatalogPublication>,
    pub toc_matches: Vec<SequencedCatalogPublication>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogPublicationPopularity {
    pub want_to_read_count: u64,
    pub currently_reading_count: u64,
    pub already_read_count: u64,
    pub rating_count: u64,
    pub rating_sum: u64,
}

impl CatalogPublicationPopularity {
    pub fn reading_log_count(&self) -> u64 {
        self.want_to_read_count.saturating_add(self.currently_reading_count).saturating_add(self.already_read_count)
    }
}

/// The same lossless EPUB package metadata retained for imported library
/// books, plus the publication's hierarchical navigation document. This is a
/// catalogue-server sidecar and is deliberately not repeated in every card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEpubMetadata {
    pub book: publication_model::BookMetadataState,
    pub toc: Vec<publication_model::PublicationTocEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSourceInventory {
    pub source_id: CatalogSourceId,
    pub crawl_status: String,
    pub last_crawl_started_at: Option<u64>,
    pub last_crawl_succeeded_at: Option<u64>,
    pub last_crawl_error: Option<String>,
    pub crawl_frontier_count: u64,
    pub active_page_count: u64,
    pub active_publication_count: u64,
    pub active_epub_count: u64,
    pub epub_extractions: CatalogEpubExtractionInventory,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogInventory {
    pub search_index_ready: bool,
    pub active_page_count: u64,
    pub active_publication_count: u64,
    pub active_epub_count: u64,
    pub epub_extractions: CatalogEpubExtractionInventory,
    pub cover_downloads: CatalogCoverDownloadInventory,
    pub metadata_backfill: CatalogMetadataBackfillInventory,
    pub description_backfill: CatalogDescriptionBackfillInventory,
    pub identity_reviews: CatalogIdentityReviewInventory,
    pub sources: Vec<CatalogSourceInventory>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogCoverDownloadInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub attempts: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogCoverDownloadJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub image_url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogMetadataBackfillInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub ocr_pending: u64,
    pub ocr_running: u64,
    pub ocr_succeeded: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogDescriptionBackfillInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub no_description: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CatalogIdentityReviewInventory {
    pub ambiguous: u64,
    pub no_match: u64,
    pub insufficient_metadata: u64,
    pub failed: u64,
    pub resolved: u64,
    pub rejected: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogIdentityReview {
    pub review_id: String,
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub status: String,
    pub detail: Option<String>,
    pub candidates: Vec<CatalogIdentityReviewCandidate>,
    pub candidates_truncated: bool,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogIdentityReviewCandidate {
    pub isbn13: String,
    pub edition_id: String,
    pub work_id: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub matched_publisher: bool,
    pub matched_publication_year: bool,
    pub classifications: Vec<String>,
}

impl From<metadata_contract::EditionIdentityMatch> for CatalogIdentityReviewCandidate {
    fn from(candidate: metadata_contract::EditionIdentityMatch) -> Self {
        let classifications = candidate
            .classifications
            .iter()
            .flat_map(|classification| match classification.scheme {
                metadata_contract::ClassificationScheme::LibraryOfCongress => subject_projection::unified_lcc_subject_paths(&classification.notation),
                metadata_contract::ClassificationScheme::DeweyDecimal => Vec::new(),
                metadata_contract::ClassificationScheme::Bisac => subject_projection::unified_subject_paths(subject_projection::BISAC_SYSTEM_ID, &classification.notation),
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            isbn13: candidate.canonical_isbn13,
            edition_id: candidate.open_library_edition_id,
            work_id: candidate.open_library_work_id,
            title: candidate.title,
            subtitle: candidate.subtitle,
            authors: candidate.authors,
            publishers: candidate.publishers,
            publication_year: candidate.publication_year,
            matched_publisher: candidate.matched_publisher,
            matched_publication_year: candidate.matched_publication_year,
            classifications,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogEpubExtractionInventory {
    pub pending: u64,
    pub running: u64,
    pub downloading: u64,
    pub extracting: u64,
    pub enriching: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub attempts: u64,
    pub downloaded_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEpubExtractionJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub book_format: CatalogBookFormat,
    pub cover_key: Option<String>,
    /// Local acquisition staged by the download worker and consumed by an
    /// extraction worker. This is durable so a restart does not force the
    /// network and CPU stages to be coupled again.
    pub staged_path: Option<String>,
    pub staged_metadata_only: bool,
    /// Monotonic claim token. Every durable write from a worker must match it.
    pub attempt: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogMetadataIndex {
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub isbns: Vec<String>,
    pub publication_year: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogMetadataBackfillIndexedJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub index: CatalogMetadataIndex,
    pub attempt: u64,
}

#[derive(Clone, Debug)]
pub struct CatalogMetadataBackfillIndexedUpdate {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub subjects: Vec<PublicationSubject>,
    pub popularity: Option<CatalogPublicationPopularity>,
    pub index: CatalogMetadataIndex,
    pub identity_result: Option<metadata_contract::EditionIdentityResult>,
    pub attempt: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogOcrBackfillJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub book_format: CatalogBookFormat,
    pub attempt: u64,
}

#[derive(Clone, Debug)]
pub struct CatalogDescriptionBackfillJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub isbns: Vec<String>,
    pub attempt: u64,
}

#[derive(Clone, Debug)]
pub struct CatalogDescriptionBackfillUpdate {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub description: Option<String>,
    pub subjects: Vec<PublicationSubject>,
    pub open_library_description_missing: bool,
    pub attempt: u64,
}

pub struct CatalogMetadataBackfillWriteTimings {
    pub subjects_ms: u64,
    pub authors_ms: u64,
    pub identity_review_ms: u64,
    pub index_ms: u64,
    pub status_ms: u64,
    pub transaction_ms: u64,
    pub commit_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEpubFailureExample {
    pub source_id: CatalogSourceId,
    pub acquisition_host: String,
    pub publication_id: String,
    pub attempts: u64,
    pub last_failed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEpubFailureDiagnostic {
    pub category: String,
    pub retryable: bool,
    pub count: u64,
    pub first_failed_at: u64,
    pub last_failed_at: u64,
    pub next_retry_at: Option<u64>,
    pub examples: Vec<CatalogEpubFailureExample>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CatalogSubjectProjectionAudit {
    pub dc_subject_occurrences: u64,
    pub dc_subject_occurrences_with_bisac_assignment: u64,
    pub dc_subject_occurrences_without_bisac_assignment: u64,
    pub distinct_dc_subject_headings: u64,
    pub distinct_dc_subject_headings_without_bisac_assignment: u64,
    pub bisac_book_subject_combinations: u64,
    pub bisac_books: u64,
    pub unmatched_dc_subjects: Vec<CatalogUnmatchedDcSubject>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CatalogUnmatchedDcSubject {
    pub subject: String,
    pub occurrences: u64,
    pub examples: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogCrawlGeneration {
    pub source_id: CatalogSourceId,
    pub generation: u64,
}

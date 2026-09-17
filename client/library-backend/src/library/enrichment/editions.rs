//! Edition matching and automatic subject enrichment.
use super::application::{EnrichmentWrite, apply_enrichment_writes, record_enrichment_failures};
use super::{EDITION_IDENTITY_BATCH_SIZE, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER};
use super::{OPEN_LIBRARY_RICH_PROVIDER, enrichment_parallelism};
use crate::BackendError;
use crate::ContentHash;
use crate::library::LibrarySession;
use library_database::EditionIdentityCandidate;
use client_library::prepared_enrichment as prepared;
use futures_util::{StreamExt, stream};
use metadata_contract::RichWorkMetadataMatch;
use metadata_contract::{EditionIdentityQuery, EditionIdentityRequest, EditionIdentityStatus, MAX_EDITION_QUERIES_PER_REQUEST, catalog_subjects, subject_consensus};
use book_enrichment::resolution::clean_fallback_authors;
use book_enrichment::resolution::{classification_only_fallback, rich_subjects};
use std::collections::HashMap;

fn edition_identity_query(candidate: &EditionIdentityCandidate, query_id: String) -> EditionIdentityQuery {
    EditionIdentityQuery { query_id, title: candidate.title.clone(), authors: clean_fallback_authors(&candidate.authors), publishers: candidate.publishers.clone(), book_year: candidate.book_year }
}

impl LibrarySession {
    /// Adds an ISBN only when dump evidence identifies one exact edition. An
    /// embedded/scanned ISBN always wins because books that already have one
    /// are excluded both while selecting and while applying candidates.
    pub(super) async fn resolve_open_library_edition_identities(&self, candidates: Vec<EditionIdentityCandidate>) -> Result<usize, BackendError> {
        let mut updated = 0;
        let mut unresolved = Vec::with_capacity(candidates.len());
        let mut writes: Vec<EnrichmentWrite> = Vec::new();
        for candidate in candidates {
            if let Some(isbn) = book_metadata::isbn_from_filename_like_title(&candidate.title) {
                writes.push(Box::new(move |conn| prepared::apply_filename_isbn_in_transaction(conn, candidate.content_hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &isbn).map(usize::from)));
            } else {
                unresolved.push(candidate);
            }
        }
        updated += apply_enrichment_writes(self, writes).await?;
        let batches = unresolved.chunks(EDITION_IDENTITY_BATCH_SIZE.min(MAX_EDITION_QUERIES_PER_REQUEST)).map(<[_]>::to_vec).collect::<Vec<_>>();
        let mut requests = stream::iter(
            batches
                .into_iter()
                .map(|batch| async move {
                    let queries = batch.iter().enumerate().map(|(index, candidate)| edition_identity_query(candidate, index.to_string())).collect();
                    (batch, self.metadata().resolve_edition_identities(&EditionIdentityRequest { queries }).await.map_err(BackendError::operation))
                })
                .collect::<Vec<_>>(),
        )
        .buffer_unordered(enrichment_parallelism().1);
        while let Some((batch, outcome)) = requests.next().await {
            let response = match outcome {
                Ok(response) => response,
                Err(error) => {
                    let failures = batch.iter().map(|candidate| (candidate.content_hash.clone(), candidate.title.clone())).collect::<Vec<_>>();
                    record_enrichment_failures(self, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, failures, error.to_string()).await?;
                    log::warn!("Open Library edition matching skipped a batch of {} books after an error: {error}", batch.len());
                    continue;
                }
            };
            let results = response.results.into_iter().map(|result| (result.query_id.clone(), result)).collect::<HashMap<_, _>>();
            let mut writes: Vec<EnrichmentWrite> = Vec::new();
            for (index, candidate) in batch.iter().enumerate() {
                let result = results.get(&index.to_string());
                let verification_query = edition_identity_query(candidate, index.to_string());
                if let Some(result) = result {
                    let verified = book_enrichment::related_isbn::verified_isbns(&verification_query, &candidate.resource_isbns, result);
                    let codes = book_enrichment::related_isbn::classifications(&verified, |isbn| {
                        result.matched.iter().chain(&result.candidates).filter(|record| record.canonical_isbn13 == isbn).map(|record| book_enrichment::related_isbn::classification_record(record.classifications.clone())).collect()
                    });
                    if !codes.is_empty() {
                        let record = book_enrichment::related_isbn::classification_record(codes);
                        let subjects = rich_subjects(&record).map_err(BackendError::operation)?;
                        let hash = candidate.content_hash;
                        writes.push(Box::new(move |conn| {
                            prepared::apply_rich_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_RICH_PROVIDER, "", Some(&record), &[], &[], subjects, "updated", Some("resource ISBN verified against title and authors"), false)?;
                            prepared::apply_edition_identity_in_transaction(conn, hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, "resource-isbn", None, "updated", Some("verified resource ISBN; book edition unchanged"))?;
                            Ok(1)
                        }));
                        continue;
                    }
                }
                if let Some(record) = result.and_then(|r| r.catalog_subjects.as_ref()).and_then(|m| catalog_subjects::classification_record(&verification_query, m)) {
                    let subjects = rich_subjects(&record).map_err(BackendError::operation)?;
                    let hash = candidate.content_hash;
                    let detail = serde_json::to_string(&record.classifications).map_err(BackendError::operation)?;
                    writes.push(Box::new(move |conn| {
                        prepared::apply_rich_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_RICH_PROVIDER, "", Some(&record), &[], &[], subjects, "updated", Some(&detail), false)?;
                        prepared::apply_edition_identity_in_transaction(conn, hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, "lc-title-author", None, "updated", Some(&detail))?;
                        Ok(1)
                    }));
                    continue;
                }
                let agreed_codes = result.map(|r| subject_consensus::resolved_classifications(&verification_query, r)).unwrap_or_default();
                if !agreed_codes.is_empty() {
                    let record = book_enrichment::related_isbn::classification_record(agreed_codes);
                    let subjects = rich_subjects(&record).map_err(BackendError::operation)?;
                    let hash = candidate.content_hash;
                    writes.push(Box::new(move |conn| {
                        prepared::apply_rich_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_RICH_PROVIDER, "", Some(&record), &[], &[], subjects, "updated", Some("shared edition classifications; exact ISBN unresolved"), false)?;
                        prepared::apply_edition_identity_in_transaction(conn, hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, "subject-consensus", None, "updated", Some("subject resolved; exact edition remains unresolved"))?;
                        Ok(1)
                    }));
                    continue;
                }
                if let Some(record) = result.filter(|r| r.status == EditionIdentityStatus::Matched).and_then(|r| r.matched.clone()).and_then(|matched| classification_only_fallback(&verification_query, matched)) {
                    let subjects = rich_subjects(&record).map_err(BackendError::operation)?;
                    let hash = candidate.content_hash;
                    writes.push(Box::new(move |conn| {
                        prepared::apply_rich_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_RICH_PROVIDER, "title-author", Some(&record), &[], &[], subjects, "updated", Some("verified title/author classification fallback"), false)?;
                        Ok(1)
                    }));
                }
                let (status, isbn, identifier, detail) = match result {
                    None => ("failed", None, index.to_string(), Some("metadata response omitted the edition query".to_owned())),
                    Some(result) if result.status == EditionIdentityStatus::Matched => match &result.matched {
                        Some(matched) => {
                            let discriminator = match (matched.matched_publisher, matched.matched_book_year) {
                                (true, true) => "; publisher/year tie-break",
                                (true, false) => "; publisher tie-break",
                                (false, true) => "; year tie-break",
                                (false, false) => "",
                            };
                            ("updated", Some(matched.canonical_isbn13.as_str()), matched.open_library_edition_id.clone(), Some(format!("snapshot {}; title/author match{discriminator}", response.snapshot.dump_date)))
                        }
                        None => ("failed", None, index.to_string(), Some("matched response omitted edition evidence".to_owned())),
                    },
                    Some(result) if result.status == EditionIdentityStatus::Ambiguous => ("no_match", None, index.to_string(), None),
                    Some(result) if result.status == EditionIdentityStatus::NoMatch || result.status == EditionIdentityStatus::InsufficientMetadata => ("no_match", None, index.to_string(), None),
                    Some(_) => ("failed", None, index.to_string(), Some("unsupported edition identity result".to_owned())),
                };
                let hash = candidate.content_hash.clone();
                let isbn = isbn.map(str::to_owned);
                writes.push(Box::new(move |conn| prepared::apply_edition_identity_in_transaction(conn, hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, &identifier, isbn.as_deref(), status, detail.as_deref()).map(usize::from)));
            }
            updated += apply_enrichment_writes(self, writes).await?;
        }
        Ok(updated)
    }
}

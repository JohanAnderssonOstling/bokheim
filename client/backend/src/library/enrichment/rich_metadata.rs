//! Applies classification, place, and person metadata.
use super::application::{EnrichmentWrite, apply_enrichment_writes, record_enrichment_failures};
use super::{DESCRIPTION_PROVIDER, MAX_DIRECT_CLASSIFICATIONS_WITHOUT_CONSENSUS, OPEN_LIBRARY_AUTHOR_PROVIDER, enrichment_parallelism};
use super::{ISBN_LOOKUP_BATCH_SIZE, OPEN_LIBRARY_RICH_PROVIDER};
use crate::BackendError;
use crate::library::LibrarySession;
use client_library::prepared_enrichment as prepared;
use futures_util::{StreamExt, stream};
use metadata_contract::EditionClassificationConsensus;
use metadata_contract::{LookupStatus, MetadataEnrichmentRequest};
use publication_enrichment::resolution::authoritative_author_resolutions;
use publication_enrichment::resolution::{entities_for_match, merge_rich_matches, rich_subjects};
use std::collections::{BTreeSet, HashMap};

impl LibrarySession {
    /// Batch-enriches ISBN books with classifications, subject places, and
    /// subject people. Canonical subject references are synchronized with the
    /// book; resolved entity labels and the place graph remain a rebuildable
    /// local cache for navigation.
    pub(super) async fn apply_rich_metadata_candidates(&self, candidates: Vec<client_library::external_metadata::RichEnrichmentCandidate>) -> Result<usize, BackendError> {
        let mut requested_isbns = candidates.iter().flat_map(|candidate| candidate.isbns.iter().cloned()).collect::<Vec<_>>();
        requested_isbns.sort_unstable();
        requested_isbns.dedup();
        let mut results = HashMap::new();
        let mut place_index = HashMap::new();
        let mut person_index = HashMap::new();
        let batches = requested_isbns.chunks(ISBN_LOOKUP_BATCH_SIZE.min(metadata_contract::MAX_ISBNS_PER_REQUEST)).map(<[_]>::to_vec).collect::<Vec<_>>();
        let mut requests =
            stream::iter(batches.into_iter().map(|isbns| async move { self.cached_metadata_enrichment(&MetadataEnrichmentRequest { isbns: isbns.to_vec() }).await }).collect::<Vec<_>>()).buffer_unordered(enrichment_parallelism().1);
        while let Some(outcome) = requests.next().await {
            let response = match outcome {
                Ok(response) => response,
                Err(error) => {
                    let failures = candidates.iter().map(|candidate| (candidate.content_hash.clone(), candidate.isbns.join(","))).collect::<Vec<_>>();
                    record_enrichment_failures(self, OPEN_LIBRARY_RICH_PROVIDER, failures, error.to_string()).await?;
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
        let mut writes: Vec<EnrichmentWrite> = Vec::new();
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
                        if candidate.reference_isbns.contains(isbn) && !candidate.exact_isbns.contains(isbn) {
                            matches = vec![publication_enrichment::related_isbn::classification_record(publication_enrichment::related_isbn::reference_classifications(isbn, matches))];
                        } else if !candidate.exact_isbns.contains(isbn) {
                            alternative_classification_consensus.record_alternative_matches(&mut matches, isbn);
                        }
                        matched_records.extend(matches);
                    }
                    Some(_) => {}
                }
            }
            if omitted {
                writes.push(Box::new(move |conn| {
                    prepared::record_external_metadata_attempt(conn, &candidate.content_hash, OPEN_LIBRARY_RICH_PROVIDER, &candidate.isbns.join(","), "failed", Some("metadata response omitted one or more requested ISBNs"))?;
                    Ok(0)
                }));
                continue;
            }
            if candidate.isbns.is_empty() {
                writes.push(Box::new(move |conn| {
                    prepared::record_external_metadata_attempt(conn, &candidate.content_hash, OPEN_LIBRARY_RICH_PROVIDER, "", "no_match", Some("no verified print ISBN classification evidence"))?;
                    Ok(0)
                }));
                continue;
            }
            let available = candidate.isbns.iter().filter_map(|isbn| results.get(isbn)).collect::<Vec<_>>();
            let descriptions = available.iter().flat_map(|r| &r.matches).filter_map(|m| m.description.as_deref()).map(str::trim).filter(|d| !d.is_empty()).collect::<BTreeSet<_>>();
            let (description_status, description) = match descriptions.len() {
                0 => ("no_match", None),
                1 => ("updated", descriptions.into_iter().next().map(str::to_owned)),
                _ => ("no_match", None),
            };
            let hash = candidate.content_hash;
            let ids = candidate.isbns.join(",");
            writes.push(Box::new(move |conn| prepared::apply_description(conn, hash, DESCRIPTION_PROVIDER, &ids, description.as_deref(), description_status)));
            let authoritative = available.iter().filter_map(|result| authoritative_author_resolutions(result)).collect::<Vec<_>>();
            if let Some(authors) = authoritative.first() {
                let ids = candidate.isbns.join(",");
                if authoritative.iter().any(|other| other != authors) {
                    writes.push(Box::new(move |conn| prepared::apply_author_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_AUTHOR_PROVIDER, &ids, &[], "no_match", Some("inspected ISBN records disagree on author credits"))));
                } else {
                    let authors = authors.clone();
                    writes.push(Box::new(move |conn| {
                        let already_applied: bool = conn.query_row(include_str!("sql/rich_metadata/apply_rich_metadata_candidates_select.sql"), rusqlite::params![hash.as_str(), OPEN_LIBRARY_AUTHOR_PROVIDER, &ids], |r| r.get(0))?;
                        if already_applied {
                            return Ok(0);
                        }
                        prepared::apply_authoritative_authors_in_transaction(conn, hash, OPEN_LIBRARY_AUTHOR_PROVIDER, &ids, &authors)
                    }));
                }
            }
            let global_alternative_classifications = alternative_classification_consensus.classifications();
            let original_has_classifications = !global_alternative_classifications.is_empty() || matched_records.iter().any(|matched| !matched.classifications.is_empty());
            let identifier = candidate.isbns.join(",");
            let (status, matched, places, people, subjects, detail, replace_previous_work_classifications) = if matched_records.is_empty() {
                if omitted {
                    ("failed", None, Vec::new(), Vec::new(), Vec::new(), Some("metadata response omitted one or more requested ISBNs".to_owned()), false)
                } else {
                    ("no_match", None, Vec::new(), Vec::new(), Vec::new(), Some(format!("checked {} ISBNs", candidate.isbns.len())), false)
                }
            } else {
                let work_count = matched_records.iter().filter_map(|matched| matched.open_library_work_id.as_deref()).collect::<BTreeSet<_>>().len();
                let mut matched = merge_rich_matches(&matched_records);
                matched.classifications.extend(global_alternative_classifications);
                matched.classifications.sort();
                matched.classifications.dedup();
                let merged_direct_classification_count = matched.classifications.len();
                let direct_consensus_replacement = original_has_classifications && candidate.classification_count > MAX_DIRECT_CLASSIFICATIONS_WITHOUT_CONSENSUS && merged_direct_classification_count < candidate.classification_count;
                let (places, people) = entities_for_match(&matched, &place_index, &person_index);
                let subjects = rich_subjects(&matched).map_err(BackendError::operation)?;
                let fallback_detail =
                    if direct_consensus_replacement { format!("; replaced the previous oversized work union with {merged_direct_classification_count} dump-native edition-consensus classifications") } else { String::new() };
                let detail = Some(format!("checked {} ISBNs; {matched_isbn_count} matched across {work_count} Open Library work records; merged all matches{fallback_detail}", candidate.isbns.len()));
                ("updated", Some(matched), places, people, subjects, detail, direct_consensus_replacement)
            };
            writes.push(Box::new(move |conn| {
                prepared::apply_rich_enrichment_in_transaction(
                    conn,
                    candidate.content_hash,
                    OPEN_LIBRARY_RICH_PROVIDER,
                    &identifier,
                    matched.as_ref(),
                    &places,
                    &people,
                    subjects,
                    status,
                    detail.as_deref(),
                    replace_previous_work_classifications,
                )?;
                Ok(usize::from(status == "updated"))
            }));
            crate::executor::sleep(std::time::Duration::from_millis(1)).await;
        }
        apply_enrichment_writes(self, writes).await
    }
}

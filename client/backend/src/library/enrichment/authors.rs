//! Author identity and credit enrichment.
use super::OPEN_LIBRARY_AUTHOR_PROVIDER;
use super::application::{EnrichmentWrite, apply_enrichment_writes, record_enrichment_failures};
use crate::BackendError;
use crate::library::LibrarySession;
use client_library::prepared_enrichment as prepared;
use metadata_contract::{LookupStatus, MetadataEnrichmentRequest};
use publication_enrichment::resolution::{authoritative_author_resolutions, match_author_resolutions, merge_rich_matches};
use std::collections::{BTreeSet, HashMap};

impl LibrarySession {
    /// Resolves scanned author credits through the public catalogue metadata
    /// endpoint. Requests contain ISBNs only; library ids, titles, paths, and
    /// credited names remain on the device.
    pub(super) async fn apply_author_candidates(&self, candidates: Vec<client_library::external_metadata::AuthorEnrichmentCandidate>) -> Result<usize, BackendError> {
        let mut updated = 0_usize;
        for batch in candidates.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
            let mut isbns = batch.iter().flat_map(|candidate| candidate.isbns.iter().cloned()).collect::<Vec<_>>();
            isbns.sort_unstable();
            isbns.dedup();
            let mut results = HashMap::new();
            for isbns in isbns.chunks(metadata_contract::MAX_ISBNS_PER_REQUEST) {
                let response = match self.cached_metadata_enrichment(&MetadataEnrichmentRequest { isbns: isbns.to_vec() }).await {
                    Ok(response) => response,
                    Err(error) => {
                        let failures = batch.iter().map(|candidate| (candidate.content_hash, candidate.isbns.join(","))).collect();
                        record_enrichment_failures(self, OPEN_LIBRARY_AUTHOR_PROVIDER, failures, error.to_string()).await?;
                        return Err(error);
                    }
                };
                for result in response.results {
                    results.insert(result.requested_isbn.clone(), result);
                }
            }
            let mut writes: Vec<EnrichmentWrite> = Vec::new();
            for candidate in batch {
                let available = candidate.isbns.iter().filter_map(|isbn| results.get(isbn)).collect::<Vec<_>>();
                if available.len() != candidate.isbns.len() {
                    let hash = candidate.content_hash;
                    let isbn = candidate.isbns.join(",");
                    writes.push(Box::new(move |conn| prepared::apply_author_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_AUTHOR_PROVIDER, &isbn, &[], "failed", Some("metadata response omitted an ISBN"))));
                    continue;
                }
                let authoritative = available.iter().filter_map(|result| authoritative_author_resolutions(result).map(|authors| (*result, authors))).collect::<Vec<_>>();
                if let Some((_result, authors)) = authoritative.first() {
                    let hash = candidate.content_hash;
                    let isbn = candidate.isbns.join(",");
                    if authoritative.iter().any(|(_, other)| other != authors) {
                        writes.push(Box::new(move |conn| prepared::apply_author_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_AUTHOR_PROVIDER, &isbn, &[], "no_match", Some("inspected ISBN records disagree on author credits"))));
                    } else {
                        let authors = authors.clone();
                        writes.push(Box::new(move |conn| prepared::apply_authoritative_authors_in_transaction(conn, hash, OPEN_LIBRARY_AUTHOR_PROVIDER, &isbn, &authors)));
                    }
                    continue;
                }
                let result = available.iter().copied().find(|r| !r.matches.is_empty()).or_else(|| available.first().copied());
                let (status, resolutions, detail) = match result {
                    None => ("failed", Vec::new(), Some("metadata response omitted the requested ISBN".to_owned())),
                    Some(result) if result.status == LookupStatus::NoMatch || result.status == LookupStatus::InvalidIsbn => ("no_match", Vec::new(), None),
                    Some(result) if result.matches.is_empty() => ("no_match", Vec::new(), Some("ISBN returned no Open Library work metadata".to_owned())),
                    Some(result) => {
                        let work_count = result.matches.iter().filter_map(|matched| matched.open_library_work_id.as_deref()).collect::<BTreeSet<_>>().len();
                        let merged = merge_rich_matches(&result.matches);
                        let (status, resolutions, detail) = match_author_resolutions(&candidate.authors.iter().map(|credit| (credit.position, credit.name.as_str())).collect::<Vec<_>>(), &merged.authors);
                        let detail = detail.or_else(|| (work_count > 1).then(|| format!("merged author evidence from {work_count} Open Library work records")));
                        (status, resolutions, detail)
                    }
                };
                let hash = candidate.content_hash.clone();
                let isbn = candidate.isbns.join(",");
                writes.push(Box::new(move |conn| prepared::apply_author_enrichment_in_transaction(conn, hash, OPEN_LIBRARY_AUTHOR_PROVIDER, &isbn, &resolutions, status, detail.as_deref())));
                crate::executor::sleep(std::time::Duration::from_millis(1)).await;
            }
            updated += apply_enrichment_writes(self, writes).await?;
        }
        Ok(updated)
    }
}

//! Asks the metadata server about books whose file facts it hasn't answered.

use std::collections::HashSet;

use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentStatus, BookFacts, MAX_BOOKS_PER_REQUEST};
use sync_common::BookId;

use crate::{EnrichmentResult, LibraryEnrichment};

pub(crate) fn now() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn failed(key: String, detail: String) -> BookEnrichment {
    BookEnrichment { key, status: BookEnrichmentStatus::Failed, record: None, toc: None, popularity: None, cover: None, failure: None, detail }
}

impl LibraryEnrichment {
    /// Sends the facts of every unanswered book in `scope`, stores the
    /// answers, then fills missing covers. Returns how many books changed.
    pub(crate) async fn enrich_books(&self, scope: &HashSet<BookId>) -> EnrichmentResult<usize> {
        let members = scope.iter().copied().collect::<Vec<_>>();
        let mut candidates = self.database.enrichment_candidates(&members, now())?.into_iter();
        let mut changed = 0;
        loop {
            // Keys are batch positions: no library identifier leaves the device.
            let (books, targets): (Vec<_>, Vec<_>) = candidates
                .by_ref()
                .take(MAX_BOOKS_PER_REQUEST)
                .enumerate()
                .map(|(index, candidate)| {
                    let readings = book_inspection::title_readings(&candidate.record, &candidate.filenames);
                    let facts = BookFacts { key: index.to_string(), record: candidate.record, audiobook: candidate.audiobook, needs_cover: false, readings, recording: candidate.recording };
                    (facts, (candidate.book_id, candidate.request_key))
                })
                .unzip();
            if books.is_empty() {
                break;
            }
            let hashes = targets.iter().map(|(hash, _)| *hash).collect::<Vec<_>>();
            match self.client.books(&BookEnrichmentRequest { books }).await {
                Ok(response) => {
                    let mut answered = vec![false; targets.len()];
                    let mut results = response.results.into_iter();
                    let mut pending = targets.iter().enumerate();
                    let answers = std::iter::from_fn(|| {
                        for result in results.by_ref() {
                            let Ok(index) = result.key.parse::<usize>() else { continue };
                            let Some((book_id, request_key)) = targets.get(index) else { continue };
                            // Match the exact key and keep the first answer, as before.
                            if answered[index] || result.key != index.to_string() {
                                continue;
                            }
                            if result.status != BookEnrichmentStatus::Failed {
                                changed += 1;
                            }
                            answered[index] = true;
                            return Some((book_id, request_key.as_str(), result));
                        }
                        for (index, (book_id, request_key)) in pending.by_ref() {
                            if !answered[index] {
                                return Some((book_id, request_key.as_str(), failed(index.to_string(), "metadata response omitted the book".to_owned())));
                            }
                        }
                        None
                    });
                    self.database.record_enrichments(&hashes, answers, now())?;
                    self.events.publish_update(library_model::LibraryUpdate::Contents);
                }
                Err(error) => {
                    log::warn!("metadata server lookup will retry: {error}");
                    let answers = targets.iter().enumerate().map(|(index, (book_id, request_key))| (book_id, request_key.as_str(), failed(index.to_string(), error.to_string())));
                    self.database.record_enrichments(&hashes, answers, now())?;
                }
            }
        }
        changed += self.fill_covers(&members).await?;
        Ok(changed)
    }
}

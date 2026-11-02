//! Asks the metadata server about books whose file facts it hasn't answered.

use std::collections::HashSet;

use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentStatus, BookFacts, MAX_BOOKS_PER_REQUEST};
use sync_common::BookId;

use crate::{EnrichmentResult, LibraryEnrichment};

pub(crate) fn now() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn failed(key: String, detail: String) -> BookEnrichment {
    BookEnrichment { key, status: BookEnrichmentStatus::Failed, record: None, toc: None, popularity: None, cover: None, detail }
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
                    (facts, (candidate.content_hash, candidate.request_key))
                })
                .unzip();
            if books.is_empty() {
                break;
            }
            let mut response = self.client.books(&BookEnrichmentRequest { books }).await;
            if let Err(error) = &response {
                log::warn!("metadata server lookup will retry: {error}");
            }
            for (index, (content_hash, request_key)) in targets.iter().enumerate() {
                let key = index.to_string();
                let result = match &mut response {
                    Ok(response) => response.results.iter().position(|result| result.key == key).map(|index| response.results.remove(index)).unwrap_or_else(|| failed(key, "metadata response omitted the book".to_owned())),
                    Err(error) => failed(key, error.to_string()),
                };
                if result.status != BookEnrichmentStatus::Failed {
                    changed += 1;
                }
                self.database.record_enrichment(content_hash, request_key, result, now())?;
            }
            if response.is_ok() {
                self.events.publish_update(library_model::LibraryUpdate::Contents);
            }
        }
        changed += self.fill_covers(&members).await?;
        Ok(changed)
    }
}

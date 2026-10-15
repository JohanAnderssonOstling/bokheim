//! Asks the metadata server about books whose file facts it hasn't answered.

use std::collections::HashSet;

use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentStatus, BookFacts, MAX_BOOKS_PER_REQUEST};
use sync_common::ContentHash;

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
    pub(crate) async fn enrich_books(&self, scope: &HashSet<ContentHash>) -> EnrichmentResult<usize> {
        let members = scope.iter().copied().collect::<Vec<_>>();
        let candidates = self.database.enrichment_candidates(&members, now())?;
        let mut changed = 0;
        for chunk in candidates.chunks(MAX_BOOKS_PER_REQUEST) {
            // Keys are batch positions: no library identifier leaves the device.
            let books = chunk
                .iter()
                .enumerate()
                .map(|(index, candidate)| BookFacts {
                    key: index.to_string(),
                    record: candidate.record.clone(),
                    audiobook: candidate.audiobook,
                    needs_cover: false,
                    readings: book_inspection::title_readings(&candidate.record, &candidate.filenames),
                    recording: candidate.recording.clone(),
                })
                .collect();
            let response = self.client.books(&BookEnrichmentRequest { books }).await;
            if let Err(error) = &response {
                log::warn!("metadata server lookup will retry: {error}");
            }
            for (index, candidate) in chunk.iter().enumerate() {
                let key = index.to_string();
                let result = match &response {
                    Ok(response) => response.results.iter().find(|result| result.key == key).cloned().unwrap_or_else(|| failed(key, "metadata response omitted the book".to_owned())),
                    Err(error) => failed(key, error.to_string()),
                };
                if result.status != BookEnrichmentStatus::Failed {
                    changed += 1;
                }
                self.database.record_enrichment(&candidate.content_hash, &candidate.request_key, &result, now())?;
            }
            if response.is_ok() {
                self.events.publish_update(library_model::LibraryUpdate::Contents);
            }
        }
        changed += self.fill_covers(&members).await?;
        Ok(changed)
    }
}

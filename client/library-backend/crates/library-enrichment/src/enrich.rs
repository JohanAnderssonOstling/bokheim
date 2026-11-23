//! Asks the metadata server about books whose file facts it hasn't answered.

use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentStatus, BookFacts, MAX_BOOKS_PER_REQUEST};

use crate::{EnrichmentResult, LibraryEnrichment};

pub(crate) fn now() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn failed(key: String, detail: String) -> BookEnrichment {
    BookEnrichment { key, status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, failure: None, detail }
}

impl LibraryEnrichment {
    /// Publish actual metadata changes; failed attempts only schedule local retries.
    pub(crate) async fn enrich_books(&self) -> EnrichmentResult<()> {
        let mut after = None;
        loop {
            let (candidates, next) = self.database.enrichment_candidates(now(), after.as_ref()).await?;
            let Some(next) = next else { break };
            after = Some(next);
            let mut candidates = candidates.into_iter();
            loop {
                // Keys are batch positions: no library identifier leaves the device.
                let (books, targets): (Vec<_>, Vec<_>) = candidates
                    .by_ref()
                    .take(MAX_BOOKS_PER_REQUEST)
                    .enumerate()
                    .map(|(index, candidate)| {
                        let readings = book_normalization::title_readings(&candidate.record, &candidate.filenames);
                        let facts = BookFacts { key: index.to_string(), record: candidate.record, audiobook: candidate.audiobook, needs_cover: false, readings, recording: candidate.recording };
                        (facts, (candidate.book_id, candidate.request_key))
                    })
                    .unzip();
                if books.is_empty() {
                    break;
                }
                let changed = match self.client.books(&BookEnrichmentRequest { books }).await {
                    Ok(response) => {
                        let mut answered = vec![false; targets.len()];
                        let mut answers = Vec::with_capacity(targets.len());
                        for result in response.results {
                            let Ok(index) = result.key.parse::<usize>() else { continue };
                            let Some((book_id, request_key)) = targets.get(index) else { continue };
                            // Match the exact key and keep the first answer.
                            if answered[index] || result.key != index.to_string() {
                                continue;
                            }
                            if result.failure.is_some() || result.status == BookEnrichmentStatus::Failed {
                                log::warn!("metadata lookup for {book_id} completed with {:?}: {}", result.failure, result.detail);
                            }
                            answered[index] = true;
                            answers.push((*book_id, request_key.clone(), result));
                        }
                        for (index, (book_id, request_key)) in targets.iter().enumerate() {
                            if !answered[index] {
                                log::warn!("metadata response omitted {book_id}; optional enrichment will retry");
                                answers.push((*book_id, request_key.clone(), failed(index.to_string(), "metadata response omitted the book".to_owned())));
                            }
                        }
                        self.database.record_enrichments(answers, now()).await?
                    }
                    Err(error) => {
                        log::warn!("metadata server lookup failed; optional enrichment will retry: {error}");
                        let answers = targets.iter().enumerate().map(|(index, (book_id, request_key))| (*book_id, request_key.clone(), failed(index.to_string(), error.to_string())));
                        self.database.record_enrichments(answers.collect(), now()).await?
                    }
                };
                if changed {
                    self.events.sync.wake();
                    self.events.refresh_contents();
                }
            }
        }
        Ok(())
    }
}

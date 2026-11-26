//! Asks the metadata server about books whose file facts it hasn't answered.

use futures_util::{
    future::{self, Either},
    pin_mut,
    stream::FuturesUnordered,
    StreamExt,
};
use library_database::EnrichmentCandidate;
use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentStatus, BookFacts, MAX_BOOKS_PER_REQUEST};
use std::collections::HashSet;

use crate::{EnrichmentResult, LibraryEnrichment};

pub(crate) fn now() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn failed(key: String, detail: String) -> BookEnrichment {
    BookEnrichment { key, status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, failure: None, detail }
}

impl LibraryEnrichment {
    /// Publish each completed request without waiting for unrelated lookups.
    pub(crate) async fn enrich_books(&self) -> EnrichmentResult<()> {
        let mut changes = self.events.enrichment.subscribe();
        let mut submitted = HashSet::new();
        let mut requests = FuturesUnordered::new();
        let mut refresh = true;
        loop {
            if refresh {
                changes.borrow_and_update();
                let excluded = submitted.iter().copied().collect::<Vec<_>>();
                let mut after = None;
                loop {
                    let (candidates, next) = self.database.enrichment_candidates_excluding(now(), after.as_ref(), &excluded).await?;
                    let Some(next) = next else { break };
                    after = Some(next);
                    submitted.extend(candidates.iter().map(|candidate| candidate.book_id));
                    let (audio, books): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|candidate| candidate.audiobook);
                    let mut books = books.into_iter();
                    while books.len() > 0 {
                        requests.push(self.enrich_batch(books.by_ref().take(MAX_BOOKS_PER_REQUEST).collect()));
                    }
                    // A slow recording returns independently of other books.
                    requests.extend(audio.into_iter().map(|book| self.enrich_batch(vec![book])));
                }
                refresh = false;
            }
            if requests.is_empty() {
                return Ok(());
            }
            // The server owns lookup capacity. Scan and thumbnail wakes can
            // submit new books while existing provider requests are waiting.
            let changed = changes.changed();
            pin_mut!(changed);
            match future::select(requests.next(), changed).await {
                Either::Left((Some(result), _)) => result?,
                Either::Left((None, _)) => return Ok(()),
                Either::Right((Ok(()), _)) => refresh = true,
                Either::Right((Err(_), _)) => return Ok(()),
            }
        }
    }

    async fn enrich_batch(&self, candidates: Vec<EnrichmentCandidate>) -> EnrichmentResult<()> {
        // Keys are batch positions: no library identifier leaves the device.
        let (books, targets): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let readings = book_normalization::title_readings(&candidate.record, &candidate.filenames);
                let facts = BookFacts { key: index.to_string(), record: candidate.record, needs_description: candidate.needs_description, audiobook: candidate.audiobook, needs_cover: false, readings, recording: candidate.recording };
                (facts, (candidate.book_id, candidate.request_key))
            })
            .unzip();
        let changed = match self.client.books(&BookEnrichmentRequest { books }).await {
            Ok(response) => {
                let mut answered = vec![false; targets.len()];
                let mut answers = Vec::with_capacity(targets.len());
                for result in response.results {
                    let Ok(index) = result.key.parse::<usize>() else { continue };
                    let Some((book_id, request_key)) = targets.get(index) else { continue };
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
        Ok(())
    }
}

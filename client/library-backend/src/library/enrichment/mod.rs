//! Asks the metadata server about books whose file facts it hasn't answered.
//! The server owns every lookup and merge rule; this only sends facts and
//! stores answers.
pub(crate) use crate::{library::LibrarySession, BackendError, ContentHash};
pub(crate) use metadata_contract::books::{BookEnrichment, BookEnrichmentRequest, BookEnrichmentStatus, MAX_BOOKS_PER_REQUEST};

pub(crate) mod covers;

pub(crate) fn now() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

pub(crate) fn failed(key: String, detail: String) -> BookEnrichment {
    BookEnrichment { key, status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, detail }
}

impl LibrarySession {
    /// Sends the facts of every unanswered book in `scope`, stores the
    /// answers, then fills missing covers. Returns how many books changed.
    pub(crate) async fn enrich_books(&self, scope: &std::collections::HashSet<ContentHash>) -> Result<usize, BackendError> {
        let members = scope.iter().copied().collect::<Vec<_>>();
        let candidates = self.db.enrichment_candidates(&members, now()).map_err(BackendError::operation)?;
        let mut found = 0;
        for chunk in candidates.chunks(MAX_BOOKS_PER_REQUEST) {
            // Keys are batch positions: no library identifier leaves the device.
            let books = chunk.iter().enumerate().map(|(index, candidate)| metadata_contract::books::BookFacts { key: index.to_string(), ..candidate.facts.clone() }).collect();
            let response = self.metadata.books(&BookEnrichmentRequest { books }).await;
            if let Err(error) = &response {
                log::warn!("metadata server lookup will retry: {error}");
            }
            for (index, candidate) in chunk.iter().enumerate() {
                let key = index.to_string();
                let result = match &response {
                    Ok(response) => response.results.iter().find(|result| result.key == key).cloned().unwrap_or_else(|| failed(key, "metadata response omitted the book".to_owned())),
                    Err(error) => failed(key, error.to_string()),
                };
                if result.status == BookEnrichmentStatus::Found {
                    found += 1;
                }
                self.db.record_enrichment(&candidate.content_hash, &candidate.request_key, &result, now()).map_err(BackendError::operation)?;
            }
            if response.is_ok() {
                self.subscriptions.publish_update(library_model::LibraryUpdate::Contents);
            }
        }
        found += self.fill_covers(&members).await?;
        Ok(found)
    }
}

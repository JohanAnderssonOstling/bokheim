//! Applies prepared enrichment results in bounded, conditional transactions.
use crate::{BackendError, ContentHash, library::LibrarySession};
use client_library::prepared_enrichment as prepared;

// Match the existing enrichment page size; network requests never own storage.
pub(super) const ENRICHMENT_WRITE_BATCH_SIZE: usize = 32;
/// Temporary internal collection point while each enrichment producer is
/// converted to return its own domain result data. It must not cross the
/// library-database boundary.
pub(super) type EnrichmentWrite = Box<dyn FnOnce(&mut prepared::Preparation<'_>) -> rusqlite::Result<usize> + Send>;

pub(super) async fn apply_enrichment_writes(library: &LibrarySession, writes: Vec<EnrichmentWrite>) -> Result<usize, BackendError> {
    let mut writes = writes.into_iter();
    let mut updated = 0;
    loop {
        let batch = writes.by_ref().take(ENRICHMENT_WRITE_BATCH_SIZE).collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        let _ = batch;
        return Err(BackendError::message("enrichment producers must return typed result data before persistence"));
        crate::executor::sleep(std::time::Duration::from_millis(1)).await;
    }
    Ok(updated)
}

pub(super) async fn record_enrichment_failures(library: &LibrarySession, provider: &'static str, failures: Vec<(ContentHash, String)>, detail: String) -> Result<(), BackendError> {
    let failures = failures
        .into_iter()
        .map(|(content_hash, identifier)| library_database::EnrichmentFailure {
            content_hash,
            provider: provider.to_owned(),
            identifier,
            detail: detail.clone(),
        })
        .collect::<Vec<_>>();
    library.db.record_enrichment_failures(failures).map_err(BackendError::operation)
}

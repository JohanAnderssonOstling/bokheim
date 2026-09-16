//! Applies prepared enrichment results in bounded, conditional transactions.
use crate::{BackendError, ContentHash, library::LibrarySession};
use client_library::prepared_enrichment as prepared;
use library_runtime::database::DatabaseError;

// Match the existing enrichment page size; network requests never own storage.
pub(super) const ENRICHMENT_WRITE_BATCH_SIZE: usize = 32;
pub(super) type EnrichmentWrite = Box<dyn FnOnce(&mut prepared::Preparation<'_>) -> rusqlite::Result<usize> + Send>;

pub(super) async fn apply_enrichment_writes(self: &LibrarySession, writes: Vec<EnrichmentWrite>) -> Result<usize, BackendError> {
    let mut writes = writes.into_iter();
    let mut updated = 0;
    loop {
        let batch = writes.by_ref().take(ENRICHMENT_WRITE_BATCH_SIZE).collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        // Preparation owns only a consistent read snapshot. Release it before
        // queueing the short conditional write; never upgrade a stale reader.
        let preparation_database = self.db.clone();
        let prepared = crate::executor::run_blocking(move || {
            let mut conn = preparation_database.get()?;
            let read = conn.transaction().map_err(DatabaseError::operation)?;
            let mut preparation = prepared::Preparation::new(&read);
            for write in batch {
                preparation.push(write).map_err(DatabaseError::operation)?;
            }
            let prepared = preparation.finish().map_err(DatabaseError::operation)?;
            read.rollback().map_err(DatabaseError::operation)?;
            Ok::<_, DatabaseError>(prepared)
        })
        .await
        .map_err(BackendError::operation)?
        .map_err(BackendError::operation)?;
        updated += self.db.command(crate::storage::storage_queue::Priority::Background, move |conn| prepared.apply(conn).map_err(DatabaseError::operation)).await.map_err(BackendError::operation)?;
        crate::executor::sleep(std::time::Duration::from_millis(1)).await;
    }
    Ok(updated)
}

pub(super) async fn record_enrichment_failures(self: &LibrarySession, provider: &'static str, failures: Vec<(ContentHash, String)>, detail: String) -> Result<(), BackendError> {
    let writes = failures
        .into_iter()
        .map(|(hash, identifier)| {
            let detail = detail.clone();
            Box::new(move |conn: &mut prepared::Preparation<'_>| {
                prepared::record_external_metadata_attempt(conn, &hash, provider, &identifier, "failed", Some(&detail))?;
                Ok(0usize)
            }) as EnrichmentWrite
        })
        .collect();
    apply_enrichment_writes(self, writes).await.map(|_| ())
}

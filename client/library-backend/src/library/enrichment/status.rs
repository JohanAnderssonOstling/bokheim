//! Read-only enrichment status for application views.
use super::{OPEN_LIBRARY_AUTHOR_PROVIDER, OPEN_LIBRARY_COVER_PROVIDER, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, OPEN_LIBRARY_RICH_PROVIDER};
use crate::BackendError;
use crate::library::LibrarySession;

// Read only the attempt ledger; opening Settings must never trigger lookups.
impl LibrarySession {
    pub(crate) async fn enrichment_status(&self) -> Result<crate::EnrichmentStatus, BackendError> {
        let counts = self
            .db
            .enrichment_status_counts([
                OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER,
                OPEN_LIBRARY_RICH_PROVIDER,
                OPEN_LIBRARY_AUTHOR_PROVIDER,
                OPEN_LIBRARY_COVER_PROVIDER,
            ])
            .map_err(BackendError::operation)?;
        Ok(crate::EnrichmentStatus {
            running: false,
            total: counts.total,
            enriched: counts.enriched,
            pending: counts.pending,
            no_match: counts.no_match,
            failed: counts.failed,
        })
    }
}

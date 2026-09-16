//! Read-only enrichment status for application views.
use super::{OPEN_LIBRARY_AUTHOR_PROVIDER, OPEN_LIBRARY_COVER_PROVIDER, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, OPEN_LIBRARY_RICH_PROVIDER};
use crate::BackendError;
use crate::library::LibrarySession;
use library_runtime::database::DatabaseError;

// Read only the attempt ledger; opening Settings must never trigger lookups.
impl LibrarySession {
    pub(crate) async fn enrichment_status(&self) -> Result<crate::EnrichmentStatus, BackendError> {
        self.db.command(crate::storage::storage_queue::Priority::Background, |conn| read_enrichment_status(conn).map_err(DatabaseError::operation)).await.map_err(BackendError::operation)
    }
}

pub(super) fn read_enrichment_status(conn: &rusqlite::Connection) -> rusqlite::Result<crate::EnrichmentStatus> {
    // A book appears once even if several providers have updated it. Failures
    // and unmatched results are not successful enrichment. Ignore hidden/trash.
    conn.query_row(include_str!("sql/status/read_enrichment_status_with.sql"), rusqlite::params![OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, OPEN_LIBRARY_RICH_PROVIDER, OPEN_LIBRARY_AUTHOR_PROVIDER, OPEN_LIBRARY_COVER_PROVIDER], |row| {
        Ok(crate::EnrichmentStatus {
            running: false,
            total: row.get::<_, i64>(0)? as u64,
            enriched: row.get::<_, i64>(1)? as u64,
            pending: row.get::<_, i64>(2)? as u64,
            no_match: row.get::<_, i64>(3)? as u64,
            failed: row.get::<_, i64>(4)? as u64,
        })
    })
}

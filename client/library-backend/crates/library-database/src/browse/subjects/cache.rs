//! Durable subject routes and membership, incrementally refreshed under the main writer.
use super::*;
use crate::transactions::WriteOutcome;

fn cache_state(conn: &rusqlite::Connection) -> Result<(bool, bool), DatabaseError> {
    let mut state = (false, false);
    conn.subject_cache_status(subject_projection::BUNDLED_UNIFIED_TAXONOMY_REVISION_ID, |row| {
        state = (row.get(0)?, row.get(1)?);
        Ok(())
    })?;
    Ok(state)
}

impl Database {
    /// Keep card reads and membership reads on the same clean cache snapshot.
    /// If another writer changes membership before the snapshot starts, refresh
    /// and retry; once it starts, WAL readers do not hold the main writer.
    pub(in crate::browse) fn with_subject_cache<T>(&self, operation: impl FnOnce() -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        self.with_cached_browse_snapshot(|| self.ensure_subject_cache(), |connection| Ok(cache_state(connection)? == (false, false)), operation)
    }

    pub(super) fn ensure_subject_cache(&self) -> Result<(), DatabaseError> {
        if cache_state(&self.connection)? == (false, false) {
            return Ok(());
        }
        self.with_write_transaction(|tx| {
            let (rebuild, dirty) = cache_state(tx)?;
            if !rebuild && !dirty {
                return Ok(WriteOutcome::Commit(()));
            }
            if rebuild {
                tx.subject_cache_rebuild()?;
            }
            let mut books = Vec::new();
            tx.subject_cache_dirty_books(|row| {
                books.push(row.get::<_, i64>(0)?);
                Ok(())
            })?;
            for book in books {
                tx.subject_cache_remove_book(book)?;
                let mut routes = Vec::new();
                tx.book_routes(book, |row| {
                    routes.push((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?));
                    Ok(())
                })?;
                if routes.is_empty() {
                    continue;
                }
                for (route, direct) in &routes {
                    tx.subject_cache_insert_route(*route)?;
                    tx.subject_cache_insert_member(*route, book, *direct)?;
                }
                // Route zero represents the synthetic Subject root.
                tx.subject_cache_insert_route(0)?;
                tx.subject_cache_insert_member(0, book, 0)?;
            }
            tx.subject_cache_prune()?;
            tx.subject_cache_clean()?;
            tx.subject_cache_revision(subject_projection::BUNDLED_UNIFIED_TAXONOMY_REVISION_ID)?;
            Ok(WriteOutcome::Commit(()))
        })
    }
}

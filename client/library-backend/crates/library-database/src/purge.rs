//! Disposable directory and book projections shared by local and remote purges.
pub(crate) use crate::DatabaseError;
use crate::SqlParam;
use crate::query_methods::query_rows;
pub(crate) use crate::shared_sql::SharedSql;
use content_address::BookId;

/// Remove the disposable book row while keeping sync intent and retryable file
/// cleanup independent of its lifetime. Caller uses remote origin.
pub(crate) fn remove_book(tx: &rusqlite::Transaction<'_>, hash: &BookId) -> Result<(), DatabaseError> {
    // Capture materialized and logical paths before cascading placements away.
    tx.purge_queue_observed_files(&hash)?;
    let placements = query_rows(|callback| tx.purge_book_placements(&hash, callback), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut folders = crate::placements::FolderPaths::new(tx);
    for (dir_id, file_name) in placements {
        if let Some(path) = folders.placement(&dir_id, &file_name)? {
            tx.purge_queue_placement_file(&hash, &(path.as_str()))?;
        }
    }
    tx.shared_cancel_book_materialization(hash)?;
    // Pending publications and canonical fields belong to this book too.
    // Keep only the lifecycle cell: it carries the purge to offline replicas.
    tx.purge_book_publications(&hash)?;
    tx.purge_book_registers(&hash)?;
    // All owned metadata, placements and device-local ingestion state cascade.
    if tx.purge_delete_book(&hash)? != 0 {
        tx.shared_purge_book_record_insert(hash)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Database, transactions::WriteOutcome};

    #[test]
    fn purge_cleanup_is_scoped_and_rolls_back_if_a_later_delete_fails() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        // This fixture owns derived rows directly; do not enqueue canonical edits.

        for hash in [BookId::new(&"a".repeat(64)), BookId::new(&"b".repeat(64))] {
            db.connection.fixtures_insert_book(&hash).unwrap();
            let id = db.connection.last_insert_rowid();
            db.connection.fixtures_insert_book_language(&id).unwrap();
            db.connection.fixtures_insert_book_subject(&id).unwrap();
        }
        let first: i64 = db.connection.fixtures_read_book_row_id(&(BookId::new(&"a".repeat(64))), |r| r.get(0)).unwrap();
        db.connection.fixtures_create_book_subject().unwrap();
        let failed = db.with_write_transaction(|tx| {
            remove_book(tx, &BookId::new(&"a".repeat(64)))?;
            Ok(WriteOutcome::Commit(()))
        });
        assert!(failed.is_err());
        assert_eq!(db.connection.fixtures_read_book_language(|r| r.get::<_, i64>(0)).unwrap(), 2);
        db.connection.fixtures_drop_TRIGGER().unwrap();
        db.with_write_transaction(|tx| {
            remove_book(tx, &BookId::new(&"a".repeat(64)))?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        for table in ["book_language", "book_subject"] {
            assert_eq!(db.connection.fixtures_subject_row_count(table, Some(first), |r| r.get::<_, i64>(0)).unwrap(), 0);
            assert_eq!(db.connection.fixtures_subject_row_count(table, None, |r| r.get::<_, i64>(0)).unwrap(), 1);
        }
        assert_eq!(db.connection.fixtures_read_book(|r| r.get::<_, i64>(0)).unwrap(), 1, "only the purged row is deleted");
    }
}

include_sqlite_sql::include_sql!("src/sql/purge.sql");
#[cfg(test)]
use crate::sql_fixtures::FixturesSql;

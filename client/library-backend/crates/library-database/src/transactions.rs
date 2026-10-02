//! Transaction boundaries and temporary sync state, private to the database owner.
use crate::sync::SyncApplySql;
use crate::{Database, DatabaseError};

pub(crate) enum WriteOutcome<T> {
    Commit(T),
    Rollback(T),
}

impl Database {
    pub(crate) fn write<T>(&self, operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        self.with_write_transaction(|tx| operation(tx).map(WriteOutcome::Commit))
    }

    /// Reserve only main: BEGIN IMMEDIATE also involves the attached read-only
    /// taxonomy. Errors and panics drop the transaction without committing.
    pub(crate) fn with_write_transaction<T>(&self, operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<WriteOutcome<T>, DatabaseError>) -> Result<T, DatabaseError> {
        let mut timing = crate::timing::PhaseTimer::new("write_transaction");
        let tx = self.connection.unchecked_transaction()?;
        tx.reserve_write_lock()?;
        timing.mark("begin_and_reserve");
        let outcome = operation(&tx);
        timing.mark("body");
        match outcome? {
            WriteOutcome::Commit(value) => {
                commit(tx)?;
                timing.mark("commit");
                Ok(value)
            }
            WriteOutcome::Rollback(value) => {
                tx.rollback()?;
                timing.mark("rollback");
                Ok(value)
            }
        }
    }
}

/// Commit an already-open transaction through the same projection boundary.
/// Callers opening their own transaction must reserve its writer before reads.
pub fn commit(tx: rusqlite::Transaction<'_>) -> Result<(), DatabaseError> {
    crate::projection::project_dirty(&tx)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.execute_batch("CREATE TABLE transaction_probe(value INTEGER);").unwrap();
        db
    }

    #[test]
    fn write_boundary_commits_or_rolls_back_explicit_outcomes_and_errors() {
        let db = database();
        db.with_write_transaction(|tx| {
            tx.execute("INSERT INTO transaction_probe VALUES (1)", [])?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        let outcome = db
            .with_write_transaction(|tx| {
                tx.execute("INSERT INTO transaction_probe VALUES (2)", [])?;
                Ok(WriteOutcome::Rollback("stale"))
            })
            .unwrap();
        assert_eq!(outcome, "stale");
        let failed: Result<(), _> = db.with_write_transaction(|tx| {
            tx.execute("INSERT INTO transaction_probe VALUES (3)", [])?;
            Err(DatabaseError::message("injected error"))
        });
        assert!(failed.is_err());
        assert_eq!(db.connection.query_row("SELECT SUM(value) FROM transaction_probe", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert!(db.connection.is_autocommit());
    }

    #[test]
    fn reservation_blocks_other_main_writers_but_allows_wal_readers() {
        let path = std::env::temp_dir().join(format!("library-writer-{}.sqlite3", uuid::Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        db.connection.execute_batch("CREATE TABLE transaction_probe(value INTEGER);").unwrap();
        let other = rusqlite::Connection::open(&path).unwrap();
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        db.with_write_transaction(|_| {
            assert_eq!(other.query_row("SELECT COUNT(*) FROM transaction_probe", [], |r| r.get::<_, i64>(0))?, 0);
            let error = other.execute("INSERT INTO transaction_probe VALUES (1)", []).unwrap_err();
            assert!(matches!(error, rusqlite::Error::SqliteFailure(ref code, _) if code.code == rusqlite::ErrorCode::DatabaseBusy));
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        other.execute("INSERT INTO transaction_probe VALUES (2)", []).unwrap();
        drop(other);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}

//! Disposable book metadata shared by local and remote purge operations.
use crate::shared_sql::SharedSql;
use crate::{DatabaseError, placements::PlacementsSql};

pub(crate) fn clear_book_metadata(tx: &rusqlite::Transaction<'_>, row_id: i64) -> Result<(), DatabaseError> {
    tx.shared_purge_book_identifier(row_id)?;
    tx.purge_book_language(row_id)?;
    tx.purge_book_contributor(row_id)?;
    tx.purge_book_subject_assignment_evidence(row_id)?;
    tx.purge_book_subject_assignment(row_id)?;
    tx.purge_book_subject(row_id)?;
    tx.purge_external_metadata_attempt(row_id)?;
    tx.purge_book_value(row_id)?;
    tx.purge_annotation(row_id)?;
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
        for hash in ["a".repeat(64), "b".repeat(64)] {
            db.execute("INSERT INTO book(content_hash,title,added_at,format) VALUES (?1,'Book',1,'epub')", [hash]).unwrap();
            let id = db.last_insert_rowid();
            db.execute("INSERT INTO book_language(book_row_id,position,language_tag) VALUES (?1,0,'en')", [id]).unwrap();
            db.execute("INSERT INTO book_subject(book_row_id,position,name,source) VALUES (?1,0,'Subject','test')", [id]).unwrap();
        }
        let first: i64 = db.query_row("SELECT row_id FROM book WHERE content_hash=?1", ["a".repeat(64)], |r| r.get(0)).unwrap();
        db.execute_batch("CREATE TEMP TRIGGER reject_purge BEFORE DELETE ON book_subject BEGIN SELECT RAISE(ABORT,'injected purge failure'); END;").unwrap();
        let failed = db.with_write_transaction(|tx| {
            clear_book_metadata(tx, first)?;
            Ok(WriteOutcome::Commit(()))
        });
        assert!(failed.is_err());
        assert_eq!(db.query_row("SELECT COUNT(*) FROM book_language", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        db.execute_batch("DROP TRIGGER reject_purge;").unwrap();
        db.with_write_transaction(|tx| {
            clear_book_metadata(tx, first)?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        for table in ["book_language", "book_subject"] {
            assert_eq!(db.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE book_row_id=?1"), [first], |r| r.get::<_, i64>(0)).unwrap(), 0);
            assert_eq!(db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        }
        assert_eq!(db.query_row("SELECT COUNT(*) FROM book", [], |r| r.get::<_, i64>(0)).unwrap(), 2, "caller owns lifecycle changes");
    }
}

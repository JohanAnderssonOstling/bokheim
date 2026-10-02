//! Disposable directory and book projections shared by local and remote purges.
pub(crate) use crate::DatabaseError;
pub(crate) use crate::shared_sql::SharedSql;

/// Remove the disposable book row while keeping sync intent and retryable file
/// cleanup independent of its lifetime. Caller uses remote origin.
pub(crate) fn remove_book(tx: &rusqlite::Transaction<'_>, hash: &str) -> Result<(), DatabaseError> {
    // Capture materialized and logical paths before cascading placements away.
    tx.execute(
        "INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path)
         SELECT 'remove',?1,relative_path FROM local_file_projection WHERE content_hash=?1",
        [hash],
    )?;
    let placements = tx
        .prepare("SELECT bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=?1")?
        .query_map([hash], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut folders = crate::placements::FolderPaths::new(tx);
    for (dir_id, file_name) in placements {
        if let Some(path) = folders.placement(&dir_id, &file_name)? {
            tx.execute("INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES('remove',?1,?2)", [hash, path.as_str()])?;
        }
    }
    tx.shared_cancel_book_materialization(hash)?;
    // Pending publications and canonical fields belong to this book too.
    // Keep only the lifecycle cell: it carries the purge to offline replicas.
    tx.execute("DELETE FROM sync_outbox WHERE (state_kind,state_key,state_subkey) IN (SELECT state_kind,state_key,state_subkey FROM sync_state_version WHERE book_key=?1 AND state_kind!='book_lifecycle')", [hash])?;
    tx.execute("DELETE FROM sync_state_version WHERE book_key=?1 AND state_kind!='book_lifecycle'", [hash])?;
    // All owned metadata, placements and device-local ingestion state cascade.
    if tx.execute("DELETE FROM book WHERE content_hash=?1", [hash])? != 0 {
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

        for hash in ["a".repeat(64), "b".repeat(64)] {
            db.connection.execute("INSERT INTO book(content_hash,title,added_at,format) VALUES (?1,'Book',1,1)", [hash]).unwrap();
            let id = db.connection.last_insert_rowid();
            db.connection.execute("INSERT INTO book_language(book_row_id,language) VALUES (?1,'en')", [id]).unwrap();
            db.connection.execute("INSERT INTO book_subject(book_row_id,position,name,source) VALUES (?1,0,'Subject','test')", [id]).unwrap();
        }
        let first: i64 = db.connection.query_row("SELECT row_id FROM book WHERE content_hash=?1", ["a".repeat(64)], |r| r.get(0)).unwrap();
        db.connection.execute_batch("CREATE TEMP TRIGGER reject_purge BEFORE DELETE ON book_subject BEGIN SELECT RAISE(ABORT,'injected purge failure'); END;").unwrap();
        let failed = db.with_write_transaction(|tx| {
            remove_book(tx, &"a".repeat(64))?;
            Ok(WriteOutcome::Commit(()))
        });
        assert!(failed.is_err());
        assert_eq!(db.connection.query_row("SELECT COUNT(*) FROM book_language", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        db.connection.execute_batch("DROP TRIGGER reject_purge;").unwrap();
        db.with_write_transaction(|tx| {
            remove_book(tx, &"a".repeat(64))?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        for table in ["book_language", "book_subject"] {
            assert_eq!(db.connection.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE book_row_id=?1"), [first], |r| r.get::<_, i64>(0)).unwrap(), 0);
            assert_eq!(db.connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        }
        assert_eq!(db.connection.query_row("SELECT COUNT(*) FROM book", [], |r| r.get::<_, i64>(0)).unwrap(), 1, "only the purged row is deleted");
    }
    #[test]
    fn purge_clears_pending_annotation_and_rollback_preserves_its_publication() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = sync_common::ContentHash::new(&"c".repeat(64));
        add(&db, hash.as_str());
        let annotation = book_model::ReaderAnnotation {
            id: "offline-note".into(),
            state: book_model::AnnotationState {
                content_hash: hash,
                detail: book_model::AnnotationDetail {
                    anchor: book_model::AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
                    exact_text: "Text".into(),
                    style: book_model::AnnotationStyle::Highlight,
                    color: "#fff".into(),
                    note: "Note".into(),
                    created_at: 1,
                    toc_ordinal: None,
                    progress: None,
                },
                modified_at: 2,
                deleted: false,
            },
        };
        db.upsert_annotation(&annotation).unwrap();
        db.trash_book(&hash).unwrap();
        let pending = db.sync_publishable_mutations().unwrap();
        assert!(pending.iter().any(|m| matches!(m.state.value, library_replica::LibraryStateEntry::Annotation { .. })));
        db.connection.execute_batch("CREATE TEMP TRIGGER reject_purge BEFORE DELETE ON book BEGIN SELECT RAISE(ABORT,'purge failed'); END;").unwrap();
        assert!(db.purge_book(&hash).is_err());
        assert_eq!(db.sync_publishable_mutations().unwrap(), pending);
        assert_eq!(db.annotations(hash).unwrap().len(), 1);
        db.connection.execute_batch("DROP TRIGGER reject_purge;").unwrap();
        db.purge_book(&hash).unwrap();
        assert!(db.annotations(hash).unwrap().is_empty());
        assert_eq!(db.connection.query_row("SELECT count(*) FROM sync_state_version WHERE book_key=?1 AND state_kind!='book_lifecycle'", [hash.as_str()], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert!(db.sync_publishable_mutations().unwrap().iter().all(|m| matches!(m.state.value, library_replica::LibraryStateEntry::BookLifecycle { .. })));
        add(&db, hash.as_str());
        db.upsert_annotation(&annotation).unwrap();
        db.sync_acknowledge_mutations(&pending.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
        assert!(db.sync_publishable_mutations().unwrap().iter().any(|m| matches!(m.state.value, library_replica::LibraryStateEntry::Annotation { .. })), "old acknowledgements cannot erase readded work");
    }

    fn add(db: &Database, hash: &str) {
        let tx = db.connection.unchecked_transaction().unwrap();
        tx.execute("INSERT INTO book(content_hash,title,format,added_at) VALUES(?1,'Book',1,1)", [hash]).unwrap();
        crate::sync::apply::registers::record_local(&tx, library_replica::LibraryStateEntry::BookLifecycle { content_hash: sync_common::ContentHash::new(hash), value: library_replica::BookLifecycleState::Present }).unwrap();
        crate::sync::apply::commit(tx).unwrap();
    }
}

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
    let placements = query_rows(|callback| tx.purge_book_placements(&hash, callback), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
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
    #[test]
    fn purge_clears_pending_annotation_and_rollback_preserves_its_publication() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = sync_common::BookId::new(&"c".repeat(64));
        add(&db, &hash);
        let tx = db.connection.unchecked_transaction().unwrap();
        crate::sync::apply::registers::record_local(
            &tx,
            library_replica::LibraryStateEntry::BookAdd {
                content_hash: hash,
                value: library_replica::BookAddition {
                    facts: library_replica::BookFacts { added_at: 1, format: book_model::BookFormat::Epub },
                    record: book_model::BookRecord { metadata: Default::default(), description: String::new() },
                    navigation: None,
                    pdf: None,
                    recording: None,
                    enrichment: Some(library_replica::EnrichmentResult::NoMatch),
                },
            },
        )
        .unwrap();
        crate::sync::apply::commit(tx).unwrap();
        let annotation = book_model::Annotation {
            id: "offline-note".into(),
            content_hash: hash,
            anchor: book_model::AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
            exact_text: "Text".into(),
            style: book_model::AnnotationStyle::Highlight,
            color: 0xffffff,
            note: "Note".into(),
            created_at: 1,
            toc_ordinal: None,
            progress: None,
            modified_at: 2,
            deleted: false,
        };
        db.upsert_annotation(&annotation).unwrap();
        db.trash_book(&hash).unwrap();
        let pending = db.sync_publishable_mutations().unwrap();
        assert!(pending.iter().any(|m| matches!(m.state.value, library_replica::LibraryStateEntry::Annotation { .. })));
        db.connection.fixtures_create_book().unwrap();
        assert!(db.purge_book(&hash).is_err());
        assert_eq!(db.sync_publishable_mutations().unwrap(), pending);
        assert_eq!(db.annotations(hash).unwrap().len(), 1);
        assert_eq!(db.connection.fixtures_read_sync_book_record_count(&(&hash), |r| r.get::<_, i64>(0)).unwrap(), 1);
        db.connection.fixtures_drop_TRIGGER().unwrap();
        db.purge_book(&hash).unwrap();
        assert!(db.annotations(hash).unwrap().is_empty());
        assert_eq!(db.connection.fixtures_read_sync_book_record_count(&(&hash), |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(db.connection.fixtures_read_sync_state_version_count(&(&hash), |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert!(db.sync_publishable_mutations().unwrap().iter().all(|m| matches!(m.state.value, library_replica::LibraryStateEntry::BookLifecycle { .. })));
        add(&db, &hash);
        db.upsert_annotation(&annotation).unwrap();
        db.sync_acknowledge_mutations(&pending.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
        assert!(db.sync_publishable_mutations().unwrap().iter().any(|m| matches!(m.state.value, library_replica::LibraryStateEntry::Annotation { .. })), "old acknowledgements cannot erase readded work");
    }

    fn add(db: &Database, hash: &BookId) {
        let tx = db.connection.unchecked_transaction().unwrap();
        tx.fixtures_insert_book_2(&hash).unwrap();
        crate::sync::apply::registers::record_local(&tx, library_replica::LibraryStateEntry::BookLifecycle { content_hash: *hash, value: library_replica::BookLifecycleState::Present }).unwrap();
        crate::sync::apply::commit(tx).unwrap();
    }
}

include_sqlite_sql::include_sql!("src/sql/purge.sql");
#[cfg(test)]
use crate::sql_fixtures::FixturesSql;

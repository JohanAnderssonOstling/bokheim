//! Recompute all assignments from stored evidence, including previously unmatched books.
use crate::SqlParam;
use crate::query_methods::query_rows;
use crate::sync::apply::{project_book, write_subject_projection};
use crate::{Database, DatabaseError};
use book_model::{BookMetadata, BookSubject};
use rusqlite::OptionalExtension;

pub(crate) fn revision(db: &rusqlite::Connection) -> Result<String, DatabaseError> {
    let content: Option<String> = db.taxonomy_content_revision(|r| r.get(0)).optional()?;
    Ok(format!("{}:{}", content.as_deref().unwrap_or(subject_projection::BUNDLED_UNIFIED_TAXONOMY_REVISION_ID), subject_projection::UNIFIED_MATCHER_VERSION))
}

impl Database {
    pub fn refresh_taxonomy_assignments(&self) -> Result<(), DatabaseError> {
        let revision = revision(&self.connection)?;
        let old: Option<String> = self.connection.taxonomy_stored_revision(|r| r.get(0))?;
        if old.as_deref() == Some(revision.as_str()) {
            return Ok(());
        }
        self.write(|tx| {
            // Keyset batches keep memory bounded without excluding unmatched books.
            let mut cursor = 0i64;
            loop {
                let rows = query_rows(|callback| tx.taxonomy_book_page(&cursor, callback), |r| r.get::<_, i64>(0))?;
                if rows.is_empty() {
                    break;
                }
                for id in rows {
                    let subjects = query_rows(|callback| tx.taxonomy_book_subjects(&id, callback), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?)))?
                        .into_iter()
                        .map(|(name, source, authority, code)| BookSubject::new(None, name, source, authority, code).map_err(DatabaseError::operation))
                        .collect::<Result<Vec<_>, DatabaseError>>()?;
                    let metadata = BookMetadata { subjects, ..BookMetadata::default() };
                    let projected = project_book(&metadata)?;
                    write_subject_projection(tx, id, &projected)?;
                    cursor = id;
                }
            }
            tx.taxonomy_set_revision(&(&revision))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subject_projection::{CATALOG_CONCEPT_SOURCE, UNIFIED_SYSTEM_ID};

    #[test]
    fn serving_session_defers_stale_subjects_until_maintenance() {
        let db = Database::open(":memory:").unwrap();
        let library = sync_common::LibraryId::new_v4();
        db.initialize_library_for_deferred_taxonomy(library).unwrap();
        db.connection.fixtures_insert_book_3(&("a".repeat(64))).unwrap();
        db.connection.fixtures_insert_book_unified_concept().unwrap();
        db.connection.fixtures_update_library_local_state_taxonomy_revision().unwrap();
        db.initialize_library_for_deferred_taxonomy(library).unwrap();
        assert_eq!(db.connection.fixtures_read_book_unified_concept_count(|row| row.get::<_, i64>(0)).unwrap(), 1);
        db.refresh_taxonomy_assignments().unwrap();
        assert_eq!(db.connection.fixtures_read_book_unified_concept_count(|row| row.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn refresh_keeps_imported_catalog_concept_identity_without_external_codes() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.fixtures_insert_book_3(&("a".repeat(64))).unwrap();
        let subject = BookSubject::new(None, "An old display label", CATALOG_CONCEPT_SOURCE, Some(UNIFIED_SYSTEM_ID.into()), Some("concept:3915".into())).unwrap();
        let projected = project_book(&BookMetadata { subjects: vec![subject], ..Default::default() }).unwrap();
        let book: i64 = db.connection.query_row("SELECT row_id FROM book", [], |row| row.get(0)).unwrap();
        db.write(|tx| write_subject_projection(tx, book, &projected)).unwrap();
        db.connection.fixtures_update_library_local_state_taxonomy_revision().unwrap();
        db.refresh_taxonomy_assignments().unwrap();
        let concept: i64 = db.connection.query_row("SELECT concept_id FROM book_unified_concept WHERE book_row_id=?1", [book], |row| row.get(0)).unwrap();
        assert_eq!(concept, 3915);
        let source: String = db.connection.query_row("SELECT source FROM book_subject WHERE book_row_id=?1", [book], |row| row.get(0)).unwrap();
        assert_eq!(source, CATALOG_CONCEPT_SOURCE);
    }

    #[test]
    fn refresh_removes_stale_assignments_without_changing_canonical_data_and_is_idempotent() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.fixtures_insert_book_3(&("a".repeat(64))).unwrap();
        db.connection.fixtures_insert_book_subject_2().unwrap();
        db.connection.fixtures_insert_book_unified_concept().unwrap();
        db.connection.fixtures_update_library_local_state_taxonomy_revision().unwrap();
        let outbox: i64 = db.connection.fixtures_read_sync_outbox_count(|r| r.get(0)).unwrap();
        db.refresh_taxonomy_assignments().unwrap();
        assert_eq!(db.connection.fixtures_read_book_unified_concept_count(|r| r.get::<_, i64>(0)).unwrap(), 0);
        let subject: (String, String, String, String) = db.connection.fixtures_read_book_subject_name_source_authority(|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(subject, ("Unmatched subject".into(), "file".into(), "unknown-authority".into(), "unknown-code".into()));
        assert_eq!(db.connection.fixtures_read_sync_outbox_count(|r| r.get::<_, i64>(0)).unwrap(), outbox);
        let changes = db.connection.total_changes();
        db.refresh_taxonomy_assignments().unwrap();
        assert_eq!(db.connection.total_changes(), changes);
    }
    #[test]
    fn failure_rolls_back_every_assignment_and_revision() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.fixtures_insert_book_3(&("a".repeat(64))).unwrap();
        db.connection.fixtures_insert_book_unified_concept().unwrap();
        db.connection.fixtures_insert_book_4(&("b".repeat(64))).unwrap();
        db.connection.fixtures_insert_book_subject_3(&("x".repeat(4097))).unwrap();
        db.connection.fixtures_update_library_local_state_taxonomy_revision().unwrap();
        assert!(db.refresh_taxonomy_assignments().is_err());
        assert_eq!(db.connection.fixtures_read_book_unified_concept_count(|r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(db.connection.fixtures_read_library_local_state_count(|r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}

include_sqlite_sql::include_sql!("src/sql/taxonomy.sql");
#[cfg(test)]
use crate::sql_fixtures::FixturesSql;

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn refresh_taxonomy_assignments() -> ();
    arguments () = ();
    execute database, arguments => database.refresh_taxonomy_assignments();
}

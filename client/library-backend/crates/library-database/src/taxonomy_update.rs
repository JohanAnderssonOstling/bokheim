//! Recompute all assignments from stored evidence, including previously unmatched books.
use crate::sync::apply::{project_book, write_subject_projection};
use crate::{Database, DatabaseError};
use book_model::{BookMetadata, BookSubject};
use rusqlite::OptionalExtension;

pub(crate) fn revision(db: &rusqlite::Connection) -> Result<String, DatabaseError> {
    let content: Option<String> = db.query_row("SELECT value FROM curated.taxonomy_meta WHERE key='client_revision'", [], |r| r.get(0)).optional()?;
    Ok(format!("{}:{}", content.as_deref().unwrap_or(subject_projection::BUNDLED_UNIFIED_TAXONOMY_REVISION_ID), subject_projection::UNIFIED_MATCHER_VERSION))
}

impl Database {
    pub(crate) fn refresh_taxonomy_assignments(&self) -> Result<(), DatabaseError> {
        let revision = revision(&self.connection)?;
        let old: Option<String> = self.connection.query_row("SELECT taxonomy_revision FROM library_local_state WHERE singleton=1", [], |r| r.get(0))?;
        if old.as_deref() == Some(&revision) {
            return Ok(());
        }
        self.write(|tx| {
            // Keyset batches keep memory bounded without excluding unmatched books.
            let mut cursor = 0i64;
            loop {
                let rows = tx.prepare("SELECT row_id FROM book WHERE row_id>?1 ORDER BY row_id LIMIT 128")?.query_map([cursor], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
                if rows.is_empty() {
                    break;
                }
                for id in rows {
                    let subjects = tx
                        .prepare("SELECT name,source,authority,code FROM book_subject WHERE book_row_id=?1 ORDER BY position")?
                        .query_map([id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?)))?
                        .map(|row| {
                            let (name, source, authority, code) = row?;
                            BookSubject::new(None, name, source, authority, code).map_err(DatabaseError::operation)
                        })
                        .collect::<Result<Vec<_>, DatabaseError>>()?;
                    let metadata = BookMetadata { subjects, ..BookMetadata::default() };
                    let projected = project_book(&metadata)?;
                    write_subject_projection(tx, id, &projected)?;
                    cursor = id;
                }
            }
            tx.execute("UPDATE library_local_state SET taxonomy_revision=?1 WHERE singleton=1", [&revision])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_removes_stale_assignments_without_changing_canonical_data_and_is_idempotent() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.execute("INSERT INTO book(row_id,content_hash) VALUES(1,?1)", ["a".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO book_subject(book_row_id,position,name,source,authority,code) VALUES(1,0,'Unmatched subject','file','unknown-authority','unknown-code')", []).unwrap();
        db.connection.execute("INSERT INTO book_unified_concept VALUES(1,123456,1)", []).unwrap();
        db.connection.execute("UPDATE library_local_state SET taxonomy_revision=NULL WHERE singleton=1", []).unwrap();
        let outbox: i64 = db.connection.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0)).unwrap();
        db.refresh_taxonomy_assignments().unwrap();
        assert_eq!(db.connection.query_row("SELECT count(*) FROM book_unified_concept", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let subject: (String, String, String, String) = db.connection.query_row("SELECT name,source,authority,code FROM book_subject", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(subject, ("Unmatched subject".into(), "file".into(), "unknown-authority".into(), "unknown-code".into()));
        assert_eq!(db.connection.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), outbox);
        let changes = db.connection.total_changes();
        db.refresh_taxonomy_assignments().unwrap();
        assert_eq!(db.connection.total_changes(), changes);
    }
    #[test]
    fn failure_rolls_back_every_assignment_and_revision() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.execute("INSERT INTO book(row_id,content_hash) VALUES(1,?1)", ["a".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO book_unified_concept VALUES(1,123456,1)", []).unwrap();
        db.connection.execute("INSERT INTO book(row_id,content_hash) VALUES(2,?1)", ["b".repeat(64)]).unwrap();
        db.connection.execute("INSERT INTO book_subject(book_row_id,position,name,source) VALUES(2,0,?1,'dc:subject')", ["x".repeat(4097)]).unwrap();
        db.connection.execute("UPDATE library_local_state SET taxonomy_revision=NULL WHERE singleton=1", []).unwrap();
        assert!(db.refresh_taxonomy_assignments().is_err());
        assert_eq!(db.connection.query_row("SELECT count(*) FROM book_unified_concept", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(db.connection.query_row("SELECT count(*) FROM library_local_state WHERE taxonomy_revision IS NOT NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}

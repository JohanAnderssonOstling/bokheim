//! Transactional, connection-local invalidation. Nothing here enters sync or
//! changes the persistent schema. Startup discovery handles process restarts.
use crate::{Database, DatabaseError};
use sync_common::BookId;

impl Database {
    pub fn begin_annotation_export_tracking(&self) -> Result<(), DatabaseError> {
        self.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS annotation_export_changed(book_id BLOB PRIMARY KEY) WITHOUT ROWID;")?;
        for (name, table, key, guard) in [
            ("annotation", "annotation", "SELECT content_hash FROM main.book WHERE row_id={row}.book_row_id", "{old}.body IS NOT {new}.body"),
            (
                "book",
                "book",
                "SELECT {row}.content_hash",
                "{old}.title IS NOT {new}.title OR {old}.subtitle IS NOT {new}.subtitle OR {old}.format IS NOT {new}.format OR {old}.toc_json IS NOT {new}.toc_json OR {old}.deleted_at IS NOT {new}.deleted_at",
            ),
            (
                "placement",
                "book_dir",
                "SELECT content_hash FROM main.book WHERE row_id={row}.book_row_id",
                "{old}.dir_id IS NOT {new}.dir_id OR {old}.file_name IS NOT {new}.file_name OR {old}.is_downloaded IS NOT {new}.is_downloaded OR {old}.deleted_at IS NOT {new}.deleted_at",
            ),
            ("canonical", "book_content", "SELECT {row}.book_id", "{old}.canonical_hash IS NOT {new}.canonical_hash"),
            ("local_content", "local_file_content", "SELECT {row}.book_id", "{old}.byte_hash IS NOT {new}.byte_hash"),
            ("pdf", "pdf_reader_metadata", "SELECT {row}.content_hash", "{old}.metadata IS NOT {new}.metadata"),
            ("record", "sync_book_record", "SELECT {row}.content_hash", "{old}.body IS NOT {new}.body"),
            ("draft", "local_book_draft", "SELECT {row}.content_hash", "{old}.body IS NOT {new}.body"),
        ] {
            for (event, row) in [("INSERT", "NEW"), ("UPDATE", "NEW"), ("DELETE", "OLD")] {
                let timing = if event == "DELETE" { "BEFORE" } else { "AFTER" };
                let when = if event == "UPDATE" { format!("WHEN {}", guard.replace("{old}", "OLD").replace("{new}", "NEW")) } else { String::new() };
                let key = key.replace("{row}", row);
                self.connection.execute_batch(&format!(
                    "CREATE TEMP TRIGGER IF NOT EXISTS export_changed_{name}_{event} {timing} {event} ON main.{table} {when}
                     BEGIN INSERT INTO annotation_export_changed(book_id) SELECT * FROM ({key}) WHERE true ON CONFLICT(book_id) DO NOTHING; END;"
                ))?;
            }
        }
        // Only books below a changed directory need their output paths checked.
        // Reading progress and unrelated directory changes do not load books.
        self.connection.execute_batch(
            "CREATE TEMP TRIGGER IF NOT EXISTS export_changed_directory_update BEFORE UPDATE OF name,parent_id,deleted_at ON main.dir
             WHEN OLD.name IS NOT NEW.name OR OLD.parent_id IS NOT NEW.parent_id OR OLD.deleted_at IS NOT NEW.deleted_at
             BEGIN INSERT INTO annotation_export_changed(book_id)
               SELECT b.content_hash FROM main.book_dir bd JOIN main.book b ON b.row_id=bd.book_row_id
               WHERE bd.dir_id IN (WITH RECURSIVE children(id) AS (SELECT OLD.id UNION SELECT d.id FROM main.dir d JOIN children c ON d.parent_id=c.id WHERE d.id<>c.id) SELECT id FROM children)
               ON CONFLICT(book_id) DO NOTHING;
             END;",
        )?;
        Ok(())
    }

    pub fn take_annotation_export_changes(&self) -> Result<Vec<BookId>, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let books = transaction.prepare("SELECT book_id FROM temp.annotation_export_changed ORDER BY book_id")?.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
        transaction.execute("DELETE FROM temp.annotation_export_changed", [])?;
        transaction.commit()?;
        Ok(books)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use sync_common::ROOT_DIR_ID;

    #[test]
    fn changes_are_transactional_book_scoped_and_ignore_reading_progress() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.begin_annotation_export_tracking().unwrap();
        let first = BookId::from_bytes([1; 32]);
        let second = BookId::from_bytes([2; 32]);
        for book in [first, second] {
            db.connection.execute("INSERT INTO book(content_hash) VALUES(?1)", [book]).unwrap();
        }
        assert_eq!(db.take_annotation_export_changes().unwrap(), vec![first, second]);
        db.connection.execute("INSERT OR IGNORE INTO book(content_hash) VALUES(?1)", [first]).unwrap();
        assert!(db.take_annotation_export_changes().unwrap().is_empty(), "ignored writes must not schedule exports");
        db.connection.execute("UPDATE book SET read_progress=0.5 WHERE content_hash=?1", [first]).unwrap();
        assert!(db.take_annotation_export_changes().unwrap().is_empty());
        let transaction = db.connection.unchecked_transaction().unwrap();
        transaction.execute("INSERT INTO annotation(id,book_row_id,body,modified_at) SELECT 'note',row_id,x'00',1 FROM book WHERE content_hash=?1", [first]).unwrap();
        assert_eq!(transaction.query_row("SELECT count(*) FROM temp.annotation_export_changed", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        transaction.rollback().unwrap();
        assert!(db.take_annotation_export_changes().unwrap().is_empty());
        db.connection.execute("INSERT INTO annotation(id,book_row_id,body,modified_at) SELECT 'note',row_id,x'00',1 FROM book WHERE content_hash=?1", [first]).unwrap();
        assert_eq!(db.take_annotation_export_changes().unwrap(), vec![first]);
        db.connection.execute("UPDATE annotation SET body=x'01',deleted_at=2 WHERE id='note'", []).unwrap();
        assert_eq!(db.take_annotation_export_changes().unwrap(), vec![first]);
        db.connection.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash) SELECT ?1,row_id,'Book.epub','scan' FROM book WHERE content_hash=?2", params![ROOT_DIR_ID.to_string(), first]).unwrap();
        assert_eq!(db.take_annotation_export_changes().unwrap(), vec![first]);
        db.connection.execute("UPDATE book_dir SET local_hash='new scan fingerprint'", []).unwrap();
        assert!(db.take_annotation_export_changes().unwrap().is_empty());
        db.connection.execute("INSERT INTO local_file_content(book_id,dir_id,byte_hash) VALUES(?1,?2,zeroblob(32))", params![first, ROOT_DIR_ID.to_string()]).unwrap();
        assert_eq!(db.take_annotation_export_changes().unwrap(), vec![first]);
        // Temporary tracking never becomes a synchronized/persistent table.
        assert_eq!(db.connection.query_row("SELECT count(*) FROM main.sqlite_schema WHERE name='annotation_export_changed'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
}

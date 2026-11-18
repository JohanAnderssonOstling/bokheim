//! Retain upgrades from the last published web client's schema (44).
use crate::{DatabaseError, annotations, notebook};
use book_model::Annotation;
use rusqlite::Transaction;
use sync_common::BookId;

pub(crate) mod book_ids;
mod storage;
pub(crate) use storage::upgrade as upgrade_storage;

pub(crate) fn upgrade_before_50(tx: &Transaction<'_>, version: i64) -> Result<(), DatabaseError> {
    tx.execute_batch(include_str!("annotations/sql/schema/tables.sql"))?;
    tx.execute_batch(include_str!("content_identity.sql"))?;
    if version < 45 {
        let has_checksum: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('book_transfer') WHERE name='upload_checksum')", [], |row| row.get(0))?;
        if !has_checksum {
            tx.execute_batch("ALTER TABLE book_transfer ADD COLUMN upload_checksum BLOB CHECK(upload_checksum IS NULL OR length(upload_checksum)=32)")?;
        }
        tx.execute("INSERT OR IGNORE INTO book_content(book_id,canonical_hash,size_bytes) SELECT b.content_hash,b.content_hash,(SELECT MAX(d.size_bytes) FROM book_dir d WHERE d.book_row_id=b.row_id) FROM book b JOIN book_transfer t ON t.content_hash=b.content_hash WHERE length(b.content_hash)=32 AND t.server_has_book=1", [])?;
        tx.execute("INSERT OR IGNORE INTO local_content_alias(byte_hash,book_id) SELECT content_hash,content_hash FROM book WHERE length(content_hash)=32", [])?;
    }
    tx.execute_batch(include_str!("storage.sql"))?;
    if version < 47 {
        // Only the derived search projection is replaced; canonical notes stay.
        tx.execute_batch("DROP TABLE IF EXISTS notebook_annotation")?;
        tx.execute_batch(include_str!("notebook/schema.sql"))?;
        let annotations = tx.prepare("SELECT body FROM annotation WHERE deleted_at IS NULL")?.query_map([], |row| row.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?;
        for body in annotations {
            let annotation = Annotation::decode(&body).map_err(DatabaseError::operation)?;
            notebook::project_annotation(tx, &annotation)?;
            annotations::project_recurrence(tx, &annotation)?;
        }
        let books = tx.prepare("SELECT content_hash FROM pdf_reader_metadata")?.query_map([], |row| row.get::<_, BookId>(0))?.collect::<Result<Vec<_>, _>>()?;
        for book in books {
            notebook::project_pdf(tx, book)?;
        }
    }
    if version < 48 {
        tx.execute_batch(include_str!("notebook/schema.sql"))?;
    }
    if version < 49 {
        tx.execute_batch(include_str!("sync/sql/schema/remove_torrents.sql"))?;
    }
    if version < 50 {
        tx.execute_batch(include_str!("sync/sql/schema/compress_descriptions.sql"))?;
    }
    #[cfg(target_arch = "wasm32")]
    tx.execute_batch("DELETE FROM local_file_work; DELETE FROM local_directory_work; DELETE FROM local_book_work;")?;
    Ok(())
}

/// Install derived indexes and caches after historical identity migration.
pub(crate) fn upgrade_review(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    tx.execute_batch(include_str!("migration/review.sql"))?;
    for (legacy, book) in book_ids::mapping(tx)? {
        tx.execute("INSERT INTO local_legacy_book_id(legacy_id,book_id) VALUES(?1,?2)", (legacy, book))?;
    }
    Ok(())
}

/// Existing optional indexes need the same invalidation triggers as new ones.
pub(crate) fn upgrade_text_index(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    let installed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='book_text_state')", [], |row| row.get(0))?;
    if installed {
        tx.execute_batch(include_str!("text_search/schema.sql"))?;
    }
    Ok(())
}

/// Install indexed directory projection and scanner journals.
pub(crate) fn upgrade_directories(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    tx.execute_batch(include_str!("migration/directories.sql"))?;
    Ok(())
}

/// Observe completed file work without treating scan bookkeeping as progress.
pub(crate) fn upgrade_scan_progress(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    tx.execute_batch(include_str!("migration/scan_progress.sql"))?;
    Ok(())
}

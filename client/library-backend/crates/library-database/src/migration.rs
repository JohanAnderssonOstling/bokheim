//! Retain upgrades from the last published web client's schema (44).
use crate::sync::{apply::registers, storage_i64};
use crate::{DatabaseError, annotations, notebook};
use book_model::Annotation;
use library_replica::LibraryStateEntry;
use rusqlite::{Transaction, params};
use sync_common::{BookId, VersionKey, wire};

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

/// Preserve valid decimal fingerprints; unreadable cached revisions are rescanned.
pub(crate) fn upgrade_fingerprints(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    tx.execute_batch("DROP INDEX IF EXISTS idx_book_dir_local_hash")?;
    for (table, column, key) in [("book_dir", "local_hash", "rowid"), ("local_import_source", "fingerprint", "path")] {
        let rows = tx.prepare(&format!("SELECT {key},{column} FROM {table}"))?.query_map([], |row| Ok((row.get::<_, rusqlite::types::Value>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN fingerprint_blob BLOB CHECK(fingerprint_blob IS NULL OR (typeof(fingerprint_blob)='blob' AND length(fingerprint_blob)=8))"))?;
        for (key_value, text) in rows {
            let fingerprint = text.parse::<u64>().ok().map(u64::to_le_bytes);
            tx.execute(&format!("UPDATE {table} SET fingerprint_blob=?2 WHERE {key}=?1"), params![key_value, fingerprint])?;
        }
        tx.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column}; ALTER TABLE {table} RENAME COLUMN fingerprint_blob TO {column}"))?;
    }
    tx.execute_batch("CREATE INDEX idx_book_dir_local_hash ON book_dir(local_hash)")?;
    Ok(())
}

/// Publish retained drafts, then remove their retired storage.
pub(crate) fn upgrade_optional_enrichment(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    tx.execute_batch("ALTER TABLE book_transfer ADD COLUMN enrichment_retry_at INTEGER")?;
    {
        let mut drafts = tx.prepare("SELECT body FROM local_book_draft")?;
        let rows = drafts.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        for bytes in rows {
            let value = wire::decode::<LibraryStateEntry>(&bytes?, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
            let address = value.address();
            if registers::canonical_body(tx, &address.kind, &address.entity_key, &address.entity_subkey)?.is_none() {
                registers::record_local(tx, value)?;
            }
        }
    }
    tx.execute_batch("DROP TABLE local_book_draft; DROP VIEW IF EXISTS uploadable_covers;")?;
    tx.execute_batch(include_str!("assets/sql/schema/asset_views.sql"))?;
    Ok(())
}

/// Preserve publication identities while moving book records into the shared register table.
pub(crate) fn upgrade_book_records(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    let indexes = tx.prepare("SELECT sql FROM sqlite_schema WHERE tbl_name='sync_state_version' AND type IN ('index','trigger') AND sql IS NOT NULL")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    tx.execute_batch(include_str!("migration/book_records.sql"))?;
    for sql in indexes {
        tx.execute_batch(&sql)?;
    }
    {
        let mut records = tx.prepare("SELECT content_hash,body,publication FROM sync_book_record")?;
        let rows = records.query_map([], |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
        for row in rows {
            let (book, body, publication) = row?;
            let version: VersionKey = wire::decode(&publication, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
            tx.execute(
                "INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key)
                VALUES('book_add',?1,'',?2,?3,?4,?5,?6,?1)",
                params![book, storage_i64(version.changed_at, "book timestamp")?, u8::from(version.conflict_rank), version.replica_id.to_string(), storage_i64(version.replica_seq.get(), "book sequence")?, body],
            )?;
        }
    }
    tx.execute_batch("DROP TABLE sync_book_record")?;
    Ok(())
}

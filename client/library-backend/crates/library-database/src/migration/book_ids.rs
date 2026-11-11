//! Replace library-scoped identities without changing local row IDs or notes.
use crate::{DatabaseError, stored_document, sync::apply::registers};
use book_model::{Annotation, AudiobookTrack};
use content_address::{BookId, ContentHash};
use library_replica::LibraryStateEntry;
use pdf_view_common::PdfReaderMetadata;
use rusqlite::{OptionalExtension, Transaction, params, types::Value};
use std::collections::{BTreeMap, BTreeSet};
use sync_common::{StateKey, wire};

pub(crate) type IdMap = BTreeMap<BookId, BookId>;

pub(crate) fn mapping(connection: &rusqlite::Connection) -> Result<IdMap, DatabaseError> {
    let bytes: Option<Vec<u8>> = connection.query_row("SELECT book_ids FROM local_identity_scope WHERE singleton=1", [], |row| row.get(0)).optional()?.flatten();
    bytes.map(|bytes| stored_document::decode(&bytes)).transpose().map(Option::unwrap_or_default)
}

fn remap(ids: &IdMap, book: BookId) -> BookId {
    ids.get(&book).copied().unwrap_or(book)
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Only declared relational identity columns are touched. Byte checksums,
/// annotation prose, paths and UUIDs are never interpreted as book IDs.
fn identity_columns(tx: &Transaction<'_>) -> Result<Vec<(String, String)>, DatabaseError> {
    let tables = tx.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let mut columns = Vec::new();
    for table in tables {
        let names = tx.prepare(&format!("PRAGMA table_info({})", quote(&table)))?.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
        for column in names {
            if matches!(column.as_str(), "content_hash" | "book_id" | "book_key" | "state_key" | "book_row_id") {
                // local_audiobook_content.content_hash is a recording checksum.
                if table == "local_audiobook_content" && column == "content_hash" {
                    continue;
                }
                columns.push((table.clone(), column));
            }
        }
    }
    Ok(columns)
}

fn rewrite_documents(tx: &Transaction<'_>, ids: &IdMap) -> Result<(), DatabaseError> {
    for (table, column) in [
        ("sync_state_version", "body"),
        ("sync_book_record", "body"),
        ("local_book_draft", "body"),
        ("sync_outbox", "body"),
        ("annotation", "body"),
        ("notebook_annotation", "body"),
        ("pdf_reader_metadata", "metadata"),
        ("book", "tracks_json"),
    ] {
        let rows = tx
            .prepare(&format!("SELECT rowid,{column} FROM {table} WHERE {column} IS NOT NULL"))?
            .query_map([], |row| {
                let value = row.get_ref(1)?;
                Ok((row.get::<_, i64>(0)?, value.as_bytes()?.to_vec()))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (row, bytes) in rows {
            let updated = match table {
                "annotation" | "notebook_annotation" => {
                    let mut value = Annotation::decode(&bytes).map_err(DatabaseError::operation)?;
                    value.book_id = remap(ids, value.book_id);
                    stored_document::encode(&value)?
                }
                "pdf_reader_metadata" => {
                    let mut value: PdfReaderMetadata = stored_document::decode(&bytes)?;
                    let book = value.book_id.parse().map_err(DatabaseError::operation)?;
                    value.book_id = remap(ids, book).to_hex();
                    stored_document::encode(&value)?
                }
                "book" => {
                    let mut value: Vec<AudiobookTrack> = stored_document::decode(&bytes)?;
                    for track in &mut value {
                        track.archive_hash = track.archive_hash.map(|book| remap(ids, book));
                    }
                    stored_document::encode(&value)?
                }
                _ => {
                    let mut value: LibraryStateEntry = stored_document::decode(&bytes)?;
                    value.remap_books(|book| remap(ids, book));
                    stored_document::encode(&value)?
                }
            };
            if updated != bytes {
                tx.execute(&format!("UPDATE {table} SET {column}=?2 WHERE rowid=?1"), params![row, updated])?;
            }
        }
    }
    let adopted: Option<String> = tx.query_row("SELECT adopted_json FROM local_storage_move WHERE singleton=1", [], |row| row.get(0)).optional()?;
    if let Some(adopted) = adopted {
        let mut values: Vec<(BookId, String, ContentHash)> = serde_json::from_str(&adopted).map_err(DatabaseError::operation)?;
        for (book, _, _) in &mut values {
            *book = remap(ids, *book);
        }
        tx.execute("UPDATE local_storage_move SET adopted_json=?1 WHERE singleton=1", [serde_json::to_string(&values).map_err(DatabaseError::operation)?])?;
    }
    Ok(())
}

fn merge_registers(tx: &Transaction<'_>, old: BookId, new: BookId) -> Result<(), DatabaseError> {
    // Preserve the ordinary register conflict ordering when two historical
    // copies converge on the same global identity.
    tx.execute(
        "INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key)
         SELECT state_kind,?2,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key FROM sync_state_version WHERE state_key=?1
         ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,body=excluded.body,book_key=excluded.book_key
         WHERE (excluded.changed_at,excluded.conflict_rank,excluded.replica_id COLLATE BINARY,excluded.replica_seq)>(sync_state_version.changed_at,sync_state_version.conflict_rank,sync_state_version.replica_id COLLATE BINARY,sync_state_version.replica_seq)",
        params![old, new],
    )?;
    tx.execute("DELETE FROM sync_state_version WHERE state_key=?1", [old])?;
    Ok(())
}

fn merge_book_rows(tx: &Transaction<'_>, columns: &[(String, String)], old: BookId, new: BookId) -> Result<(), DatabaseError> {
    let old_row: Option<i64> = tx.query_row("SELECT row_id FROM book WHERE content_hash=?1", [old], |row| row.get(0)).optional()?;
    let new_row: Option<i64> = tx.query_row("SELECT row_id FROM book WHERE content_hash=?1", [new], |row| row.get(0)).optional()?;
    let (Some(old_row), Some(new_row)) = (old_row, new_row) else { return Ok(()) };
    let conflicting: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM book_content old JOIN book_content new ON new.book_id=?2 WHERE old.book_id=?1 AND old.canonical_hash<>new.canonical_hash)", params![old, new], |row| row.get(0))?;
    if conflicting {
        return Err(DatabaseError::message("global book identity would merge different uploaded revisions"));
    }
    tx.execute(
        "UPDATE book_transfer AS target SET
            server_has_book=MAX(target.server_has_book,source.server_has_book),server_has_cover=MAX(target.server_has_cover,source.server_has_cover),
            cover_state=CASE WHEN target.cover_state='ready' OR source.cover_state='ready' THEN 'ready' ELSE COALESCE(target.cover_state,source.cover_state) END,
            book_download=CASE WHEN target.book_download='user_initiated' OR source.book_download='user_initiated' THEN 'user_initiated' ELSE COALESCE(target.book_download,source.book_download) END,
            book_download_at=COALESCE(target.book_download_at,source.book_download_at),
            upload_size=CASE WHEN target.upload_id IS NULL THEN source.upload_size ELSE target.upload_size END,
            upload_checksum=CASE WHEN target.upload_id IS NULL THEN source.upload_checksum ELSE target.upload_checksum END,
            upload_prepared=CASE WHEN target.upload_id IS NULL THEN source.upload_prepared ELSE target.upload_prepared END,
            upload_id=COALESCE(target.upload_id,source.upload_id)
         FROM book_transfer source WHERE source.content_hash=?1 AND target.content_hash=?2",
        params![old, new],
    )?;
    // A downloaded placement must not lose its physical observation merely
    // because the other historical copy was already selected as the survivor.
    tx.execute(
        "UPDATE book_dir AS target SET file_name=source.file_name,local_hash=source.local_hash,is_downloaded=1,size_bytes=source.size_bytes
         FROM book_dir source WHERE target.book_row_id=?2 AND source.book_row_id=?1 AND target.dir_id=source.dir_id AND target.is_downloaded=0 AND source.is_downloaded=1 AND source.deleted_at IS NULL",
        params![old_row, new_row],
    )?;
    for (table, column) in columns.iter().filter(|(_, column)| column == "book_row_id") {
        tx.execute(&format!("UPDATE OR IGNORE {} SET {}=?2 WHERE {}=?1", quote(table), quote(column), quote(column)), params![old_row, new_row])?;
    }
    // All child references have moved before any duplicate parent projection
    // is deleted, including the subject/evidence composite foreign key.
    for (table, column) in columns.iter().filter(|(_, column)| column == "book_row_id") {
        tx.execute(&format!("DELETE FROM {} WHERE {}=?1", quote(table), quote(column)), [old_row])?;
    }
    Ok(())
}

pub(crate) fn upgrade(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    let has_mapping: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('local_identity_scope') WHERE name='book_ids')", [], |row| row.get(0))?;
    if !has_mapping {
        tx.execute_batch("ALTER TABLE local_identity_scope ADD COLUMN book_ids BLOB; ALTER TABLE local_identity_scope ADD COLUMN assets_pending INTEGER NOT NULL DEFAULT 0 CHECK(assets_pending IN (0,1));")?;
    }
    tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
    let scope: Vec<u8> = tx.query_row("SELECT scope FROM local_identity_scope WHERE singleton=1", [], |row| row.get(0))?;
    let scope: [u8; 16] = scope.try_into().map_err(|_| DatabaseError::message("invalid legacy library identity scope"))?;
    let books = tx.prepare("SELECT content_hash,deleted_at FROM book WHERE length(content_hash)=33")?.query_map([], |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, Option<i64>>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    let mut ids = IdMap::new();
    for (old, deleted) in books {
        if !old.is_library_scoped() {
            continue;
        }
        let canonical: Option<ContentHash> = tx.query_row("SELECT canonical_hash FROM book_content WHERE book_id=?1", [old], |row| row.get(0)).optional()?;
        let proofs = tx
            .prepare("SELECT byte_hash FROM local_content_alias WHERE book_id=?1 UNION SELECT content_hash FROM local_audiobook_content WHERE book_id=?1 UNION SELECT canonical_hash FROM local_content_origin WHERE book_id=?1")?
            .query_map([old], |row| row.get::<_, ContentHash>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        // Uploaded revisions are available on every replica and the server.
        // Unpublished books recover their initial revision from local proofs.
        let initial = canonical.or_else(|| proofs.iter().copied().find(|hash| BookId::legacy_scoped(&scope, *hash) == old)).or_else(|| (proofs.len() == 1).then(|| proofs[0]));
        let Some(initial) = initial else {
            if deleted.is_some() {
                log::warn!("retaining opaque tombstone {old}: no content proof survives");
                continue;
            }
            return Err(DatabaseError::message(format!("cannot migrate book {old}: its initial content checksum is missing or ambiguous")));
        };
        let new = BookId::for_local_content(initial);
        if old != new {
            ids.insert(old, new);
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    if ids.values().any(|new| ids.contains_key(new)) {
        return Err(DatabaseError::message("overlapping legacy and global book identities"));
    }
    rewrite_documents(tx, &ids)?;
    let columns = identity_columns(tx)?;
    let queued = tx.prepare("SELECT state_kind,state_key,state_subkey FROM sync_outbox")?.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Value>(1)?, row.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
    // Prepared envelopes and publisher sequences cannot be reused at a new
    // address: an old acknowledgement could otherwise discard new work.
    tx.execute("DELETE FROM sync_outbox", [])?;
    let mut queued_addresses = BTreeSet::new();
    for (kind, key, subkey) in queued {
        let key = match key {
            Value::Blob(bytes) => {
                let key = StateKey::from_storage(&kind, &bytes).map_err(DatabaseError::message)?;
                match key {
                    StateKey::Book(book) => StateKey::Book(remap(&ids, book)),
                    key => key,
                }
            }
            Value::Text(text) => StateKey::Text(text),
            _ => return Err(DatabaseError::message("invalid queued state key")),
        };
        queued_addresses.insert((kind, key.as_bytes().to_vec(), subkey));
    }
    for (&old, &new) in &ids {
        merge_registers(tx, old, new)?;
        merge_book_rows(tx, &columns, old, new)?;
        // Children are updated before deleting duplicate book rows. In the
        // usual case the integer book row, notes and placements stay in place.
        for (table, column) in columns.iter().filter(|(table, column)| table != "book" && column != "book_row_id" && !(table == "sync_state_version" && column == "state_key")) {
            tx.execute(&format!("UPDATE OR IGNORE {} SET {}=?2 WHERE {}=?1", quote(table), quote(column), quote(column)), params![old, new])?;
            tx.execute(&format!("DELETE FROM {} WHERE {}=?1", quote(table), quote(column)), [old])?;
        }
        tx.execute("UPDATE OR IGNORE book SET content_hash=?2 WHERE content_hash=?1", params![old, new])?;
        tx.execute("DELETE FROM book WHERE content_hash=?1", [old])?;
        tx.execute("INSERT OR IGNORE INTO sync_projection_dirty(state_kind,state_key,state_subkey) VALUES('book_projection',?1,'')", [new])?;
    }
    for (kind, key, subkey) in queued_addresses {
        let key = StateKey::from_storage(&kind, &key).map_err(DatabaseError::message)?;
        registers::schedule(tx, &kind, &key, &subkey, None)?;
    }
    let bytes = wire::encode(&ids).map_err(DatabaseError::operation)?;
    tx.execute("UPDATE local_identity_scope SET book_ids=?1,assets_pending=1 WHERE singleton=1", [bytes])?;
    crate::projection::project_dirty(tx)?;
    log::info!("migrated {} library-scoped book identities", ids.len());
    Ok(())
}

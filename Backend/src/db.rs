// `include_sql!` generates query methods with one argument per SQL parameter.
#![allow(clippy::too_many_arguments)]

use crate::library::{Book, Dir, NonNegativeTimestamp};
use crate::path_types::{LibraryRoot, RelativeDirPath};
use crate::reading::{EpubCfi, ReadProgress};
use crate::transfer::{TransferJob, TransferOrigin};
use crate::{BlobKind, BookId, BookPlacement, ContentHash, RelativeBookPath, TransferJobKind, TransferState, TransferStatus};
use include_sqlite_sql::{impl_sql, include_sql};
use rusqlite::types::Type;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, Transaction};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::hash::Hasher;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::{ActorId, ClientId, DirId, DirectoryState, EventId, FileName, Hlc, LocalChange, LocalSeq, ServerChange, SyncOp, VersionKey};
use uuid::Uuid;
use xxhash_rust::xxh3::Xxh3;

include_sql!("src/database/create_library.sql");
include_sql!("src/database/library.sql");
include_sql!("src/database/views.sql");

mod remote_reducer;

pub use remote_reducer::apply_remote_change_in_tx;
#[cfg(test)]
use remote_reducer::{apply_remote_book_dir, RemotePlacementState};

fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn nonnegative_u64(value: i64, column: usize, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} must not be negative")))
}

fn nonnegative_i64(value: i64, column: usize, name: &str) -> rusqlite::Result<i64> {
    if value < 0 {
        Err(invalid_integer_column(column, format!("{name} must not be negative")))
    } else {
        Ok(value)
    }
}

fn bounded_u8(value: i64, column: usize, name: &str) -> rusqlite::Result<u8> {
    u8::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} is outside its storage range")))
}

fn storage_i64(value: u64, name: &str) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} exceeds the database integer range")))))
}

#[derive(Clone, Debug)]
struct StoredVersion(VersionKey);

impl StoredVersion {
    fn decode(hlc_pt: i64, hlc_l: i64, actor: String, actor_seq: i64, event_id: String, conflict_rank: u8, columns: [usize; 5]) -> rusqlite::Result<Option<Self>> {
        if hlc_pt == 0 && hlc_l == 0 && actor.is_empty() && actor_seq == 0 && event_id.is_empty() {
            return Ok(None);
        }
        let actor = ActorId::parse(&actor).map_err(|error| invalid_text_column(columns[2], error.to_string()))?;
        let event_id = EventId::parse(&event_id).map_err(|error| invalid_text_column(columns[4], error.to_string()))?;
        let hlc = Hlc { pt: nonnegative_u64(hlc_pt, columns[0], "HLC physical time")?, l: nonnegative_u64(hlc_l, columns[1], "HLC logical counter")? };
        let actor_seq = nonnegative_u64(actor_seq, columns[3], "actor sequence")?;
        Ok(Some(Self(VersionKey::new(hlc, conflict_rank, actor, actor_seq, event_id))))
    }

    fn key(&self) -> &VersionKey {
        &self.0
    }

    fn into_key(self) -> VersionKey {
        self.0
    }
}

fn read_progress_column(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<ReadProgress> {
    let value = row.get::<_, Option<f32>>(column)?.unwrap_or(0.0);
    ReadProgress::new(value).map_err(|error| rusqlite::Error::FromSqlConversionFailure(column, Type::Real, Box::new(error)))
}

fn unix_micros() -> Result<i64, rusqlite::Error> {
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_micros();
    i64::try_from(micros).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn unix_millis() -> Result<u64, rusqlite::Error> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_millis();
    u64::try_from(millis).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn unix_secs() -> Result<i64, rusqlite::Error> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_secs();
    i64::try_from(seconds).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn reset_development_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    let drop_stmt = r#"
    PRAGMA foreign_keys = OFF;

    DROP TRIGGER IF EXISTS trig_dir_added_insert;
    DROP TRIGGER IF EXISTS trig_dir_added_reactivate;
    DROP TRIGGER IF EXISTS trig_dir_moved;
    DROP TRIGGER IF EXISTS trig_dir_deleted;
    DROP TRIGGER IF EXISTS trig_dir_state_insert;
    DROP TRIGGER IF EXISTS trig_dir_state_update;
    DROP TRIGGER IF EXISTS trig_book_added;
    DROP TRIGGER IF EXISTS trig_book_added_reactivate;
    DROP TRIGGER IF EXISTS trig_book_deleted;
    DROP TRIGGER IF EXISTS trig_book_dir_added_insert;
    DROP TRIGGER IF EXISTS trig_book_dir_added_reactivate;
    DROP TRIGGER IF EXISTS trig_book_dir_deleted;
    DROP TRIGGER IF EXISTS trig_book_read_pos;
    DROP TRIGGER IF EXISTS trig_book_last_read;
    DROP TRIGGER IF EXISTS trig_book_metadata;
    DROP TRIGGER IF EXISTS trig_event_log_hlc;

    DROP VIEW IF EXISTS dir_paths;

    PRAGMA foreign_keys = ON;
    "#;

    conn.execute_batch(drop_stmt)?;
    Ok(())
}

fn execute_idempotent_migration(conn: &Connection, sql: &str, expected_error: &str) -> Result<(), rusqlite::Error> {
    match conn.execute(sql, ()) {
        Ok(_) => Ok(()),
        Err(error) if error.to_string().contains(expected_error) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Rebuilds `book_dir` onto its path-based primary key
/// `(dir_id, book_id, file_name)` for databases created when `local_hash` was
/// the key. SQLite can't alter a primary key in place, so the table is recreated
/// and its rows copied. No-op once migrated (detected from the stored DDL).
fn migrate_book_dir_primary_key(conn: &Connection) -> Result<(), rusqlite::Error> {
    let ddl: String = conn.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='book_dir'", [], |row| row.get(0))?;
    // The old schema's only "(local_hash)" occurrence is its PRIMARY KEY clause;
    // the new schema doesn't mention it (local_hash is a plain column).
    let needs_migration = ddl.contains("(local_hash)");
    if !needs_migration {
        return Ok(());
    }

    // PRAGMA foreign_keys only takes effect outside a transaction, so toggle it
    // around an explicit one. Nothing references book_dir, so the drop is safe.
    conn.execute_batch(
        "PRAGMA foreign_keys=OFF;
         BEGIN;
         CREATE TABLE book_dir_migrated(
             dir_id          TEXT not null references dir(id) on delete cascade,
             book_id         TEXT not null references book(id) on delete cascade,
             file_name       TEXT not null,
             local_hash      TEXT not null,
             content_hash    TEXT not null default '',
             is_downloaded   INTEGER not null default 0,
             last_scan       INTEGER not null default 0,
             hlc_pt          INTEGER not null default 0,
             hlc_l           INTEGER not null default 0,
             hlc_actor       TEXT not null default '',
             hlc_seq         INTEGER not null default 0,
             hlc_event       TEXT not null default '',
             intent_deleted  INTEGER,
             deleted_at      INTEGER,
             PRIMARY KEY     (dir_id, book_id, file_name)
         );
         INSERT OR IGNORE INTO book_dir_migrated (dir_id, book_id, file_name, local_hash, content_hash, is_downloaded, last_scan, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, deleted_at)
             SELECT dir_id, book_id, file_name, local_hash, content_hash, is_downloaded, last_scan, hlc_pt, hlc_l, hlc_actor, hlc_seq, '', deleted_at FROM book_dir;
         DROP TABLE book_dir;
         ALTER TABLE book_dir_migrated RENAME TO book_dir;
         COMMIT;
         PRAGMA foreign_keys=ON;",
    )?;
    Ok(())
}

fn migrate_asset_state(conn: &Connection) -> Result<(), rusqlite::Error> {
    let legacy_blob_state_exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='blob_state')", [], |row| row.get(0))?;
    if legacy_blob_state_exists {
        conn.execute_batch(
            "BEGIN;
             INSERT OR IGNORE INTO asset_remote_state(kind, hash, presence, last_checked, last_error)
             SELECT kind, hash, CASE WHEN state IN (2, 3) THEN 2 ELSE 0 END, last_checked, last_error
             FROM blob_state;
             DROP TABLE blob_state;
             COMMIT;",
        )?;
    }

    if let Some(raw) = get_sync_metadata(conn, "pending_download_hashes")? {
        let hashes = serde_json::from_str::<Vec<ContentHash>>(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let now = now_ts_secs()?;
        for hash in hashes {
            conn.add_asset_request(BlobKind::Book.storage(), hash.as_str(), TransferOrigin::Background.storage(), now)?;
        }
        conn.execute("DELETE FROM sync_metadata WHERE key='pending_download_hashes'", [])?;
    }
    Ok(())
}

fn repair_legacy_versions(conn: &Connection, table: &str, key_expression: &str) -> Result<(), rusqlite::Error> {
    let query = format!(
        "SELECT rowid, {key_expression}, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event
         FROM {table}
         WHERE NOT (hlc_pt=0 AND hlc_l=0 AND hlc_actor='' AND hlc_seq=0 AND hlc_event='')
           AND (hlc_actor='' OR hlc_event='' OR hlc_event='00000000-0000-0000-0000-000000000000')"
    );
    let rows = {
        let mut statement = conn.prepare(&query)?;
        let mapped = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let update = format!("UPDATE {table} SET hlc_actor=?1, hlc_event=?2 WHERE rowid=?3");
    for (rowid, key, hlc_pt, hlc_l, actor, actor_seq) in rows {
        let actor = if actor.is_empty() { "legacy" } else { actor.as_str() };
        let digest = blake3::hash(format!("bokheim-legacy-version:{table}:{key}:{hlc_pt}:{hlc_l}:{actor}:{actor_seq}").as_bytes());
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&digest.as_bytes()[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        conn.execute(&update, rusqlite::params![actor, Uuid::from_bytes(bytes).to_string(), rowid])?;
    }
    Ok(())
}

pub fn init_db(conn: &Connection) -> Result<ClientId, rusqlite::Error> {
    reset_development_schema(conn)?;

    conn.create_author_table()?;
    conn.create_book_table()?;
    conn.create_book_search_document_table()?;
    conn.create_book_search_bigram_table()?;
    conn.create_book_search_bigram_index()?;
    // Migration: complete LWW version for the book's live/deleted state.
    execute_idempotent_migration(conn, "ALTER TABLE book ADD COLUMN hlc_pt INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book ADD COLUMN hlc_l INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book ADD COLUMN hlc_actor TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book ADD COLUMN hlc_seq INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book ADD COLUMN hlc_event TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    conn.create_dir_table()?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN hlc_pt INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN hlc_l INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN hlc_actor TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN hlc_seq INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN hlc_event TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN intent_parent_id TEXT", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN intent_name TEXT", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE dir ADD COLUMN intent_deleted INTEGER", "duplicate column name")?;
    conn.execute("UPDATE dir SET intent_parent_id=parent_id, intent_name=name, intent_deleted=(deleted_at IS NOT NULL) WHERE intent_parent_id IS NULL OR intent_name IS NULL OR intent_deleted IS NULL", ())?;
    conn.insert_root()?;
    // Repair legacy duplicate sibling projections before enforcing the physical
    // path invariant. Stable id ordering makes the migration deterministic.
    conn.execute_batch(
        "WITH ranked AS (
             SELECT id, row_number() OVER (PARTITION BY parent_id, name ORDER BY id) AS position
             FROM dir WHERE deleted_at IS NULL AND id != '__root__'
         )
         UPDATE dir SET deleted_at = 1 WHERE id IN (SELECT id FROM ranked WHERE position > 1);",
    )?;
    conn.create_live_dir_path_index()?;
    conn.create_book_dir_table()?;
    // Migration: older databases stored the (non-content) local fingerprint as
    // the blob key. Add the portable content-hash column; it backfills lazily on
    // the next scan when each file is re-hashed.
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    // Migration: scan-generation stamp used to reconcile vanished files.
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN last_scan INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    // Migration: per-placement HLC making book_dir add/remove a LWW register.
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN hlc_pt INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN hlc_l INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN hlc_actor TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN hlc_seq INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN hlc_event TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book_dir ADD COLUMN intent_deleted INTEGER", "duplicate column name")?;
    // Migration: older databases keyed book_dir on local_hash, collapsing the
    // same content placed in multiple folders. Rebuild onto the path-based key.
    migrate_book_dir_primary_key(conn)?;
    conn.execute("UPDATE book_dir SET intent_deleted=(deleted_at IS NOT NULL) WHERE intent_deleted IS NULL", ())?;
    repair_legacy_versions(conn, "book", "id")?;
    repair_legacy_versions(conn, "dir", "id")?;
    repair_legacy_versions(conn, "book_dir", "dir_id || ':' || book_id || ':' || file_name")?;
    conn.execute_batch(
        "WITH ranked AS (
             SELECT rowid, row_number() OVER (
                 PARTITION BY dir_id, file_name
                 ORDER BY hlc_pt DESC, hlc_l DESC, (deleted_at IS NOT NULL) DESC, hlc_actor DESC, hlc_seq DESC, book_id DESC
             ) AS position
             FROM book_dir WHERE deleted_at IS NULL
         )
         UPDATE book_dir SET deleted_at = 1 WHERE rowid IN (SELECT rowid FROM ranked WHERE position > 1);",
    )?;
    conn.create_book_dir_local_hash_index()?;
    conn.create_live_book_path_index()?;
    conn.create_event_log_table()?;
    execute_idempotent_migration(conn, "ALTER TABLE event_log ADD COLUMN event_id TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    conn.execute(
        "UPDATE event_log SET event_id = lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(6))) WHERE event_id = ''",
        (),
    )?;
    conn.create_event_log_event_id_index()?;
    // Migration: HLC columns were added after event_log already existed in some
    // development/test databases. Add them before any index or trigger uses them.
    execute_idempotent_migration(conn, "ALTER TABLE event_log ADD COLUMN hlc_pt INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE event_log ADD COLUMN hlc_l INTEGER NOT NULL DEFAULT 0", "duplicate column name")?;
    conn.create_event_log_singleton_guard_index()?;
    // Migration: older databases had an unused `event_kind NOT NULL` column on
    // event_log. It is no longer populated by the triggers, so drop it if present.
    execute_idempotent_migration(conn, "ALTER TABLE event_log DROP COLUMN event_kind", "no such column")?;
    conn.execute("DROP TABLE IF EXISTS applied_remote_change", ())?;
    conn.execute("DROP TABLE IF EXISTS local_sync_rejection", ())?;
    conn.execute("DROP TABLE IF EXISTS local_sync_pending_dependency", ())?;
    conn.create_sync_metadata_table()?;
    conn.create_transfer_jobs_table()?;
    execute_idempotent_migration(conn, "ALTER TABLE transfer_jobs ADD COLUMN claim_token TEXT NOT NULL DEFAULT ''", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE transfer_jobs ADD COLUMN origin TEXT NOT NULL DEFAULT 'background' CHECK(origin IN ('background', 'user_initiated'))", "duplicate column name")?;
    conn.execute("UPDATE transfer_jobs SET claimed_at=NULL WHERE claimed_at IS NOT NULL AND claim_token=''", [])?;
    conn.create_pending_fs_operations_table()?;
    conn.create_asset_remote_state_table()?;
    conn.create_blob_local_ref_table()?;
    conn.create_asset_requests_table()?;
    conn.create_book_download_requests_table()?;
    execute_idempotent_migration(conn, "ALTER TABLE asset_requests ADD COLUMN origin TEXT NOT NULL DEFAULT 'background' CHECK(origin IN ('background', 'user_initiated'))", "duplicate column name")?;
    execute_idempotent_migration(conn, "ALTER TABLE book_download_requests ADD COLUMN origin TEXT NOT NULL DEFAULT 'user_initiated' CHECK(origin IN ('background', 'user_initiated'))", "duplicate column name")?;
    backfill_missing_book_search_index(conn)?;
    migrate_asset_state(conn)?;
    conn.create_views()?;
    conn.execute_batch(include_str!("database/triggers.sql"))?;
    conn.execute("DROP TABLE IF EXISTS pending_remote_change", ())?;
    conn.execute("DELETE FROM sync_metadata WHERE key = 'pending_remote_changes'", ())?;
    conn.set_sync_metadata("change_origin", "local")?;
    if get_sync_metadata(conn, "hlc_pt")?.is_none() {
        conn.set_sync_metadata("hlc_pt", "0")?;
    }
    if get_sync_metadata(conn, "hlc_l")?.is_none() {
        conn.set_sync_metadata("hlc_l", "0")?;
    }

    // Enable WAL mode AFTER creating all tables
    // WAL mode is persistent across connections once set
    log::info!("Setting WAL mode and busy timeout");
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", "10000")?;

    // Verify it was applied
    let journal_mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    let busy_timeout: i32 = conn.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    log::info!("Database configured: journal_mode={}, busy_timeout={}", journal_mode, busy_timeout);

    get_or_create_client_id(conn)
}

pub fn get_or_create_client_id(db: &Connection) -> Result<ClientId, rusqlite::Error> {
    let mut client_id: Option<String> = None;
    db.get_sync_metadata_value("client_id", |row| {
        client_id = Some(row.get(0)?);
        Ok(())
    })?;

    match client_id {
        Some(id) => ClientId::parse(&id).map_err(|error| invalid_text_column(0, error.to_string())),
        None => {
            let new_id = Uuid::new_v4().to_string();
            db.set_sync_metadata("client_id", &new_id)?;
            ClientId::parse(&new_id).map_err(|error| invalid_text_column(0, error.to_string()))
        }
    }
}

pub fn get_dir_contents(conn: &Connection, dir_path: &str) -> Result<(Vec<Dir>, Vec<Book>), rusqlite::Error> {
    let mut books: Vec<Book> = Vec::new();
    let mut sub_dirs: Vec<Dir> = Vec::new();

    conn.get_sub_dirs(dir_path, |row| {
        let raw_id: String = row.get(0)?;
        let raw_name: String = row.get(1)?;
        let raw_book_count: i64 = row.get(2)?;
        let raw_downloaded_book_count: i64 = row.get(3)?;
        let id = DirId::parse(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let name = FileName::parse(&raw_name).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let book_count = usize::try_from(raw_book_count).map_err(|_| invalid_integer_column(2, "folder book count must not be negative"))?;
        let downloaded_book_count = usize::try_from(raw_downloaded_book_count).map_err(|_| invalid_integer_column(3, "downloaded folder book count must not be negative"))?;
        sub_dirs.push(Dir::new(id, name, book_count, downloaded_book_count));
        Ok(())
    })?;

    conn.get_books_in_dir(dir_path, |row| {
        let book_id: String = row.get(0)?;
        let parsed_book_id = BookId::parse(&book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let title: Option<String> = row.get(1).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(2).ok();
        books.push(Book::try_new(parsed_book_id, title, author.unwrap_or_default()).map_err(|error| invalid_text_column(1, error.to_string()))?);
        Ok(())
    })?;
    Ok((sub_dirs, books))
}

pub fn get_descendant_book_ids(conn: &Connection, dir_id: &str) -> Result<Vec<BookId>, rusqlite::Error> {
    let mut book_ids = Vec::new();
    conn.get_descendant_book_ids(dir_id, |row| {
        let raw: String = row.get(0)?;
        book_ids.push(BookId::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?);
        Ok(())
    })?;
    Ok(book_ids)
}

pub fn get_folder_download_counts(conn: &Connection, dir_id: &str) -> Result<(usize, usize), rusqlite::Error> {
    let mut counts = (0, 0);
    conn.get_folder_download_counts(dir_id, |row| {
        let total = row.get::<_, i64>(0)?;
        let downloaded = row.get::<_, i64>(1)?;
        counts = (usize::try_from(downloaded).map_err(|_| invalid_integer_column(1, "downloaded book count must not be negative"))?, usize::try_from(total).map_err(|_| invalid_integer_column(0, "folder book count must not be negative"))?);
        Ok(())
    })?;
    Ok(counts)
}

pub fn get_book_ancestor_dir_ids(conn: &Connection, book_id: BookId) -> Result<Vec<DirId>, rusqlite::Error> {
    let mut dir_ids = Vec::new();
    conn.get_book_ancestor_dir_ids(&book_id.canonical(), |row| {
        let raw: String = row.get(0)?;
        dir_ids.push(DirId::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?);
        Ok(())
    })?;
    Ok(dir_ids)
}

pub fn get_recently_read(conn: &Connection, limit: i32) -> Result<Vec<Book>, rusqlite::Error> {
    let mut books: Vec<Book> = Vec::new();

    conn.get_recently_read(limit, |row| {
        let book_id: String = row.get(0)?;
        let parsed_book_id = BookId::parse(&book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let title: Option<String> = row.get(1).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(2).ok();
        let read_progress = read_progress_column(row, 3)?;
        let last_read = nonnegative_i64(row.get::<_, Option<i64>>(4)?.unwrap_or(0), 4, "last-read timestamp")?;
        let mut book = Book::try_new(parsed_book_id, title, author.unwrap_or_default()).map_err(|error| invalid_text_column(1, error.to_string()))?;
        book.set_read_progress(read_progress);
        book.set_last_read(NonNegativeTimestamp::new(last_read).map_err(|error| invalid_integer_column(4, error.to_string()))?);
        books.push(book);
        Ok(())
    })?;
    Ok(books)
}

pub fn get_recently_added(conn: &Connection, limit: i32) -> Result<Vec<Book>, rusqlite::Error> {
    let mut books: Vec<Book> = Vec::new();
    conn.get_recently_added(limit, |row| {
        let book_id: String = row.get(0)?;
        let parsed_book_id = BookId::parse(&book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let title: Option<String> = row.get(1).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(2).ok();
        let read_progress = read_progress_column(row, 3)?;
        let added_at = nonnegative_i64(row.get::<_, Option<i64>>(4)?.unwrap_or(0), 4, "added timestamp")?;
        let mut book = Book::try_new(parsed_book_id, title, author.unwrap_or_default()).map_err(|error| invalid_text_column(1, error.to_string()))?;
        book.set_read_progress(read_progress);
        book.set_added_at(NonNegativeTimestamp::new(added_at).map_err(|error| invalid_integer_column(4, error.to_string()))?);
        books.push(book);
        Ok(())
    })?;
    Ok(books)
}

fn normalized_book_search_text(title: &str, author: &str) -> String {
    format!("{}\n{}", title.trim(), author.trim()).to_lowercase()
}

fn search_bigrams(text: &str) -> BTreeSet<String> {
    let characters = text.chars().collect::<Vec<_>>();
    characters.windows(2).map(|pair| pair.iter().collect::<String>()).collect()
}

fn write_book_search_index(conn: &Connection, book_id: &str, title: &str, author: &str) -> Result<(), rusqlite::Error> {
    let normalized_text = normalized_book_search_text(title, author);
    conn.execute(
        "INSERT INTO book_search_document(book_id, normalized_text) VALUES (?1, ?2)
         ON CONFLICT(book_id) DO UPDATE SET normalized_text=excluded.normalized_text",
        rusqlite::params![book_id, normalized_text],
    )?;
    conn.execute("DELETE FROM book_search_bigram WHERE book_id=?1", [book_id])?;
    let mut insert = conn.prepare_cached("INSERT OR IGNORE INTO book_search_bigram(book_id, gram) VALUES (?1, ?2)")?;
    for gram in search_bigrams(&normalized_text) {
        insert.execute(rusqlite::params![book_id, gram])?;
    }
    Ok(())
}

fn backfill_missing_book_search_index(conn: &Connection) -> Result<(), rusqlite::Error> {
    let books = {
        let mut statement = conn.prepare(
            "SELECT b.id, COALESCE(b.title, ''), COALESCE(a.name, '')
             FROM book b
             LEFT JOIN author a ON a.id=b.author_id
             LEFT JOIN book_search_document d ON d.book_id=b.id
             WHERE d.book_id IS NULL
                OR (length(d.normalized_text) >= 2 AND NOT EXISTS (
                    SELECT 1 FROM book_search_bigram g WHERE g.book_id=b.id
                ))",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows
    };

    for (book_id, title, author) in books {
        write_book_search_index(conn, &book_id, &title, &author)?;
    }
    Ok(())
}

fn decode_search_book(row: &rusqlite::Row<'_>) -> Result<Book, rusqlite::Error> {
    let id: String = row.get(0)?;
    let book_id = BookId::parse(&id).map_err(|error| invalid_text_column(0, error.to_string()))?;
    let mut book = Book::try_new(book_id, row.get(1)?, row.get(2)?).map_err(|error| invalid_text_column(1, error.to_string()))?;
    book.set_read_progress(read_progress_column(row, 3)?);
    book.set_last_read(NonNegativeTimestamp::new(nonnegative_i64(row.get(4)?, 4, "last-read timestamp")?).map_err(|error| invalid_integer_column(4, error.to_string()))?);
    book.set_added_at(NonNegativeTimestamp::new(nonnegative_i64(row.get(5)?, 5, "added timestamp")?).map_err(|error| invalid_integer_column(5, error.to_string()))?);
    Ok(book)
}

pub fn search_books(conn: &Connection, query: &str, limit: usize) -> Result<Vec<Book>, rusqlite::Error> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = storage_i64(u64::try_from(limit).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?, "search limit")?;
    let grams = search_bigrams(&normalized_query);
    if grams.is_empty() {
        let mut statement = conn.prepare(
            "SELECT DISTINCT b.id, COALESCE(b.title, ''), COALESCE(a.name, ''), COALESCE(b.read_progress, 0), COALESCE(b.last_read, 0), COALESCE(b.added_at, 0)
             FROM book_search_document d
             JOIN book b ON b.id=d.book_id
             LEFT JOIN author a ON a.id=b.author_id
             JOIN book_dir bd ON bd.book_id=b.id
             WHERE b.deleted_at IS NULL AND bd.deleted_at IS NULL
               AND instr(d.normalized_text, ?1) > 0
             ORDER BY LOWER(COALESCE(b.title, '')), LOWER(COALESCE(a.name, ''))
             LIMIT ?2",
        )?;
        let rows = statement.query_map(rusqlite::params![normalized_query, limit], decode_search_book)?;
        return rows.collect();
    }

    let placeholders = std::iter::repeat_n("?", grams.len()).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "WITH candidates AS (
             SELECT book_id
             FROM book_search_bigram INDEXED BY idx_book_search_bigram_gram
             WHERE gram IN ({placeholders})
             GROUP BY book_id
             HAVING COUNT(DISTINCT gram) = ?
         )
         SELECT DISTINCT b.id, COALESCE(b.title, ''), COALESCE(a.name, ''), COALESCE(b.read_progress, 0), COALESCE(b.last_read, 0), COALESCE(b.added_at, 0)
         FROM candidates c
         JOIN book_search_document d ON d.book_id=c.book_id
         JOIN book b ON b.id=c.book_id
         LEFT JOIN author a ON a.id=b.author_id
         JOIN book_dir bd ON bd.book_id=b.id
         WHERE b.deleted_at IS NULL AND bd.deleted_at IS NULL
           AND instr(d.normalized_text, ?) > 0
         ORDER BY LOWER(COALESCE(b.title, '')), LOWER(COALESCE(a.name, ''))
         LIMIT ?",
    );
    let mut parameters = grams.into_iter().map(Value::Text).collect::<Vec<_>>();
    parameters.push(Value::Integer(i64::try_from(parameters.len()).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?));
    parameters.push(Value::Text(normalized_query));
    parameters.push(Value::Integer(limit));
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(parameters.iter()), decode_search_book)?;
    rows.collect()
}

pub fn get_authors(conn: &Connection) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    let mut authors = Vec::new();
    conn.get_authors(|row| {
        let name: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        let count = usize::try_from(count).map_err(|_| invalid_integer_column(1, "author book count must not be negative"))?;
        authors.push((name, count));
        Ok(())
    })?;
    Ok(authors)
}

pub fn get_author_books_grouped(conn: &Connection, author_name: &str) -> Result<Vec<(DirId, String, Vec<Book>)>, rusqlite::Error> {
    let mut grouped: HashMap<DirId, (String, Vec<Book>)> = HashMap::new();
    conn.get_author_books(author_name, |row| {
        let raw_dir_id: String = row.get(0)?;
        let dir_id = DirId::parse(&raw_dir_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let folder_path: String = row.get(1)?;
        let book_id: String = row.get(2)?;
        let parsed_book_id = BookId::parse(&book_id).map_err(|error| invalid_text_column(2, error.to_string()))?;
        let title: Option<String> = row.get(3).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(4).ok();
        let book = Book::try_new(parsed_book_id, title, author.unwrap_or_default()).map_err(|error| invalid_text_column(3, error.to_string()))?;
        grouped.entry(dir_id).or_insert_with(|| (folder_path, Vec::new())).1.push(book);
        Ok(())
    })?;

    Ok(grouped.into_iter().map(|(dir_id, (folder_path, books))| (dir_id, folder_path, books)).collect())
}

pub fn get_book_file_path(conn: &Connection, book_id: BookId) -> Result<Option<RelativeBookPath>, rusqlite::Error> {
    let mut file_path: Option<String> = None;

    conn.get_book_file_path(&book_id.canonical(), |row| {
        file_path = Some(row.get(0)?);
        Ok(())
    })?;
    file_path.map(|path| RelativeBookPath::parse(&path).map_err(|error| invalid_text_column(0, error.to_string()))).transpose()
}

pub fn get_book_reading_position(conn: &Connection, book_id: BookId) -> Result<Option<EpubCfi>, rusqlite::Error> {
    let mut position = None;
    conn.get_book_reading_position(&book_id.canonical(), |row| {
        let raw: Option<String> = row.get(0)?;
        if let Some(raw) = raw.filter(|value| value != "0" && !value.is_empty()) {
            if sync_common::pdf_page_from_reading_position(&raw).is_none() {
                position = Some(EpubCfi::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?);
            }
        }
        Ok(())
    })?;
    Ok(position)
}

pub fn get_pdf_reading_page(conn: &Connection, book_id: BookId) -> Result<Option<u32>, rusqlite::Error> {
    let mut position = None;
    conn.get_book_reading_position(&book_id.canonical(), |row| {
        let raw: Option<String> = row.get(0)?;
        if let Some(raw) = raw.filter(|value| value != "0" && !value.is_empty()) {
            position = sync_common::pdf_page_from_reading_position(&raw);
            if position.is_none() && !sync_common::valid_epub_cfi(&raw) {
                return Err(invalid_text_column(0, "invalid reading position"));
            }
        }
        Ok(())
    })?;
    Ok(position)
}

pub fn is_book_downloaded(conn: &Connection, book_id: BookId) -> Result<bool, rusqlite::Error> {
    let mut downloaded = false;
    conn.is_book_downloaded(&book_id.canonical(), |row| {
        downloaded = row.get::<_, bool>(0)?;
        Ok(())
    })?;
    Ok(downloaded)
}

pub fn is_book_download_requested(conn: &Connection, book_id: BookId) -> Result<bool, rusqlite::Error> {
    let mut requested = false;
    conn.is_book_download_requested(&book_id.canonical(), |row| {
        requested = row.get::<_, bool>(0)?;
        Ok(())
    })?;
    Ok(requested)
}

pub fn get_book_download_states(conn: &Connection, book_ids: &[BookId]) -> Result<HashMap<BookId, (bool, bool)>, rusqlite::Error> {
    if book_ids.is_empty() {
        return Ok(HashMap::new());
    }
    const MAX_IDS_PER_QUERY: usize = 500;
    if book_ids.len() > MAX_IDS_PER_QUERY {
        let mut states = HashMap::with_capacity(book_ids.len());
        for chunk in book_ids.chunks(MAX_IDS_PER_QUERY) {
            states.extend(get_book_download_states(conn, chunk)?);
        }
        return Ok(states);
    }
    let placeholders = std::iter::repeat_n("?", book_ids.len()).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT candidate.id,
                EXISTS (
                    SELECT 1 FROM book_dir placement
                    WHERE placement.book_id = candidate.id
                      AND placement.deleted_at IS NULL
                      AND placement.is_downloaded = 1
                ) AS downloaded,
                EXISTS (
                    SELECT 1 FROM book_download_requests request
                    WHERE request.book_id = candidate.id
                ) OR EXISTS (
                    SELECT 1
                    FROM book_dir placement
                    JOIN asset_requests request
                      ON request.kind = 'book'
                     AND request.hash = placement.content_hash
                    WHERE placement.book_id = candidate.id
                      AND placement.deleted_at IS NULL
                ) AS requested
         FROM book candidate
         WHERE candidate.id IN ({placeholders})"
    );
    let canonical_ids = book_ids.iter().copied().map(BookId::canonical).collect::<Vec<_>>();
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(canonical_ids.iter()), |row| {
        let raw: String = row.get(0)?;
        let book_id = BookId::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?;
        Ok((book_id, (row.get(1)?, row.get(2)?)))
    })?;
    rows.collect()
}

#[derive(Debug, Clone)]
pub struct BookAsset {
    pub book_id: BookId,
    /// Content-addressed blob key (full-file blake3 hex).
    pub content_hash: ContentHash,
    pub rel_path: RelativeBookPath,
}

fn parse_content_hash_column(raw: &str, column: usize) -> Result<ContentHash, rusqlite::Error> {
    ContentHash::parse(raw).map_err(|error| invalid_text_column(column, error.to_string()))
}

pub fn get_book_assets(conn: &Connection) -> Result<Vec<BookAsset>, rusqlite::Error> {
    let mut assets = Vec::new();
    conn.get_book_assets(|row| {
        let raw_book_id: String = row.get(0)?;
        let book_id = BookId::parse(&raw_book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_content_hash: String = row.get(1)?;
        let content_hash = parse_content_hash_column(&raw_content_hash, 1)?;
        let raw_rel_path: String = row.get(2)?;
        let rel_path = RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_text_column(2, error.to_string()))?;
        let _: i64 = row.get(3)?;
        assets.push(BookAsset { book_id, content_hash, rel_path });
        Ok(())
    })?;
    Ok(assets)
}

#[derive(Debug)]
pub struct ClaimedTransferJob {
    pub(crate) id: i64,
    pub(crate) attempts: i64,
    pub(crate) claim_token: String,
    pub(crate) job: TransferJob,
}

fn now_ts_secs() -> Result<i64, rusqlite::Error> {
    unix_secs()
}

pub fn upsert_transfer_job(conn: &Connection, kind: TransferJobKind, content_hash: &ContentHash, rel_path: &str, local_path: &str, book_id: Option<BookId>, origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    let book_id = book_id.map(BookId::canonical).unwrap_or_default();
    conn.upsert_transfer_job(kind.storage(), content_hash.as_str(), rel_path, local_path, &book_id, origin.storage(), now)?;
    Ok(())
}

pub fn claim_due_transfer_job(conn: &Connection, allow_background_transfers: bool) -> Result<Option<ClaimedTransferJob>, rusqlite::Error> {
    let now = now_ts_secs()?;
    let claim_token = Uuid::new_v4().to_string();
    let mut claimed_job = None;
    conn.claim_due_transfer_job(now, &claim_token, i32::from(allow_background_transfers), |row| {
        let id: i64 = row.get(0)?;
        let raw_kind: String = row.get(1)?;
        let kind = TransferJobKind::parse_storage(&raw_kind).ok_or_else(|| invalid_text_column(1, "invalid transfer job kind"))?;
        let raw_content_hash: String = row.get(2)?;
        let content_hash = parse_content_hash_column(&raw_content_hash, 2)?;
        let rel_path: String = row.get(3)?;
        let local_path: String = row.get(4)?;
        let raw_book_id: String = row.get(5)?;
        let raw_origin: String = row.get(6)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(6, "invalid transfer origin"))?;
        let job = TransferJob::from_storage(kind, content_hash, &rel_path, &local_path, &raw_book_id, origin).ok_or_else(|| invalid_text_column(3, "invalid transfer job payload"))?;
        claimed_job = Some(ClaimedTransferJob { id, attempts: row.get(7)?, claim_token: row.get(8)?, job });
        Ok(())
    })?;
    Ok(claimed_job)
}

pub fn complete_transfer_job(conn: &Connection, id: i64, claim_token: &str) -> Result<(), rusqlite::Error> {
    conn.complete_transfer_job(id, claim_token)?;
    Ok(())
}

pub fn get_next_transfer_retry_at(conn: &Connection) -> Result<Option<i64>, rusqlite::Error> {
    let mut next = None;
    conn.get_next_transfer_retry_at(|row| {
        next = row.get(0)?;
        Ok(())
    })?;
    Ok(next)
}

pub fn get_transfer_statuses(conn: &Connection) -> Result<Vec<TransferStatus>, rusqlite::Error> {
    let mut transfers = Vec::new();
    conn.get_transfer_statuses(|row| {
        let id: i64 = row.get(0)?;
        if id <= 0 {
            return Err(invalid_integer_column(0, "transfer id must be positive"));
        }
        let raw_kind: String = row.get(1)?;
        let kind = TransferJobKind::parse_storage(&raw_kind).ok_or_else(|| invalid_text_column(1, "invalid transfer job kind"))?;
        let raw_content_hash: String = row.get(2)?;
        let content_hash = parse_content_hash_column(&raw_content_hash, 2)?;
        let rel_path: String = row.get(3)?;
        let local_path: String = row.get(4)?;
        let raw_book_id: String = row.get(5)?;
        let book_id = if raw_book_id.is_empty() { None } else { Some(BookId::parse(&raw_book_id).map_err(|error| invalid_text_column(5, error.to_string()))?.canonical()) };
        let raw_origin: String = row.get(6)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(6, "invalid transfer origin"))?;
        let attempts = u32::try_from(row.get::<_, i64>(7)?).map_err(|_| invalid_integer_column(7, "transfer attempts must not be negative"))?;
        let claimed_at: Option<i64> = row.get(8)?;
        let next_retry_at = nonnegative_i64(row.get(9)?, 9, "next retry timestamp")?;
        let last_error: Option<String> = row.get(10)?;
        let created_at = nonnegative_i64(row.get(11)?, 11, "transfer creation timestamp")?;
        let updated_at = nonnegative_i64(row.get(12)?, 12, "transfer update timestamp")?;
        let state = if claimed_at.is_some() {
            TransferState::Running
        } else if last_error.is_some() {
            TransferState::Retrying
        } else {
            TransferState::Queued
        };
        let path = if !rel_path.is_empty() { Some(rel_path) } else { Path::new(&local_path).file_name().and_then(|name| name.to_str()).map(str::to_owned) };
        transfers.push(TransferStatus {
            id: id.to_string(),
            kind,
            content_hash: content_hash.as_str().to_owned(),
            book_id,
            path,
            state,
            origin,
            attempts,
            last_error,
            next_retry_at: (next_retry_at > 0).then(|| next_retry_at.to_string()),
            created_at: created_at.to_string(),
            updated_at: updated_at.to_string(),
        });
        Ok(())
    })?;
    Ok(transfers)
}

pub fn clear_transfer_jobs(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.clear_transfer_jobs()?;
    Ok(())
}

pub fn fail_transfer_job(conn: &Connection, id: i64, claim_token: &str, attempts: i64, error: &str) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    let backoff = (2_i64.pow((attempts as u32).min(8)) * 2).min(600);
    let jitter = (now % 3).abs();
    let next_retry_at = now + backoff + jitter;
    conn.fail_transfer_job(id, claim_token, error, next_retry_at, now)?;
    Ok(())
}

pub fn heartbeat_transfer_job(conn: &Connection, id: i64, claim_token: &str) -> Result<(), rusqlite::Error> {
    conn.heartbeat_transfer_job(id, claim_token, now_ts_secs()?)?;
    Ok(())
}

pub fn reset_stale_claims(conn: &Connection, stale_after_secs: i64) -> Result<(), rusqlite::Error> {
    let stale_before = now_ts_secs()?.saturating_sub(stale_after_secs.max(1));
    conn.reset_stale_claims(stale_before)?;
    Ok(())
}

pub fn prune_unclaimed_transfer_jobs(conn: &Connection, desired: &[TransferJob]) -> Result<(), rusqlite::Error> {
    let mut rows: Vec<(i64, TransferJob)> = Vec::new();
    conn.get_unclaimed_transfer_jobs(|row| {
        let id: i64 = row.get(0)?;
        let raw_kind: String = row.get(1)?;
        let kind = TransferJobKind::parse_storage(&raw_kind).ok_or_else(|| invalid_text_column(1, "invalid transfer job kind"))?;
        let raw_content_hash: String = row.get(2)?;
        let content_hash = parse_content_hash_column(&raw_content_hash, 2)?;
        let rel_path: String = row.get(3)?;
        let local_path: String = row.get(4)?;
        let raw_book_id: String = row.get(5)?;
        let raw_origin: String = row.get(6)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(6, "invalid transfer origin"))?;
        let job = TransferJob::from_storage(kind, content_hash, &rel_path, &local_path, &raw_book_id, origin).ok_or_else(|| invalid_text_column(3, "invalid transfer job payload"))?;
        rows.push((id, job));
        Ok(())
    })?;
    for (id, key) in rows {
        if !desired.iter().any(|desired| desired.same_persisted_key(&key)) {
            conn.delete_transfer_job_by_id(id)?;
        }
    }
    Ok(())
}

pub fn upsert_blob_local_ref(conn: &Connection, kind: BlobKind, content_hash: &ContentHash, path: &str, exists: bool) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    let exists_flag = if exists { 1 } else { 0 };
    conn.upsert_blob_local_ref(kind.storage(), content_hash.as_str(), path, exists_flag, now)?;
    Ok(())
}

pub fn get_locally_present_blob_hashes(conn: &Connection, kind: BlobKind) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut hashes = Vec::new();
    conn.get_locally_present_blob_hashes(kind.storage(), |row| {
        let raw_content_hash: String = row.get(0)?;
        hashes.push(parse_content_hash_column(&raw_content_hash, 0)?);
        Ok(())
    })?;
    Ok(hashes)
}

pub fn get_blob_local_path(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<Option<String>, rusqlite::Error> {
    let mut path: Option<String> = None;
    conn.get_blob_local_path(kind.storage(), content_hash.as_str(), |row| {
        path = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(path)
}

/// Resolves a tombstoned placement to `(library-relative path, content_hash)`,
/// or None if it is no longer tombstoned (an add resurrected it later in the same
/// pull batch, so its file must not be touched). The content hash lets the caller
/// relocate the file to a live placement of the same content (a move).
pub fn resolve_removed_book_dir(conn: &Connection, dir_id: &str, book_id: BookId, file_name: &str) -> Result<Option<(RelativeBookPath, Option<ContentHash>)>, rusqlite::Error> {
    let mut result = None;
    conn.resolve_removed_book_dir(dir_id, &book_id.canonical(), file_name, |row| {
        let raw_rel_path: String = row.get(0)?;
        let rel_path = RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_content_hash: String = row.get(1)?;
        let content_hash = if raw_content_hash.is_empty() { None } else { Some(parse_content_hash_column(&raw_content_hash, 1)?) };
        result = Some((rel_path, content_hash));
        Ok(())
    })?;
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookPath {
    pub book_id: BookId,
    pub rel_path: RelativeBookPath,
}

pub fn get_book_paths_by_content_hash(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<BookPath>, rusqlite::Error> {
    let mut paths = Vec::new();
    conn.get_book_paths_by_content_hash(content_hash.as_str(), |row| {
        let raw_book_id: String = row.get(0)?;
        let book_id = BookId::parse(&raw_book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_rel_path: String = row.get(1)?;
        let rel_path = RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_text_column(1, error.to_string()))?;
        paths.push(BookPath { book_id, rel_path });
        Ok(())
    })?;
    Ok(paths)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPathComponent {
    pub id: DirId,
    pub name: FileName,
}

fn collect_dir_path_components<F>(mut query: F) -> Result<Vec<DirPathComponent>, rusqlite::Error>
where
    F: FnMut(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>,
{
    let mut components = Vec::new();
    let mut row_fn = |row: &rusqlite::Row<'_>| {
        let raw_id: String = row.get(0)?;
        let raw_name: String = row.get(1)?;
        let id = DirId::parse(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let name = FileName::parse(&raw_name).map_err(|error| invalid_text_column(1, error.to_string()))?;
        components.push(DirPathComponent { id, name });
        Ok(())
    };
    query(&mut row_fn)?;
    Ok(components)
}

pub fn get_dir_path_components(conn: &Connection, dir_id: &str) -> Result<Vec<DirPathComponent>, rusqlite::Error> {
    collect_dir_path_components(|row_fn| conn.get_dir_path_components(dir_id, row_fn))
}

pub fn get_dir_path_components_tx(tx: &Transaction, dir_id: &str) -> Result<Vec<DirPathComponent>, rusqlite::Error> {
    collect_dir_path_components(|row_fn| tx.get_dir_path_components(dir_id, row_fn))
}

pub(crate) fn dir_path_from_components(components: &[DirPathComponent]) -> Result<Option<RelativeDirPath>, rusqlite::Error> {
    if components.is_empty() {
        return Ok(None);
    }
    RelativeDirPath::from_components(components.iter().map(|component| component.name.as_str())).map(Some).map_err(|error| invalid_text_column(1, error.to_string()))
}

pub(crate) fn get_dir_rel_path_tx(tx: &Transaction, dir_id: &str) -> Result<Option<RelativeDirPath>, rusqlite::Error> {
    let components = get_dir_path_components_tx(tx, dir_id)?;
    dir_path_from_components(&components)
}

pub(crate) fn get_live_dir_paths_tx(tx: &Transaction) -> Result<HashMap<DirId, RelativeDirPath>, rusqlite::Error> {
    let mut statement = tx.prepare(
        "WITH RECURSIVE paths(id, path) AS (
             SELECT id, '' FROM dir WHERE id='__root__' AND deleted_at IS NULL
             UNION ALL
             SELECT d.id, CASE WHEN paths.path='' THEN d.name ELSE paths.path || '/' || d.name END
             FROM dir d JOIN paths ON d.parent_id=paths.id
             WHERE d.id!='__root__' AND d.deleted_at IS NULL
         )
         SELECT id, path FROM paths WHERE id!='__root__'",
    )?;
    let mut rows = statement.query([])?;
    let mut paths = HashMap::new();
    while let Some(row) = rows.next()? {
        let id = DirId::parse(&row.get::<_, String>(0)?).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let path = RelativeDirPath::parse(&row.get::<_, String>(1)?).map_err(|error| invalid_text_column(1, error.to_string()))?;
        paths.insert(id, path);
    }
    Ok(paths)
}

pub fn materialize_dir_with_markers(conn: &Connection, library_path: &Path, dir_id: &str) -> std::io::Result<Option<PathBuf>> {
    let components = get_dir_path_components(conn, dir_id).map_err(std::io::Error::other)?;
    let root = LibraryRoot::open(library_path)?;
    materialize_dir_components_with_markers(&root, &components, true)
}

pub fn materialize_dir_parent_with_markers(conn: &Connection, library_path: &Path, dir_id: &str) -> std::io::Result<Option<PathBuf>> {
    let components = get_dir_path_components(conn, dir_id).map_err(std::io::Error::other)?;
    let root = LibraryRoot::open(library_path)?;
    materialize_dir_components_with_markers(&root, &components, false)
}

fn materialize_dir_components_with_markers(library_root: &LibraryRoot, components: &[DirPathComponent], include_leaf: bool) -> std::io::Result<Option<PathBuf>> {
    if components.is_empty() {
        return Ok(None);
    }
    let end = if include_leaf { components.len() } else { components.len().saturating_sub(1) };
    let path = if end == 0 {
        library_root.as_path().to_path_buf()
    } else {
        let relative = RelativeDirPath::from_components(components[..end].iter().map(|component| component.name.as_str())).map_err(std::io::Error::other)?;
        library_root.resolve_dir(&relative)
    };
    let mut current = library_root.as_path().to_path_buf();
    for component in &components[..end] {
        current.push(component.name.as_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("directory materialization crosses a symlink: {}", current.display()))),
            Ok(metadata) if !metadata.is_dir() => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("directory materialization path is not a directory: {}", current.display()))),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(error) => return Err(error),
        }
        let marker = current.join(".biblos_uuid");
        if !marker.exists() {
            fs::write(marker, component.id.as_str())?;
        } else {
            require_directory_marker(&current, &component.id)?;
        }
    }
    Ok(Some(path))
}

pub fn get_book_dir_entry_by_path(conn: &Connection, rel_path: &RelativeBookPath) -> Result<Option<(BookId, DirId, FileName)>, rusqlite::Error> {
    let mut entry = None;
    conn.get_book_dir_entry_by_path(rel_path.as_str(), |row| {
        let raw_book_id: String = row.get(0)?;
        let raw_dir_id: String = row.get(1)?;
        let raw_file_name: String = row.get(2)?;
        let book_id = BookId::parse(&raw_book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let dir_id = DirId::parse(&raw_dir_id).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let file_name = FileName::parse(&raw_file_name).map_err(|error| invalid_text_column(2, error.to_string()))?;
        entry = Some((book_id, dir_id, file_name));
        Ok(())
    })?;
    Ok(entry)
}

pub fn book_placement_expects_hash(conn: &Connection, placement: &BookPlacement, content_hash: &ContentHash) -> Result<bool, rusqlite::Error> {
    let Some((book_id, dir_id, file_name)) = get_book_dir_entry_by_path(conn, &placement.rel_path)? else {
        return Ok(false);
    };
    if book_id != placement.book_id {
        return Ok(false);
    }
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM book_dir
             WHERE dir_id=?1 AND book_id=?2 AND file_name=?3
               AND content_hash=?4 AND deleted_at IS NULL
         )",
        rusqlite::params![dir_id.as_str(), book_id.canonical(), file_name.as_str(), content_hash.as_str()],
        |row| row.get(0),
    )
}

pub fn local_file_fingerprint(path: &Path) -> std::io::Result<u64> {
    let mut hasher = Xxh3::new();
    let metadata = fs::metadata(path)?;
    let modified_time = metadata.modified()?.duration_since(UNIX_EPOCH).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?.as_secs();
    hasher.write_u64(modified_time);
    hasher.write_u64(metadata.size());
    hasher.write_u64(metadata.ino());
    hasher.write_u64(metadata.dev());
    Ok(hasher.digest())
}

pub fn stamp_book_dir_local_hash(conn: &Connection, dir_id: &str, book_id: BookId, file_name: &str, path: &Path) -> std::io::Result<()> {
    let local_hash = local_file_fingerprint(path)?.to_string();
    conn.update_book_dir_local_hash(dir_id, &book_id.canonical(), file_name, &local_hash).map_err(std::io::Error::other)?;
    Ok(())
}

pub fn stamp_book_dir_local_hash_by_rel_path(conn: &Connection, rel_path: &RelativeBookPath, book_id: BookId, abs_path: &Path) -> std::io::Result<()> {
    let Some((found_book_id, dir_id, file_name)) = get_book_dir_entry_by_path(conn, rel_path).map_err(std::io::Error::other)? else {
        return Ok(());
    };
    if found_book_id != book_id {
        return Ok(());
    }
    stamp_book_dir_local_hash(conn, dir_id.as_str(), book_id, file_name.as_str(), abs_path)
}

pub fn get_book_hashes(conn: &Connection, book_id: BookId) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut hashes = Vec::new();
    conn.get_book_hashes(&book_id.canonical(), |row| {
        let raw_content_hash: String = row.get(0)?;
        hashes.push(parse_content_hash_column(&raw_content_hash, 0)?);
        Ok(())
    })?;
    Ok(hashes)
}

pub fn is_blob_local(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<bool, rusqlite::Error> {
    let mut exists = 0_i32;
    conn.get_blob_local_presence(kind.storage(), content_hash.as_str(), |row| {
        exists = row.get::<_, i32>(0)?;
        Ok(())
    })?;
    Ok(exists > 0)
}

pub fn mark_asset_remote_present(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    conn.mark_asset_remote_present(kind.storage(), content_hash.as_str(), now)?;
    Ok(())
}

pub fn mark_asset_remote_missing(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    conn.mark_asset_remote_missing(kind.storage(), content_hash.as_str(), now)?;
    Ok(())
}

pub fn set_asset_remote_error(conn: &Connection, kind: BlobKind, content_hash: &ContentHash, error: &str) -> Result<(), rusqlite::Error> {
    let now = now_ts_secs()?;
    conn.set_asset_remote_error(kind.storage(), content_hash.as_str(), error, now)?;
    Ok(())
}

pub fn get_asset_requests(conn: &Connection, kind: BlobKind) -> Result<Vec<(ContentHash, TransferOrigin)>, rusqlite::Error> {
    let mut hashes = Vec::new();
    conn.get_asset_requests(kind.storage(), |row| {
        let raw: String = row.get(0)?;
        let raw_origin: String = row.get(1)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(1, "invalid transfer origin"))?;
        hashes.push((parse_content_hash_column(&raw, 0)?, origin));
        Ok(())
    })?;
    Ok(hashes)
}

pub fn add_asset_request(conn: &Connection, kind: BlobKind, hash: &ContentHash, origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    conn.add_asset_request(kind.storage(), hash.as_str(), origin.storage(), now_ts_secs()?)?;
    Ok(())
}

pub fn complete_asset_request(conn: &Connection, kind: BlobKind, hash: &ContentHash) -> Result<(), rusqlite::Error> {
    conn.complete_asset_request(kind.storage(), hash.as_str())?;
    Ok(())
}

pub fn add_book_download_request(conn: &Connection, book_id: BookId, origin: TransferOrigin) -> Result<(), rusqlite::Error> {
    conn.add_book_download_request(&book_id.canonical(), origin.storage(), now_ts_secs()?)?;
    Ok(())
}

pub fn get_book_download_requests(conn: &Connection) -> Result<Vec<(BookId, TransferOrigin)>, rusqlite::Error> {
    let mut books = Vec::new();
    conn.get_book_download_requests(|row| {
        let raw: String = row.get(0)?;
        let raw_origin: String = row.get(1)?;
        let origin = TransferOrigin::parse_storage(&raw_origin).ok_or_else(|| invalid_text_column(1, "invalid transfer origin"))?;
        books.push((BookId::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?, origin));
        Ok(())
    })?;
    Ok(books)
}

pub fn complete_book_download_request(conn: &Connection, book_id: BookId) -> Result<(), rusqlite::Error> {
    conn.complete_book_download_request(&book_id.canonical())?;
    Ok(())
}

pub fn update_reading_position(conn: &Connection, book_id: BookId, cfi: &EpubCfi) -> Result<(), rusqlite::Error> {
    conn.update_reading_position(&book_id.canonical(), cfi.as_str())?;
    Ok(())
}

pub fn update_pdf_reading_page(conn: &Connection, book_id: BookId, page_index: u32) -> Result<(), rusqlite::Error> {
    let position = sync_common::pdf_page_reading_position(page_index);
    conn.update_reading_position(&book_id.canonical(), &position)?;
    Ok(())
}

pub fn update_last_read(conn: &Connection, book_id: BookId) -> Result<(), rusqlite::Error> {
    let last_read_time = unix_micros()?;
    conn.update_last_read(&book_id.canonical(), last_read_time)?;
    Ok(())
}

#[derive(Debug)]
pub enum OutboxReadError {
    Database(rusqlite::Error),
    CorruptMetadata { key: &'static str },
    UnsupportedEventType { local_seq: i64, event_type: String },
    CorruptEvent { local_seq: i64, reason: &'static str },
}

impl std::fmt::Display for OutboxReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "failed to read local sync outbox: {error}"),
            Self::CorruptMetadata { key } => write!(formatter, "sync metadata '{key}' is not a valid non-negative integer"),
            Self::UnsupportedEventType { local_seq, event_type } => write!(formatter, "unsupported local event type '{event_type}' at local_seq {local_seq}"),
            Self::CorruptEvent { local_seq, reason } => write!(formatter, "corrupt local event at local_seq {local_seq}: {reason}"),
        }
    }
}

impl std::error::Error for OutboxReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptMetadata { .. } | Self::UnsupportedEventType { .. } | Self::CorruptEvent { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
struct OutboxRow {
    event_id: String,
    entity_id: String,
    event_type: String,
    payload: String,
    changed_at: i64,
    local_seq: i64,
    hlc_pt: i64,
    hlc_l: i64,
}

fn decode_outbox_row(row: OutboxRow) -> Result<LocalChange, OutboxReadError> {
    let raw_local_seq = row.local_seq;
    let corrupt = |reason| OutboxReadError::CorruptEvent { local_seq: raw_local_seq, reason };
    let event_id = EventId::parse(&row.event_id).map_err(|_| corrupt("invalid event id"))?;
    let payload = serde_json::from_str(&row.payload).map_err(|_| corrupt("payload is not valid JSON"))?;
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let local_seq = u64::try_from(row.local_seq).map_err(|_| corrupt("negative local sequence"))?;
    let hlc_pt = u64::try_from(row.hlc_pt).map_err(|_| corrupt("negative HLC physical component"))?;
    let hlc_l = u64::try_from(row.hlc_l).map_err(|_| corrupt("negative HLC logical component"))?;

    if !SyncOp::is_known_event_type(&row.event_type) {
        return Err(OutboxReadError::UnsupportedEventType { local_seq: raw_local_seq, event_type: row.event_type });
    }
    let op = SyncOp::from_event_parts(&row.event_type, row.entity_id, &payload, changed_at).ok_or_else(|| corrupt("known event has invalid identity or payload"))?;
    let local_seq = LocalSeq::new(local_seq).map_err(|_| corrupt("local sequence must be positive"))?;
    Ok(LocalChange { event_id, op, changed_at, hlc: Hlc { pt: hlc_pt, l: hlc_l }, local_seq })
}

pub fn get_unpushed_events(conn: &Connection) -> Result<Vec<LocalChange>, OutboxReadError> {
    let last_local_seq_sent = match get_sync_metadata(conn, "last_pushed_local_seq").map_err(OutboxReadError::Database)? {
        Some(value) => value.parse::<u64>().map_err(|_| OutboxReadError::CorruptMetadata { key: "last_pushed_local_seq" })?,
        None => 0,
    };
    let mut events = Vec::new();
    let mut decode_error = None;
    let query_result = conn.get_unpushed_events(last_local_seq_sent, |row| {
        let stored = OutboxRow { event_id: row.get(0)?, entity_id: row.get(1)?, event_type: row.get(2)?, payload: row.get(3)?, changed_at: row.get(4)?, local_seq: row.get(5)?, hlc_pt: row.get(6)?, hlc_l: row.get(7)? };
        match decode_outbox_row(stored) {
            Ok(change) => {
                events.push(change);
                Ok(())
            }
            Err(error) => {
                decode_error = Some(error);
                Err(rusqlite::Error::InvalidQuery)
            }
        }
    });
    if let Some(error) = decode_error {
        return Err(error);
    }
    query_result.map_err(OutboxReadError::Database)?;
    Ok(events)
}
pub fn mark_events_pushed(conn: &Connection, up_to_local_seq: u64) -> Result<(), rusqlite::Error> {
    set_sync_metadata(conn, "last_pushed_local_seq", &up_to_local_seq.to_string())
}

/// Bumps and returns the monotonic scan generation counter. Each full library
/// scan claims a fresh id, stamps every placement it sees with it, and then
/// reconciles unseen local placements via [`mark_vanished_book_dirs_deleted`].
pub fn next_scan_id(conn: &Connection) -> Result<i64, rusqlite::Error> {
    let current = match get_sync_metadata(conn, "scan_seq")? {
        Some(value) => value.parse::<i64>().map_err(|error| invalid_text_column(0, error.to_string()))?,
        None => 0,
    };
    let next = current.checked_add(1).ok_or_else(|| invalid_text_column(0, "scan sequence overflow"))?;
    set_sync_metadata(conn, "scan_seq", &next.to_string())?;
    Ok(next)
}

/// Consecutive scans a downloaded file must be absent before it's deleted. The
/// grace rides out transient misses; because deletions propagate to other devices
/// (destructive), the scanner pairs it with a scheduled re-scan so the count can
/// advance to this threshold.
pub const VANISH_GRACE_SCANS: i64 = 2;

/// Soft-deletes locally-present placements absent for at least [`VANISH_GRACE_SCANS`]
/// scans (files removed from disk). A move surfaces here as the old path being
/// deleted while the new path was added during the same scan. Emits
/// book_dir_removed events.
pub fn mark_vanished_book_dirs_deleted(conn: &Connection, scan_id: i64) -> Result<(), rusqlite::Error> {
    let now = unix_micros()?;
    conn.mark_vanished_book_dirs_deleted(scan_id, VANISH_GRACE_SCANS, now)?;
    Ok(())
}

/// Count of placements missing this scan but still within the grace window, i.e.
/// pending deletions a follow-up scan should revisit.
pub fn count_pending_vanish_candidates(conn: &Connection, scan_id: i64) -> Result<i64, rusqlite::Error> {
    let mut count = 0_i64;
    conn.count_pending_vanish_candidates(scan_id, VANISH_GRACE_SCANS, |row| {
        count = row.get(0)?;
        Ok(())
    })?;
    Ok(count)
}

pub fn get_sync_metadata(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    let mut value: Option<String> = None;
    conn.get_sync_metadata_value(key, |row| {
        value = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(value)
}

pub fn set_sync_metadata(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.set_sync_metadata(key, value)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DeletedDirCascade {
    file_paths: Vec<RelativeBookPath>,
    dir_paths: Vec<(DirId, RelativeDirPath)>,
}

pub fn delete_dir(conn: &mut Connection, dir_id: &DirId) -> Result<DeletedDirCascade, rusqlite::Error> {
    let now = unix_micros()?;
    let tx = conn.transaction()?;

    let mut file_paths = Vec::new();
    tx.get_dir_delete_file_paths(dir_id.as_str(), |row| {
        let raw_path: String = row.get(0)?;
        file_paths.push(RelativeBookPath::parse(&raw_path).map_err(|error| invalid_text_column(0, error.to_string()))?);
        Ok(())
    })?;

    let mut dir_paths = Vec::new();
    tx.get_dir_delete_dir_paths(dir_id.as_str(), |row| {
        let raw_path: String = row.get(0)?;
        let path = RelativeDirPath::parse(&raw_path).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_path_id: String = row.get(1)?;
        let path_id = DirId::parse(&raw_path_id).map_err(|error| invalid_text_column(1, error.to_string()))?;
        dir_paths.push((path_id, path));
        Ok(())
    })?;

    tx.tombstone_book_dirs_in_subtree(dir_id.as_str(), now)?;
    tx.tombstone_dirs_in_subtree(dir_id.as_str(), now)?;
    tx.commit()?;

    Ok(DeletedDirCascade { file_paths, dir_paths })
}

pub fn remove_deleted_dir_artifacts(library_path: &Path, deleted: &DeletedDirCascade) -> std::io::Result<()> {
    let library_root = LibraryRoot::open(library_path)?;
    for rel_path in &deleted.file_paths {
        let abs = library_root.resolve_book(rel_path);
        ensure_no_symlink_components(library_root.as_path(), &abs)?;
        match fs::remove_file(&abs) {
            Ok(()) => log::info!("Deleted {:?} (dir deleted locally)", abs),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }

    for (dir_id, rel_path) in &deleted.dir_paths {
        let abs = library_root.resolve_dir(rel_path);
        if !abs.exists() {
            continue;
        }
        ensure_no_symlink_components(library_root.as_path(), &abs)?;
        let marker_content = require_directory_marker(&abs, dir_id)?;
        let marker = abs.join(".biblos_uuid");
        match fs::remove_file(&marker) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        match fs::remove_dir(&abs) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                let _ = fs::write(marker, marker_content);
                return Err(err);
            }
        }
    }
    Ok(())
}

/// A placement a remote removal soft-deleted, so the caller can delete the
/// corresponding local file (propagating the deletion across devices).
#[derive(Debug, Clone)]
pub struct RemovedBookDir {
    pub dir_id: DirId,
    pub book_id: BookId,
    pub file_name: FileName,
}

fn invalid_pending_column(id: i64, column: usize, message: impl std::fmt::Display) -> rusqlite::Error {
    invalid_text_column(column, format!("pending filesystem operation {id}: {message}"))
}

#[derive(Debug, Clone)]
enum PendingFsOperation {
    RemoveBookFile { id: i64, dir_id: DirId, book_id: BookId, file_name: FileName, rel_path: Option<RelativeBookPath> },
    RemoveDirectory { id: i64, dir_id: DirId, rel_path: RelativeDirPath },
}

impl PendingFsOperation {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let id: i64 = row.get(0)?;
        if id <= 0 {
            return Err(rusqlite::Error::FromSqlConversionFailure(0, Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "pending filesystem operation id must be positive"))));
        }
        let kind: String = row.get(1)?;
        let raw_dir_id: String = row.get(2)?;
        let raw_book_id: String = row.get(3)?;
        let raw_file_name: String = row.get(4)?;
        let raw_rel_path: String = row.get(5)?;
        let dir_id = DirId::parse(&raw_dir_id).map_err(|error| invalid_pending_column(id, 2, error))?;
        match kind.as_str() {
            "book_file_remove" => {
                let book_id = BookId::parse(&raw_book_id).map_err(|error| invalid_pending_column(id, 3, error))?;
                if raw_book_id != book_id.canonical() {
                    return Err(invalid_pending_column(id, 3, "book id is not canonical"));
                }
                let file_name = FileName::parse(&raw_file_name).map_err(|error| invalid_pending_column(id, 4, error))?;
                let rel_path = if raw_rel_path.is_empty() { None } else { Some(RelativeBookPath::parse(&raw_rel_path).map_err(|error| invalid_pending_column(id, 5, error))?) };
                if rel_path.as_ref().is_some_and(|path| Path::new(path.as_str()).file_name().and_then(|name| name.to_str()) != Some(file_name.as_str())) {
                    return Err(invalid_pending_column(id, 5, "book removal path does not end in its file name"));
                }
                Ok(Self::RemoveBookFile { id, dir_id, book_id, file_name, rel_path })
            }
            "dir_remove" if raw_book_id.is_empty() && raw_file_name.is_empty() => {
                if dir_id.as_str() == "__root__" {
                    return Err(invalid_pending_column(id, 2, "the library root cannot be removed"));
                }
                let rel_path = RelativeDirPath::parse(&raw_rel_path).map_err(|error| invalid_pending_column(id, 5, error))?;
                Ok(Self::RemoveDirectory { id, dir_id, rel_path })
            }
            "dir_remove" => Err(invalid_pending_column(id, 1, "directory removal contains book-only fields")),
            _ => Err(invalid_pending_column(id, 1, format!("unknown operation kind '{kind}'"))),
        }
    }

    fn id(&self) -> i64 {
        match self {
            Self::RemoveBookFile { id, .. } | Self::RemoveDirectory { id, .. } => *id,
        }
    }
}

#[derive(Debug, Default)]
pub struct PendingRemovalPaths {
    pub files: HashSet<PathBuf>,
    pub dirs: HashSet<PathBuf>,
}

fn pending_fs_operations(conn: &Connection) -> Result<Vec<PendingFsOperation>, rusqlite::Error> {
    let mut operations = Vec::new();
    conn.get_pending_fs_operations(|row| {
        operations.push(PendingFsOperation::decode(row)?);
        Ok(())
    })?;
    Ok(operations)
}

pub fn pending_removal_paths(conn: &Connection) -> Result<PendingRemovalPaths, rusqlite::Error> {
    let mut paths = PendingRemovalPaths::default();
    for operation in pending_fs_operations(conn)? {
        match operation {
            PendingFsOperation::RemoveBookFile { rel_path: Some(rel_path), .. } => {
                paths.files.insert(PathBuf::from(rel_path.as_str().trim_start_matches('/')));
            }
            PendingFsOperation::RemoveBookFile { rel_path: None, .. } => {}
            PendingFsOperation::RemoveDirectory { rel_path, .. } => {
                paths.dirs.insert(rel_path.as_path().to_path_buf());
            }
        }
    }
    Ok(paths)
}

pub fn is_book_file_pending_removal_tx(tx: &Transaction, dir_id: &str, file_name: &str) -> Result<bool, rusqlite::Error> {
    let mut pending = false;
    tx.is_book_file_pending_removal(dir_id, file_name, |row| {
        pending = row.get(0)?;
        Ok(())
    })?;
    Ok(pending)
}

#[cfg(test)]
pub fn pending_fs_operation_count(conn: &Connection) -> Result<usize, rusqlite::Error> {
    Ok(pending_fs_operations(conn)?.len())
}

fn pending_book_removal_is_still_current(conn: &Connection, dir_id: &DirId, book_id: BookId, file_name: &FileName) -> Result<bool, rusqlite::Error> {
    let mut deleted = false;
    conn.get_book_dir_removal_state(dir_id.as_str(), &book_id.canonical(), file_name.as_str(), |row| {
        deleted = row.get(0)?;
        Ok(())
    })?;
    Ok(deleted)
}

fn pending_dir_removal_is_superseded(conn: &Connection, dir_id: &DirId) -> Result<bool, rusqlite::Error> {
    let mut live = false;
    conn.is_dir_live(dir_id.as_str(), |row| {
        live = row.get(0)?;
        Ok(())
    })?;
    Ok(live)
}

fn ensure_no_symlink_components(library_root: &Path, path: &Path) -> std::io::Result<()> {
    let relative = path.strip_prefix(library_root).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "pending filesystem path escaped the library root"))?;
    let mut current = library_root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("pending filesystem path contains a symlink: {}", current.display()))),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn require_directory_marker(directory: &Path, dir_id: &DirId) -> std::io::Result<Vec<u8>> {
    let marker = directory.join(".biblos_uuid");
    let content = fs::read(&marker)?;
    let marker_id = std::str::from_utf8(&content).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?.trim();
    if marker_id != dir_id.as_str() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("directory marker at {} identifies '{}', expected '{}'", marker.display(), marker_id, dir_id)));
    }
    Ok(content)
}

fn require_book_parent_identity(library_root: &Path, path: &Path, dir_id: &DirId) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "pending book path has no parent directory"))?;
    if dir_id.as_str() == "__root__" {
        if parent != library_root {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "root placement path is not directly below the library root"));
        }
        return Ok(());
    }
    require_directory_marker(parent, dir_id).map(|_| ())
}

fn execute_pending_fs_operation(conn: &Connection, library_root: &Path, operation: &PendingFsOperation) -> Result<(), Box<dyn std::error::Error>> {
    match operation {
        PendingFsOperation::RemoveBookFile { dir_id, book_id, file_name, rel_path, .. } => {
            if !pending_book_removal_is_still_current(conn, dir_id, *book_id, file_name)? {
                return Ok(());
            }
            let rel_path = if let Some(rel_path) = rel_path {
                rel_path.clone()
            } else {
                let components = get_dir_path_components(conn, dir_id.as_str())?;
                let dir_path = dir_path_from_components(&components)?.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "pending book removal has no resolvable directory"))?;
                dir_path.join_file(file_name)?
            };
            let path = rel_path.resolve_under(library_root);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
            ensure_no_symlink_components(library_root, &path)?;
            require_book_parent_identity(library_root, &path, dir_id)?;
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }
        PendingFsOperation::RemoveDirectory { dir_id, rel_path, .. } => {
            if pending_dir_removal_is_superseded(conn, dir_id)? {
                return Ok(());
            }
            let path = rel_path.resolve_under(library_root);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
            ensure_no_symlink_components(library_root, &path)?;
            let marker = path.join(".biblos_uuid");
            let marker_content = require_directory_marker(&path, dir_id)?;
            match fs::remove_file(&marker) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            match fs::remove_dir(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => {
                    let _ = fs::write(marker, marker_content);
                    Err(error.into())
                }
            }
        }
    }
}

/// Drains one durable outbox pass. Failed operations remain queued for the next
/// sync/startup, while completed or superseded operations are removed.
pub fn drain_pending_fs_operations(conn: &Connection, library_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let library_root = library_path.canonicalize()?;
    if !library_root.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("library root is not a directory: {}", library_path.display())).into());
    }
    let mut first_filesystem_error = None;
    for operation in pending_fs_operations(conn)? {
        match execute_pending_fs_operation(conn, &library_root, &operation) {
            Ok(()) => {
                conn.delete_pending_fs_operation(operation.id())?;
            }
            Err(error) => {
                log::warn!("Pending filesystem operation {} failed: {}", operation.id(), error);
                conn.fail_pending_fs_operation(operation.id(), &error.to_string())?;
                if first_filesystem_error.is_none() {
                    first_filesystem_error = Some(error);
                }
            }
        }
    }
    match first_filesystem_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub fn set_book_downloaded(conn: &Connection, rel_path: &RelativeBookPath, book_id: BookId, downloaded: bool) -> Result<(), rusqlite::Error> {
    let Some((found_book_id, dir_id, file_name)) = get_book_dir_entry_by_path(conn, rel_path)? else {
        return Ok(());
    };
    if found_book_id != book_id {
        return Err(invalid_text_column(0, "book placement identity mismatch"));
    }
    let value = if downloaded { 1 } else { 0 };
    conn.set_book_downloaded(dir_id.as_str(), &book_id.canonical(), file_name.as_str(), value)?;
    Ok(())
}

pub fn upsert_dir(transaction: &Transaction, dir_uuid: &DirId, parent_uuid: &DirId, name: &FileName) -> Result<(), rusqlite::Error> {
    if dir_uuid.as_str() == "__root__" {
        return Err(invalid_text_column(0, "the root directory is reserved"));
    }
    if dir_uuid == parent_uuid {
        return Err(invalid_text_column(0, "a directory cannot be its own parent"));
    }
    let closes_cycle: bool = transaction.query_row(
        "WITH RECURSIVE ancestors(id, depth) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT d.parent_id, ancestors.depth + 1
             FROM dir d JOIN ancestors ON d.id=ancestors.id
             WHERE d.deleted_at IS NULL AND d.id!='__root__' AND ancestors.depth < 10000
         )
         SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=?2)",
        rusqlite::params![parent_uuid.as_str(), dir_uuid.as_str()],
        |row| row.get(0),
    )?;
    if closes_cycle {
        return Err(invalid_text_column(0, "directory move would create a cycle"));
    }
    transaction.upsert_dir(dir_uuid.as_str(), parent_uuid.as_str(), name.as_str())?;
    Ok(())
}

pub(crate) struct CreatedDirectory {
    pub(crate) directory: Dir,
    pub(crate) relative_path: RelativeDirPath,
}

fn rollback_created_directory(path: &Path, dir_id: &DirId) {
    let marker = path.join(".biblos_uuid");
    if fs::read_to_string(&marker).ok().as_deref() != Some(dir_id.as_str()) {
        return;
    }
    if fs::remove_file(&marker).is_ok() && fs::remove_dir(path).is_err() {
        let _ = fs::write(marker, dir_id.as_str());
    }
}

pub(crate) fn create_local_directory(conn: &mut Connection, library_root: &LibraryRoot, parent_id: &DirId, name: &FileName) -> Result<CreatedDirectory, Box<dyn std::error::Error>> {
    let tx = conn.transaction()?;
    let parent_is_live: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NULL)", [parent_id.as_str()], |row| row.get(0))?;
    if !parent_is_live {
        return Err(format!("parent directory does not exist: {parent_id}").into());
    }

    let dir_id = DirId::parse(&Uuid::new_v4().to_string())?;
    upsert_dir(&tx, &dir_id, parent_id, name)?;
    let components = get_dir_path_components_tx(&tx, dir_id.as_str())?;
    let relative_path = dir_path_from_components(&components)?.ok_or("created directory has no projected path")?;
    materialize_dir_components_with_markers(library_root, &components, false)?;
    let absolute_path = library_root.resolve_dir(&relative_path);
    match fs::symlink_metadata(&absolute_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err(format!("a filesystem entry already exists at {}", absolute_path.display()).into()),
        Err(error) => return Err(error.into()),
    }

    fs::create_dir(&absolute_path)?;
    let marker_path = absolute_path.join(".biblos_uuid");
    let marker_result = (|| {
        let mut marker = fs::OpenOptions::new().write(true).create_new(true).open(marker_path)?;
        marker.write_all(dir_id.as_str().as_bytes())?;
        marker.sync_all()
    })();
    if let Err(error) = marker_result {
        let _ = fs::remove_file(absolute_path.join(".biblos_uuid"));
        let _ = fs::remove_dir(&absolute_path);
        return Err(error.into());
    }

    if let Err(error) = tx.commit() {
        rollback_created_directory(&absolute_path, &dir_id);
        return Err(error.into());
    }
    Ok(CreatedDirectory { directory: Dir::new(dir_id, name.clone(), 0, 0), relative_path })
}

pub struct BookDir {
    pub book_id: BookId,
    /// Content-addressed blob key carried by the existing row, so the scan-skip
    /// path can re-insert without recomputing it. Empty for rows predating the
    /// content-hash column (those are re-hashed to backfill).
    pub content_hash: ContentHash,
}

pub fn insert_book(transaction: &Transaction, book_id: BookId) -> Result<(), rusqlite::Error> {
    let added_at = unix_micros()?;

    transaction.first_book_insert(&book_id.canonical(), added_at)?;
    Ok(())
}

pub fn insert_local_book_dir(transaction: &Transaction, dir_id: &DirId, book_id: BookId, file_name: &FileName, local_hash: u64, content_hash: &ContentHash, last_scan: i64) -> Result<(), rusqlite::Error> {
    // A scan observes one final occupant for a physical path. Tombstone any
    // displaced identity first, in this same transaction, so resurrection also
    // works when the returning occupant already has a historical tombstone row.
    // The removal is identity-specific and cannot later erase the replacement.
    transaction.execute(
        "UPDATE book_dir SET deleted_at=?4, intent_deleted=1
         WHERE dir_id=?1 AND file_name=?2 AND book_id<>?3 AND deleted_at IS NULL",
        rusqlite::params![dir_id.as_str(), file_name.as_str(), book_id.canonical(), unix_micros()?],
    )?;
    transaction.insert_book_dir(dir_id.as_str(), &book_id.canonical(), file_name.as_str(), &local_hash.to_string(), content_hash.as_str(), last_scan, 1)?;
    Ok(())
}

pub fn get_book_dir_by_hash(transaction: &Transaction, hash: u64) -> Result<Option<BookDir>, rusqlite::Error> {
    let mut result: Option<BookDir> = None;
    let hash = hash.to_string();
    transaction.get_book_dir_by_hash(&hash, |row| {
        let raw_book_id: String = row.get(0)?;
        let book_id = BookId::parse(&raw_book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_parent_id: String = row.get(1)?;
        let _parent_id = DirId::parse(&raw_parent_id).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let raw_file_name: String = row.get(2)?;
        let _file_name = FileName::parse(&raw_file_name).map_err(|error| invalid_text_column(2, error.to_string()))?;
        let raw_content_hash: String = row.get(3)?;
        let Ok(content_hash) = ContentHash::parse(&raw_content_hash) else {
            log::info!("Rehashing placement with legacy content key");
            return Ok(());
        };
        result = Some(BookDir { book_id, content_hash });
        Ok(())
    })?;
    Ok(result)
}

fn get_or_create_author_id(transaction: &Transaction, author_name: &str) -> Result<Option<i64>, rusqlite::Error> {
    let normalized = author_name.trim();
    if normalized.is_empty() {
        return Ok(None);
    }
    transaction.execute("INSERT OR IGNORE INTO author(name) VALUES (?1)", [normalized])?;
    transaction.query_row("SELECT id FROM author WHERE name = ?1 LIMIT 1", [normalized], |row| row.get(0)).map(Some)
}

pub fn update_book_metadata(transaction: &Transaction, book_id: BookId, title: &str, author: &str) -> Result<(), rusqlite::Error> {
    let author_id = get_or_create_author_id(transaction, author)?;
    transaction.execute("UPDATE book SET title = ?1, author_id = ?2 WHERE id = ?3", rusqlite::params![title, author_id, book_id.canonical()])?;
    write_book_search_index(transaction, &book_id.canonical(), title, author)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CONTENT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_content_hash() -> ContentHash {
        ContentHash::parse(TEST_CONTENT_HASH).unwrap()
    }

    fn test_dir_id(value: &str) -> DirId {
        DirId::parse(value).unwrap()
    }

    fn test_file_name(value: &str) -> FileName {
        FileName::parse(value).unwrap()
    }

    #[test]
    fn client_identity_is_validated_at_the_database_boundary() {
        let conn = Connection::open_in_memory().unwrap();
        let generated = init_db(&conn).unwrap();
        assert_eq!(get_or_create_client_id(&conn).unwrap(), generated);

        set_sync_metadata(&conn, "client_id", "invalid/client").unwrap();
        assert!(get_or_create_client_id(&conn).is_err());
    }

    #[test]
    fn subdirectory_book_counts_include_descendants_without_double_counting_books() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO dir(id, parent_id, name) VALUES
                 ('a', '__root__', 'A'),
                 ('a-child', 'a', 'Child'),
                 ('b', '__root__', 'B');
             INSERT INTO book(id, title) VALUES ('1', 'One'), ('2', 'Two'), ('3', 'Three'), ('4', 'Deleted');
             UPDATE book SET deleted_at=1 WHERE id='4';
             INSERT INTO book_dir(dir_id, book_id, file_name, local_hash) VALUES
                 ('a', '1', 'one.epub', 'a-one'),
                 ('a-child', '1', 'one-again.epub', 'a-one-again'),
                 ('a-child', '2', 'two.epub', 'a-two'),
                 ('a', '4', 'deleted.epub', 'a-deleted'),
                 ('b', '3', 'three.epub', 'b-three');
             UPDATE book_dir SET is_downloaded=1 WHERE book_id IN ('1', '3');",
        )
        .unwrap();

        let (directories, _) = get_dir_contents(&conn, "__root__").unwrap();
        let counts = directories.into_iter().map(|directory| (directory.name().as_str().to_owned(), (directory.book_count(), directory.downloaded_book_count()))).collect::<HashMap<_, _>>();

        assert_eq!(counts.get("A"), Some(&(2, 1)));
        assert_eq!(counts.get("B"), Some(&(1, 1)));
    }

    #[test]
    fn folder_download_queries_are_recursive_and_preserve_durable_requests() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let second_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let third_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        conn.execute_batch(
            "INSERT INTO dir(id, parent_id, name) VALUES
                 ('shelf', '__root__', 'Shelf'),
                 ('child', 'shelf', 'Child'),
                 ('other', '__root__', 'Other');
             INSERT INTO book(id, added_at) VALUES ('1', 1), ('2', 1), ('3', 1);",
        )
        .unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('shelf', '1', 'one.epub', ?1, ?1, 1)", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('child', '2', 'two.epub', ?1, ?1, 0)", [second_hash]).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('other', '3', 'three.epub', ?1, ?1, 1)", [third_hash]).unwrap();

        assert_eq!(get_descendant_book_ids(&conn, "shelf").unwrap(), vec![BookId::new(1), BookId::new(2)]);
        assert_eq!(get_book_ancestor_dir_ids(&conn, BookId::new(2)).unwrap(), vec![test_dir_id("__root__"), test_dir_id("child"), test_dir_id("shelf")]);
        assert_eq!(get_folder_download_counts(&conn, "shelf").unwrap(), (1, 2));
        assert_eq!(get_folder_download_counts(&conn, "other").unwrap(), (1, 1));

        add_book_download_request(&conn, BookId::new(2), TransferOrigin::UserInitiated).unwrap();
        assert!(is_book_download_requested(&conn, BookId::new(2)).unwrap());
        let states = get_book_download_states(&conn, &[BookId::new(1), BookId::new(2), BookId::new(3)]).unwrap();
        assert_eq!(states.get(&BookId::new(1)), Some(&(true, false)));
        assert_eq!(states.get(&BookId::new(2)), Some(&(false, true)));
        assert_eq!(states.get(&BookId::new(3)), Some(&(true, false)));
        complete_book_download_request(&conn, BookId::new(2)).unwrap();
        add_asset_request(&conn, BlobKind::Book, &ContentHash::parse(second_hash).unwrap(), TransferOrigin::UserInitiated).unwrap();
        assert!(is_book_download_requested(&conn, BookId::new(2)).unwrap());
        assert!(!is_book_download_requested(&conn, BookId::new(1)).unwrap());
    }

    #[test]
    fn bigram_search_matches_two_character_title_and_unicode_author_substrings() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO book(id, title, added_at) VALUES ('1', '', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash) VALUES ('__root__', '1', 'power.epub', 'power')", []).unwrap();

        let transaction = conn.transaction().unwrap();
        update_book_metadata(&transaction, BookId::new(1), "Power and Progress", "GÖRAN Smith").unwrap();
        transaction.commit().unwrap();

        assert_eq!(search_books(&conn, "ow", 10).unwrap().len(), 1);
        assert_eq!(search_books(&conn, "ör", 10).unwrap().len(), 1);
        assert_eq!(search_books(&conn, "progress", 10).unwrap().len(), 1);
        assert!(search_books(&conn, "missing", 10).unwrap().is_empty());
        let indexed: i64 = conn.query_row("SELECT COUNT(*) FROM book_search_bigram WHERE book_id='1' AND gram='ow'", [], |row| row.get(0)).unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn database_initialization_backfills_missing_book_search_documents() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO author(id, name) VALUES (1, 'Octavia Butler');
             INSERT INTO book(id, title, author_id, added_at) VALUES ('2', 'Parable of the Sower', 1, 1);
             INSERT INTO book_dir(dir_id, book_id, file_name, local_hash) VALUES ('__root__', '2', 'parable.epub', 'parable');",
        )
        .unwrap();

        init_db(&conn).unwrap();

        assert_eq!(search_books(&conn, "ra", 10).unwrap().len(), 1);
        assert_eq!(search_books(&conn, "butler", 10).unwrap().len(), 1);
    }

    fn server_change(op: SyncOp, pt: u64, l: u64) -> ServerChange {
        server_change_from(op, pt, l, "remote", 1, 1)
    }

    fn server_change_from(op: SyncOp, pt: u64, l: u64, actor: &str, local_seq: u64, server_seq: u64) -> ServerChange {
        let change = LocalChange { event_id: EventId::new(), op, changed_at: pt, hlc: Hlc { pt, l }, local_seq: LocalSeq::new(local_seq).unwrap() };
        ServerChange { change, actor_id: ActorId::parse(actor).unwrap(), server_seq: sync_common::ServerSeq::new(server_seq).unwrap() }
    }

    fn valid_outbox_row() -> OutboxRow {
        OutboxRow { event_id: EventId::new().to_string(), entity_id: "1".to_string(), event_type: "book_deleted".to_string(), payload: "{}".to_string(), changed_at: 10, local_seq: 1, hlc_pt: 10, hlc_l: 0 }
    }

    #[test]
    fn outbox_decoder_accepts_valid_rows() {
        let change = decode_outbox_row(valid_outbox_row()).expect("valid outbox row");
        assert_eq!(change.local_seq.get(), 1);
        assert!(matches!(change.op, SyncOp::BookDeleted { book_id } if book_id == BookId::new(1)));
    }

    #[test]
    fn outbox_decoder_rejects_invalid_json_and_known_payloads() {
        let mut invalid_json = valid_outbox_row();
        invalid_json.payload = "{".to_string();
        assert!(matches!(decode_outbox_row(invalid_json), Err(OutboxReadError::CorruptEvent { local_seq: 1, .. })));

        let mut malformed_known = valid_outbox_row();
        malformed_known.event_type = "book_dir_added".to_string();
        assert!(matches!(decode_outbox_row(malformed_known), Err(OutboxReadError::CorruptEvent { local_seq: 1, .. })));
    }

    #[test]
    fn outbox_read_never_returns_events_after_an_undecodable_row() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE event_log(
                 local_seq INTEGER PRIMARY KEY,
                 event_id TEXT NOT NULL,
                 entity_id TEXT NOT NULL,
                 event_type TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 timestamp INTEGER NOT NULL,
                 hlc_pt INTEGER NOT NULL,
                 hlc_l INTEGER NOT NULL
             );
             CREATE TABLE sync_metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO event_log VALUES
                 (1, '00000000-0000-4000-8000-000000000001', '1', 'future_event', '{}', 10, 10, 0),
                 (2, '00000000-0000-4000-8000-000000000002', '1', 'book_deleted', '{}', 11, 11, 0);",
        )
        .unwrap();

        assert!(matches!(get_unpushed_events(&conn), Err(OutboxReadError::UnsupportedEventType { local_seq: 1, .. })));
        assert_eq!(get_sync_metadata(&conn, "last_pushed_local_seq").unwrap(), None);
    }

    /// Builds an in-memory DB carrying the pre-migration `book_dir` schema, where
    /// `local_hash` was the primary key.
    fn old_schema_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // FK targets the rebuilt book_dir references; enforced after migration
        // turns foreign_keys back on.
        conn.execute_batch(
            "CREATE TABLE dir(id TEXT PRIMARY KEY);
             CREATE TABLE book(id TEXT PRIMARY KEY);
             INSERT INTO dir(id) VALUES ('d1'), ('d2');
             INSERT INTO book(id) VALUES ('b1');",
        )
        .unwrap();
        conn.execute_batch(
            // Columns the standalone ALTERs add before the rebuild runs in init_db.
            "CREATE TABLE book_dir(
                 dir_id          TEXT not null,
                 book_id         TEXT not null,
                 file_name       TEXT not null,
                 local_hash      TEXT not null,
                 content_hash    TEXT not null default '',
                 last_scan       INTEGER not null default 0,
                 hlc_pt          INTEGER not null default 0,
                 hlc_l           INTEGER not null default 0,
                 hlc_actor       TEXT not null default '',
                 hlc_seq         INTEGER not null default 0,
                 hlc_event       TEXT not null default '',
                 is_downloaded   INTEGER not null default 0,
                 deleted_at      INTEGER,
                 PRIMARY KEY     (local_hash)
             );",
        )
        .unwrap();
        conn
    }

    fn count_rows(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM book_dir", [], |r| r.get(0)).unwrap()
    }

    /// Fresh table on the current (path-keyed) schema, for sweep tests.
    fn new_schema_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE book_dir(
                 dir_id          TEXT not null,
                 book_id         TEXT not null,
                 file_name       TEXT not null,
                 local_hash      TEXT not null,
                 content_hash    TEXT not null default '',
                 is_downloaded   INTEGER not null default 0,
                 last_scan       INTEGER not null default 0,
                 hlc_pt          INTEGER not null default 0,
                 hlc_l           INTEGER not null default 0,
                 hlc_actor       TEXT not null default '',
                 hlc_seq         INTEGER not null default 0,
                 hlc_event       TEXT not null default '',
                 intent_deleted  INTEGER,
                 deleted_at      INTEGER,
                 PRIMARY KEY     (dir_id, book_id, file_name)
             );",
        )
        .unwrap();
        conn
    }

    fn is_alive(conn: &Connection, dir: &str, book: &str, file: &str) -> bool {
        conn.query_row("SELECT deleted_at IS NULL FROM book_dir WHERE dir_id=?1 AND book_id=?2 AND file_name=?3", rusqlite::params![dir, book, file], |r| r.get::<_, bool>(0)).unwrap()
    }

    /// Schema needed to drive apply_remote_book_dir: dir, book (for
    /// first_book_insert), and the path-keyed book_dir with HLC columns.
    fn lww_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE dir(id TEXT PRIMARY KEY, parent_id TEXT NOT NULL, name TEXT NOT NULL, deleted_at INTEGER);
             INSERT INTO dir(id, parent_id, name) VALUES ('__root__', '__root__', ''), ('d1', '__root__', 'D1');
             CREATE TABLE book(id TEXT PRIMARY KEY, added_at INTEGER, deleted_at INTEGER);
             CREATE TABLE book_dir(
                 dir_id TEXT not null, book_id TEXT not null, file_name TEXT not null,
                 local_hash TEXT not null, content_hash TEXT not null default '',
                 is_downloaded INTEGER not null default 0, last_scan INTEGER not null default 0,
                 hlc_pt INTEGER not null default 0, hlc_l INTEGER not null default 0,
                 hlc_actor TEXT not null default '', hlc_seq INTEGER not null default 0,
                 hlc_event TEXT not null default '',
                 intent_deleted INTEGER,
                 deleted_at INTEGER,
                 PRIMARY KEY (dir_id, book_id, file_name)
             );
             CREATE TABLE pending_fs_operations(
                 id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL,
                 dir_id TEXT NOT NULL DEFAULT '', book_id TEXT NOT NULL DEFAULT '',
                 file_name TEXT NOT NULL DEFAULT '', rel_path TEXT NOT NULL DEFAULT '',
                 attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_at INTEGER NOT NULL,
                 UNIQUE(kind, dir_id, book_id, file_name, rel_path)
             );",
        )
        .unwrap();
        conn
    }

    fn book_dir_change(add: bool, pt: u64, l: u64) -> ServerChange {
        let op = if add {
            SyncOp::BookDirAdded { book_id: BookId::new(1), dir_id: test_dir_id("d1"), file_name: test_file_name("f.epub"), content_hash: test_content_hash() }
        } else {
            SyncOp::BookDirRemoved { book_id: BookId::new(1), dir_id: test_dir_id("d1"), file_name: test_file_name("f.epub") }
        };
        server_change(op, pt, l)
    }

    fn book_dir_add_change(book_id: BookId, dir_id: &str, file_name: &str, pt: u64) -> ServerChange {
        server_change(SyncOp::BookDirAdded { book_id, dir_id: test_dir_id(dir_id), file_name: test_file_name(file_name), content_hash: test_content_hash() }, pt, 0)
    }

    fn dir_added_change(dir_id: &str, parent_id: &str, name: &str, pt: u64) -> ServerChange {
        dir_state_change(dir_id, parent_id, name, false, pt)
    }

    fn dir_state_change(dir_id: &str, parent_id: &str, name: &str, deleted: bool, pt: u64) -> ServerChange {
        let parent_id = test_dir_id(parent_id);
        let name = test_file_name(name);
        let directory = if deleted { DirectoryState::Deleted { parent_id, name } } else { DirectoryState::Present { parent_id, name } };
        server_change(SyncOp::DirState { dir_id: test_dir_id(dir_id), directory }, pt, 0)
    }

    fn fk_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn
    }

    fn reading_position_change(cfi: &str, pt: u64, l: u64) -> ServerChange {
        server_change(SyncOp::ReadingPositionUpdated { book_id: BookId::new(1), cfi: cfi.into() }, pt, l)
    }

    fn singleton_guard_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.insert_book("1", "T", 1).unwrap();
        update_reading_position(&conn, BookId::new(1), &EpubCfi::parse("epubcfi(/6/2)").unwrap()).unwrap();
        conn
    }

    fn read_pos_raw(conn: &Connection) -> String {
        conn.query_row("SELECT read_pos FROM book WHERE id='1'", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn pdf_page_position_uses_the_synchronized_reading_register() {
        let conn = singleton_guard_conn();
        update_pdf_reading_page(&conn, BookId::new(1), 17).unwrap();

        assert_eq!(get_pdf_reading_page(&conn, BookId::new(1)).unwrap(), Some(17));
        assert_eq!(get_book_reading_position(&conn, BookId::new(1)).unwrap(), None);
        assert_eq!(read_pos_raw(&conn), "pdfpage(17)");
        let payload: String = conn.query_row("SELECT payload FROM event_log WHERE event_type='reading_position_updated' ORDER BY local_seq DESC LIMIT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&payload).unwrap()["cfi"], "pdfpage(17)");
    }

    fn latest_read_pos_event(conn: &Connection) -> (u64, u64, u64) {
        conn.query_row("SELECT local_seq, hlc_pt, hlc_l FROM event_log WHERE entity_id='1' AND event_type='reading_position_updated' ORDER BY local_seq DESC LIMIT 1", [], |r| {
            let seq: i64 = r.get(0)?;
            let pt: i64 = r.get(1)?;
            let l: i64 = r.get(2)?;
            Ok((seq.max(0) as u64, pt.max(0) as u64, l.max(0) as u64))
        })
        .unwrap()
    }

    /// Applies a sequence of (add?, hlc_pt, hlc_l) remote ops and reports whether
    /// the placement ends up live (present) or tombstoned (removed).
    fn final_present(ops: &[(bool, u64, u64)]) -> bool {
        let mut conn = lww_conn();
        let tx = conn.transaction().unwrap();
        let dir_id = test_dir_id("d1");
        let file_name = test_file_name("f.epub");
        for &(add, pt, l) in ops {
            let change = book_dir_change(add, pt, l);
            let content_hash = test_content_hash();
            let state = if add { RemotePlacementState::Present(&content_hash) } else { RemotePlacementState::Removed };
            apply_remote_book_dir(&tx, &change, BookId::new(1), &dir_id, &file_name, state).unwrap();
        }
        let present = tx.query_row("SELECT COUNT(*) FROM book_dir WHERE dir_id='d1' AND book_id='1' AND file_name='f.epub' AND deleted_at IS NULL", [], |r| r.get::<_, i64>(0)).unwrap() > 0;
        tx.commit().unwrap();
        present
    }

    #[test]
    fn local_book_dir_ops_stamp_hlc_via_triggers() {
        // Drives the real schema + triggers to confirm a local add/remove stamps
        // book_dir.hlc in-transaction (the BEFORE->AFTER flip and nested stamp).
        let conn = Connection::open_in_memory().unwrap();
        let _client = init_db(&conn).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1','__root__','Folder')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at) VALUES ('b1', 1)", []).unwrap();

        // Local add (change_origin defaults to 'local'): trig_book_dir_added_insert
        // -> event_log -> trig_event_log_hlc -> stamps the placement's HLC.
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded, last_scan) VALUES ('d1','b1','f.epub','fp','HASH',1,1)", []).unwrap();
        let add_pt: i64 = conn.query_row("SELECT hlc_pt FROM book_dir WHERE file_name='f.epub'", [], |r| r.get(0)).unwrap();
        assert!(add_pt > 0, "local add must stamp an HLC, got {add_pt}");

        // Local remove: trig_book_dir_deleted (now AFTER) emits book_dir_removed and
        // the stamp advances the HLC.
        conn.execute("UPDATE book_dir SET deleted_at=1 WHERE file_name='f.epub'", []).unwrap();
        let removed: i64 = conn.query_row("SELECT COUNT(*) FROM event_log WHERE event_type='book_dir_removed'", [], |r| r.get(0)).unwrap();
        assert_eq!(removed, 1, "local remove must emit a book_dir_removed event");
        let remove_pt: i64 = conn.query_row("SELECT hlc_pt FROM book_dir WHERE file_name='f.epub'", [], |r| r.get(0)).unwrap();
        assert!(remove_pt >= add_pt, "remove must advance the HLC stamp: {remove_pt} >= {add_pt}");

        // Reactivation runs under an outer UPSERT. Metadata conflict handling in
        // its nested HLC trigger must remain an UPSERT as well; `OR REPLACE` is
        // overridden by the outer statement and used to fail on the metadata PK.
        let remove_version: (i64, i64) = conn.query_row("SELECT hlc_pt, hlc_l FROM book_dir WHERE file_name='f.epub'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        conn.insert_book_dir("d1", "b1", "f.epub", "fp2", "HASH", 2, 1).unwrap();
        let readd_version: (i64, i64) = conn.query_row("SELECT hlc_pt, hlc_l FROM book_dir WHERE file_name='f.epub'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let added: i64 = conn.query_row("SELECT COUNT(*) FROM event_log WHERE event_type='book_dir_added'", [], |r| r.get(0)).unwrap();
        assert_eq!(added, 2, "local re-add must emit a second book_dir_added event");
        assert!(readd_version > remove_version, "re-add must advance the HLC stamp: {readd_version:?} > {remove_version:?}");
    }

    #[test]
    fn local_book_lifecycle_ops_stamp_complete_versions() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        set_sync_metadata(&conn, "client_id", "local-device").unwrap();

        conn.insert_book("1", "Book", 1).unwrap();
        let added: (i64, i64, String, i64, bool) =
            conn.query_row("SELECT hlc_pt, hlc_l, hlc_actor, hlc_seq, deleted_at IS NOT NULL FROM book WHERE id='1'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
        assert!(added.0 > 0);
        assert_eq!(added.2, "local-device");
        assert!(added.3 > 0);
        assert!(!added.4);

        conn.execute("UPDATE book SET deleted_at=1 WHERE id='1'", []).unwrap();
        let deleted: (i64, i64, String, i64, bool) =
            conn.query_row("SELECT hlc_pt, hlc_l, hlc_actor, hlc_seq, deleted_at IS NOT NULL FROM book WHERE id='1'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
        assert!((deleted.0, deleted.1) > (added.0, added.1));
        assert_eq!(deleted.2, "local-device");
        assert!(deleted.3 > added.3);
        assert!(deleted.4);
    }

    #[test]
    fn dir_state_triggers_emit_full_state_for_create_move_and_delete() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
        conn.execute("UPDATE dir SET name='B' WHERE id='d1'", []).unwrap();
        conn.execute("UPDATE dir SET deleted_at=1 WHERE id='d1'", []).unwrap();

        let events: Vec<(String, String)> = {
            let mut stmt = conn.prepare("SELECT event_type, payload FROM event_log WHERE entity_id='d1' ORDER BY local_seq").unwrap();
            stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|(event_type, _)| event_type == "dir_state"));

        let first: serde_json::Value = serde_json::from_str(&events[0].1).unwrap();
        assert_eq!(first["parent_id"], "__root__");
        assert_eq!(first["name"], "A");
        assert_eq!(first["deleted"].as_i64(), Some(0));

        let renamed: serde_json::Value = serde_json::from_str(&events[1].1).unwrap();
        assert_eq!(renamed["name"], "B");
        assert_eq!(renamed["deleted"].as_i64(), Some(0));

        let deleted: serde_json::Value = serde_json::from_str(&events[2].1).unwrap();
        assert_eq!(deleted["name"], "B");
        assert_eq!(deleted["deleted"].as_i64(), Some(1));
    }

    #[test]
    fn stale_remote_dir_state_does_not_overwrite_unpushed_local_move() {
        let mut conn = fk_conn();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Local')", []).unwrap();
        conn.execute("UPDATE dir SET name='LocalNew' WHERE id='d1'", []).unwrap();
        let local_pt: u64 = conn
            .query_row("SELECT hlc_pt FROM event_log WHERE entity_id='d1' AND event_type='dir_state' ORDER BY local_seq DESC LIMIT 1", [], |r| {
                let pt: i64 = r.get(0)?;
                Ok(pt.max(0) as u64)
            })
            .unwrap();

        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &dir_state_change("d1", "__root__", "RemoteOld", false, local_pt.saturating_sub(1))).unwrap();
        tx.commit().unwrap();

        let name: String = conn.query_row("SELECT name FROM dir WHERE id='d1'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "LocalNew");
    }

    #[test]
    fn singleton_remote_apply_skips_stale_value_behind_unpushed_local_edit() {
        let mut conn = singleton_guard_conn();
        let (_, local_pt, _) = latest_read_pos_event(&conn);
        let stale_pt = local_pt.saturating_sub(1);

        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", stale_pt, 0)).unwrap();
        tx.commit().unwrap();

        assert_eq!(read_pos_raw(&conn), "epubcfi(/6/2)");
    }

    #[test]
    fn singleton_remote_apply_accepts_newer_value_than_unpushed_local_edit() {
        let mut conn = singleton_guard_conn();
        let (_, local_pt, local_l) = latest_read_pos_event(&conn);

        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", local_pt + 1, local_l)).unwrap();
        tx.commit().unwrap();

        assert_eq!(read_pos_raw(&conn), "epubcfi(/6/4)");
    }

    #[test]
    fn singleton_guard_only_considers_events_after_push_watermark() {
        let mut conn = singleton_guard_conn();
        let (local_seq, local_pt, _) = latest_read_pos_event(&conn);
        mark_events_pushed(&conn, local_seq).unwrap();

        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/6)", local_pt.saturating_sub(1), 0)).unwrap();
        tx.commit().unwrap();

        assert_eq!(read_pos_raw(&conn), "epubcfi(/6/6)");
    }

    #[test]
    fn persisted_reading_state_is_validated_in_rust() {
        let conn = fk_conn();
        conn.execute("INSERT INTO book(id, title, added_at, read_pos) VALUES ('1', 'Book', 1, 'not-a-cfi')", []).unwrap();
        assert!(get_book_reading_position(&conn, BookId::new(1)).is_err());

        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Shelf')", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('d1', '1', 'book.epub', 'fp', ?1, 1)", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("UPDATE book SET read_progress=150, last_read=1 WHERE id='1'", []).unwrap();
        assert!(get_recently_read(&conn, 10).is_err());
    }

    #[test]
    fn invalid_remote_reading_position_is_rejected_before_storage() {
        let mut conn = fk_conn();
        conn.insert_book("1", "Book", 1).unwrap();
        let tx = conn.transaction().unwrap();
        assert!(apply_remote_change_in_tx(&tx, &reading_position_change("not-a-cfi", 10, 0)).is_err());
        drop(tx);
        let stored: Option<String> = conn.query_row("SELECT read_pos FROM book WHERE id='1'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, None);
    }

    #[test]
    fn scalar_registers_arriving_before_book_lifecycle_are_preserved() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", 10, 0)).unwrap();
        apply_remote_change_in_tx(&tx, &server_change(SyncOp::BookOpened { book_id: BookId::new(1), last_read: 11 }, 11, 0)).unwrap();
        apply_remote_change_in_tx(&tx, &server_change(SyncOp::BookAdded { book_id: BookId::new(1), title: "Book".to_string(), added_at: 12 }, 12, 0)).unwrap();
        tx.commit().unwrap();

        let state: (Option<String>, i64) = conn.query_row("SELECT read_pos, last_read FROM book WHERE id='1'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(state, (Some("epubcfi(/6/4)".to_string()), 11));
    }

    #[test]
    fn upgraded_empty_default_stamps_the_generated_event_identity() {
        let conn = fk_conn();
        conn.execute("INSERT INTO book(id, title, added_at) VALUES ('1', 'Book', 1)", []).unwrap();
        conn.execute("DELETE FROM event_log", []).unwrap();
        conn.execute("UPDATE book SET hlc_event='', hlc_seq=0 WHERE id='1'", []).unwrap();

        conn.execute(
            "INSERT INTO event_log(event_id, entity_id, event_type, payload, timestamp)
             VALUES ('', '1', 'book_added', '{\"title\":\"Book\",\"added_at\":1}', 1)",
            [],
        )
        .unwrap();

        let (persisted, stamped): (String, String) = conn
            .query_row(
                "SELECT event_log.event_id, book.hlc_event
                 FROM event_log JOIN book ON book.id=event_log.entity_id",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stamped, persisted);
        EventId::parse(&stamped).expect("the entity version must carry the generated mutation id");
    }

    #[test]
    fn deferred_pull_tx_allows_book_dir_before_dir_in_same_batch() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

        apply_remote_change_in_tx(&tx, &book_dir_add_change(BookId::new(1), "late-dir", "f.epub", 10)).unwrap();
        apply_remote_change_in_tx(&tx, &dir_added_change("late-dir", "__root__", "Late", 11)).unwrap();
        tx.commit().unwrap();

        let present: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE book_id='1' AND dir_id='late-dir' AND file_name='f.epub' AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(present, 1);
    }

    #[test]
    fn compacted_dir_state_can_arrive_after_book_dir_add_in_same_batch() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

        apply_remote_change_in_tx(&tx, &book_dir_add_change(BookId::new(1), "d-final", "f.epub", 10)).unwrap();
        apply_remote_change_in_tx(&tx, &dir_state_change("d-final", "__root__", "Final", false, 11)).unwrap();
        tx.commit().unwrap();

        let row: (String, i64) = conn.query_row("SELECT d.name, COUNT(bd.book_id) FROM dir d JOIN book_dir bd ON bd.dir_id=d.id WHERE d.id='d-final' AND bd.book_id='1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(row, ("Final".to_string(), 1));
    }

    #[test]
    fn deferred_pull_tx_allows_child_dir_before_parent_in_same_batch() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

        apply_remote_change_in_tx(&tx, &dir_added_change("child-dir", "parent-dir", "Child", 10)).unwrap();
        apply_remote_change_in_tx(&tx, &dir_added_change("parent-dir", "__root__", "Parent", 11)).unwrap();
        tx.commit().unwrap();

        let parent: String = conn.query_row("SELECT parent_id FROM dir WHERE id='child-dir'", [], |r| r.get(0)).unwrap();
        assert_eq!(parent, "parent-dir");
    }

    #[test]
    fn deferred_pull_tx_fails_loudly_when_dependency_missing_at_commit() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

        apply_remote_change_in_tx(&tx, &book_dir_add_change(BookId::new(1), "missing-dir", "f.epub", 10)).unwrap();

        assert!(tx.commit().is_err(), "unclosed pull batch must fail at commit");
    }

    #[test]
    fn remote_book_dir_into_soft_deleted_dir_still_applies() {
        let mut conn = fk_conn();
        conn.execute("INSERT INTO dir(id, parent_id, name, deleted_at) VALUES ('soft-dir', '__root__', 'Soft', 1)", []).unwrap();
        let tx = conn.transaction().unwrap();
        tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

        apply_remote_change_in_tx(&tx, &book_dir_add_change(BookId::new(1), "soft-dir", "f.epub", 10)).unwrap();
        tx.commit().unwrap();

        let present: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE book_id='1' AND dir_id='soft-dir' AND file_name='f.epub' AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(present, 1);
    }

    #[test]
    fn delete_dir_cascades_subtree_events_files_and_leaves_siblings() {
        let mut conn = fk_conn();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('p', '__root__', 'Parent')", []).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('a', 'p', 'A')", []).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('b', 'a', 'B')", []).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('s', '__root__', 'Sibling')", []).unwrap();
        conn.execute("INSERT INTO book(id, title, added_at, last_read) VALUES ('1', 'One', 1, 10), ('2', 'Two', 1, 20), ('3', 'Three', 1, 30)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('a', '1', 'a.epub', 'ha', 'H1', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('b', '2', 'b.epub', 'hb', 'H2', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('s', '3', 's.epub', 'hs', 'H3', 1)", []).unwrap();
        conn.execute("DELETE FROM event_log", []).unwrap();

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("Parent").join("A").join("B")).unwrap();
        std::fs::create_dir_all(temp.path().join("Sibling")).unwrap();
        std::fs::write(temp.path().join("Parent").join(".biblos_uuid"), "p").unwrap();
        std::fs::write(temp.path().join("Parent").join("A").join(".biblos_uuid"), "a").unwrap();
        std::fs::write(temp.path().join("Parent").join("A").join("B").join(".biblos_uuid"), "b").unwrap();
        std::fs::write(temp.path().join("Sibling").join(".biblos_uuid"), "s").unwrap();
        std::fs::write(temp.path().join("Parent").join("A").join("a.epub"), b"a").unwrap();
        std::fs::write(temp.path().join("Parent").join("A").join("B").join("b.epub"), b"b").unwrap();
        std::fs::write(temp.path().join("Sibling").join("s.epub"), b"s").unwrap();

        let deleted = delete_dir(&mut conn, &test_dir_id("a")).unwrap();
        remove_deleted_dir_artifacts(temp.path(), &deleted).unwrap();

        let tombstoned_dirs: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE id IN ('a','b') AND deleted_at IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(tombstoned_dirs, 2);
        let tombstoned_placements: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE book_id IN ('1','2') AND deleted_at IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(tombstoned_placements, 2);
        assert!(is_alive(&conn, "s", "3", "s.epub"));

        let deleted_dir_events: i64 = conn.query_row("SELECT COUNT(*) FROM event_log WHERE event_type='dir_state' AND json_extract(payload, '$.deleted') = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(deleted_dir_events, 2);
        let removed_placement_events: i64 = conn.query_row("SELECT COUNT(*) FROM event_log WHERE event_type='book_dir_removed'", [], |r| r.get(0)).unwrap();
        assert_eq!(removed_placement_events, 2);

        assert!(!temp.path().join("Parent").join("A").join("a.epub").exists());
        assert!(!temp.path().join("Parent").join("A").join("B").join("b.epub").exists());
        assert!(!temp.path().join("Parent").join("A").exists());
        assert!(temp.path().join("Parent").exists());
        assert!(temp.path().join("Sibling").join("s.epub").exists());
        assert_eq!(count_all_books(&conn), 1);
        let recent_ids: Vec<BookId> = get_recently_read(&conn, 10).unwrap().into_iter().map(|book| book.id()).collect();
        assert_eq!(recent_ids, vec![BookId::new(3)]);

        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('a2', 'p', 'A')", []).unwrap();
        let recreated: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE id='a2' AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(recreated, 1);
    }

    #[test]
    fn materialize_dir_with_markers_writes_nested_markers_idempotently() {
        let conn = fk_conn();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d2', 'd1', 'B')", []).unwrap();
        let temp = tempfile::tempdir().unwrap();

        let path = materialize_dir_with_markers(&conn, temp.path(), "d2").unwrap().unwrap();
        assert_eq!(path, temp.path().join("A").join("B"));
        assert_eq!(std::fs::read_to_string(temp.path().join("A").join(".biblos_uuid")).unwrap(), "d1");
        assert_eq!(std::fs::read_to_string(temp.path().join("A").join("B").join(".biblos_uuid")).unwrap(), "d2");

        std::fs::write(temp.path().join("A").join("B").join(".biblos_uuid"), "custom").unwrap();
        assert!(materialize_dir_with_markers(&conn, temp.path(), "d2").is_err());
        assert_eq!(std::fs::read_to_string(temp.path().join("A").join("B").join(".biblos_uuid")).unwrap(), "custom");
    }

    #[test]
    fn client_create_directory_commits_nested_database_and_filesystem_identity() {
        let mut conn = fk_conn();
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let root_id = test_dir_id("__root__");
        let parent = create_local_directory(&mut conn, &library_root, &root_id, &test_file_name("Parent")).unwrap();
        let child = create_local_directory(&mut conn, &library_root, parent.directory.id(), &test_file_name("Child")).unwrap();

        assert_eq!(child.relative_path.as_str(), "Parent/Child");
        assert_eq!(fs::read_to_string(root.path().join("Parent/.biblos_uuid")).unwrap(), parent.directory.id().as_str());
        assert_eq!(fs::read_to_string(root.path().join("Parent/Child/.biblos_uuid")).unwrap(), child.directory.id().as_str());
        let events: i64 = conn.query_row("SELECT COUNT(*) FROM event_log WHERE event_type='dir_state' AND json_extract(payload, '$.deleted')=0", [], |row| row.get(0)).unwrap();
        assert_eq!(events, 2);
    }

    #[test]
    fn client_create_directory_refuses_untracked_filesystem_entries_and_rolls_back_database_state() {
        let mut conn = fk_conn();
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Existing")).unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let result = create_local_directory(&mut conn, &library_root, &test_dir_id("__root__"), &test_file_name("Existing"));

        assert!(result.is_err());
        let stored: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE name='Existing'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, 0);
        assert!(!root.path().join("Existing/.biblos_uuid").exists());
    }

    #[test]
    fn client_create_directory_rejects_duplicate_names_without_damaging_the_original() {
        let mut conn = fk_conn();
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let root_id = test_dir_id("__root__");
        let original = create_local_directory(&mut conn, &library_root, &root_id, &test_file_name("Shelf")).unwrap();

        assert!(create_local_directory(&mut conn, &library_root, &root_id, &test_file_name("Shelf")).is_err());
        let stored: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE name='Shelf' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, 1);
        assert_eq!(fs::read_to_string(root.path().join("Shelf/.biblos_uuid")).unwrap(), original.directory.id().as_str());
    }

    #[test]
    fn client_create_directory_rejects_a_missing_parent_without_touching_disk() {
        let mut conn = fk_conn();
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();

        assert!(create_local_directory(&mut conn, &library_root, &test_dir_id("missing"), &test_file_name("Child")).is_err());
        assert!(!root.path().join("Child").exists());
        let stored: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE name='Child'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, 0);
    }

    #[test]
    fn stamp_book_dir_local_hash_makes_scanner_fast_lookup_hit() {
        let conn = fk_conn();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at) VALUES ('1', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('d1', '1', 'f.epub', 'old', ?1, 1)", [TEST_CONTENT_HASH]).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("A");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f.epub");
        std::fs::write(&file, b"epub bytes").unwrap();

        stamp_book_dir_local_hash(&conn, "d1", BookId::new(1), "f.epub", &file).unwrap();
        let hash = local_file_fingerprint(&file).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        let found = get_book_dir_by_hash(&tx, hash).unwrap().unwrap();
        assert_eq!(found.book_id, BookId::new(1));
        assert_eq!(found.content_hash, test_content_hash());
    }

    #[test]
    fn invalid_legacy_content_hash_forces_scanner_rehash() {
        let conn = new_schema_conn();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash) VALUES ('d1', '1', 'f.epub', '7', 'legacy-key')", []).unwrap();
        let tx = conn.unchecked_transaction().unwrap();

        assert!(get_book_dir_by_hash(&tx, 7).unwrap().is_none());
    }

    #[test]
    fn invalid_legacy_transfer_jobs_fail_closed_and_remain_visible() {
        let conn = fk_conn();
        conn.execute_batch("PRAGMA ignore_check_constraints=ON;").unwrap();
        conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('upload_blob', 'legacy-key', '/f.epub', '/tmp/f.epub', '1', 0, 0)", []).unwrap();
        conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('book_download', ?1, '/g.epub', '/tmp/g.epub', '1', 1, 1)", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('download_blob', ?1, '/h.epub', '/tmp/h.epub', '', 2, 2)", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('download_blob', ?1, '/../escape.epub', '/tmp/escape.epub', '1', 3, 3)", [TEST_CONTENT_HASH]).unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints=OFF;").unwrap();

        assert!(claim_due_transfer_job(&conn, true).is_err());
        let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM transfer_jobs", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 4);
    }

    #[test]
    fn reclaimed_transfer_claim_is_fenced_from_the_old_worker() {
        let conn = fk_conn();
        let hash = test_content_hash();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &hash, "", "/tmp/book.epub", None, TransferOrigin::Background).unwrap();
        let first = claim_due_transfer_job(&conn, true).unwrap().unwrap();
        conn.execute("UPDATE transfer_jobs SET claimed_at=0 WHERE id=?1", [first.id]).unwrap();
        reset_stale_claims(&conn, 1).unwrap();
        let second = claim_due_transfer_job(&conn, true).unwrap().unwrap();
        assert_ne!(first.claim_token, second.claim_token);

        complete_transfer_job(&conn, first.id, &first.claim_token).unwrap();
        let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM transfer_jobs", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 1, "an expired worker must not complete the new owner's claim");

        fail_transfer_job(&conn, first.id, &first.claim_token, first.attempts, "late failure").unwrap();
        let owner: String = conn.query_row("SELECT claim_token FROM transfer_jobs", [], |row| row.get(0)).unwrap();
        assert_eq!(owner, second.claim_token, "an expired worker must not reschedule the new owner's claim");
        complete_transfer_job(&conn, second.id, &second.claim_token).unwrap();
        let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM transfer_jobs", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn durable_transfer_statuses_follow_queue_claim_and_retry_state() {
        let conn = fk_conn();
        let hash = test_content_hash();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &hash, "books/test.epub", "/tmp/test.epub", Some(BookId::new(7)), TransferOrigin::UserInitiated).unwrap();

        let queued = get_transfer_statuses(&conn).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].state, TransferState::Queued);
        assert_eq!(queued[0].book_id.as_deref(), Some("7"));
        assert_eq!(queued[0].path.as_deref(), Some("books/test.epub"));

        let claimed = claim_due_transfer_job(&conn, true).unwrap().unwrap();
        assert_eq!(get_transfer_statuses(&conn).unwrap()[0].state, TransferState::Running);

        fail_transfer_job(&conn, claimed.id, &claimed.claim_token, claimed.attempts, "network unavailable").unwrap();
        let retrying = get_transfer_statuses(&conn).unwrap();
        assert_eq!(retrying[0].state, TransferState::Retrying);
        assert_eq!(retrying[0].last_error.as_deref(), Some("network unavailable"));
        assert!(retrying[0].next_retry_at.is_some());
    }

    #[test]
    fn metered_gate_skips_background_jobs_but_never_user_initiated_jobs() {
        let conn = fk_conn();
        let background_hash = test_content_hash();
        let user_hash = ContentHash::parse("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &background_hash, "", "/tmp/background.epub", None, TransferOrigin::Background).unwrap();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &user_hash, "", "/tmp/user.epub", None, TransferOrigin::UserInitiated).unwrap();

        let manual = claim_due_transfer_job(&conn, false).unwrap().expect("manual transfer must remain eligible");
        assert_eq!(manual.job.origin(), TransferOrigin::UserInitiated);
        complete_transfer_job(&conn, manual.id, &manual.claim_token).unwrap();
        assert!(claim_due_transfer_job(&conn, false).unwrap().is_none(), "background transfer must pause on a disallowed metered connection");

        let background = claim_due_transfer_job(&conn, true).unwrap().expect("background transfer must resume when policy permits it");
        assert_eq!(background.job.origin(), TransferOrigin::Background);
    }

    #[test]
    fn user_initiated_origin_upgrades_and_cannot_be_downgraded() {
        let conn = fk_conn();
        let hash = test_content_hash();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &hash, "", "/tmp/book.epub", None, TransferOrigin::Background).unwrap();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &hash, "", "/tmp/book.epub", None, TransferOrigin::UserInitiated).unwrap();
        upsert_transfer_job(&conn, TransferJobKind::UploadBook, &hash, "", "/tmp/book.epub", None, TransferOrigin::Background).unwrap();

        let statuses = get_transfer_statuses(&conn).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].origin, TransferOrigin::UserInitiated);
    }

    #[test]
    fn sqlite_rejects_blob_values_outside_the_rust_domains() {
        let conn = fk_conn();

        assert!(conn.execute("INSERT INTO asset_remote_state(kind, hash, presence) VALUES ('cover', ?1, 1)", [TEST_CONTENT_HASH]).is_err());
        assert!(conn.execute("INSERT INTO asset_remote_state(kind, hash, presence) VALUES ('book', 'legacy-key', 1)", []).is_err());
        assert!(conn.execute("INSERT INTO asset_remote_state(kind, hash, presence) VALUES ('book', ?1, 9)", [TEST_CONTENT_HASH]).is_err());
        assert!(conn.execute("INSERT INTO transfer_jobs(kind, key1, local_path, created_at, updated_at) VALUES ('download_blob', ?1, '/tmp/f', 0, 0)", [TEST_CONTENT_HASH]).is_err());
        assert!(conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('upload_blob', ?1, '/f.epub', '/tmp/f', '', 0, 0)", [TEST_CONTENT_HASH]).is_err());
        assert!(conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('upload_blob', ?1, '', '/tmp/f', '1', 0, 0)", [TEST_CONTENT_HASH]).is_err());
        assert!(conn.execute("INSERT INTO transfer_jobs(kind, key1, rel_path, local_path, book_id, created_at, updated_at) VALUES ('download_thumbnail', ?1, '/unexpected', '', '1', 0, 0)", [TEST_CONTENT_HASH]).is_err());
    }

    #[test]
    fn local_and_remote_asset_facts_are_independent_and_namespaced() {
        let conn = fk_conn();
        let content_hash = test_content_hash();

        upsert_blob_local_ref(&conn, BlobKind::Book, &content_hash, "/book.epub", true).unwrap();
        mark_asset_remote_missing(&conn, BlobKind::Book, &content_hash).unwrap();

        assert_eq!(get_locally_present_blob_hashes(&conn, BlobKind::Book).unwrap(), vec![content_hash.clone()]);
        assert!(get_locally_present_blob_hashes(&conn, BlobKind::Thumbnail).unwrap().is_empty());
        assert!(is_blob_local(&conn, BlobKind::Book, &content_hash).unwrap());
        assert!(!is_blob_local(&conn, BlobKind::Thumbnail, &content_hash).unwrap());
        let remote_presence: i64 = conn.query_row("SELECT presence FROM asset_remote_state WHERE kind='book' AND hash=?1", [content_hash.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(remote_presence, 1, "a local copy must not imply remote presence");
        let thumbnail_remote_rows: i64 = conn.query_row("SELECT COUNT(*) FROM asset_remote_state WHERE kind='thumbnail' AND hash=?1", [content_hash.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(thumbnail_remote_rows, 0);
    }

    #[test]
    fn legacy_asset_state_and_json_download_intent_migrate_without_loss() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sync_metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE blob_state(
                 kind TEXT NOT NULL,
                 hash TEXT NOT NULL,
                 state INTEGER NOT NULL,
                 last_checked INTEGER,
                 last_error TEXT,
                 PRIMARY KEY(kind, hash)
             );",
        )
        .unwrap();
        conn.execute("INSERT INTO blob_state(kind, hash, state, last_checked) VALUES ('book', ?1, 2, 17)", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO sync_metadata(key, value) VALUES ('pending_download_hashes', ?1)", [serde_json::json!([TEST_CONTENT_HASH]).to_string()]).unwrap();

        init_db(&conn).unwrap();

        let presence: i64 = conn.query_row("SELECT presence FROM asset_remote_state WHERE kind='book' AND hash=?1", [TEST_CONTENT_HASH], |row| row.get(0)).unwrap();
        assert_eq!(presence, 2);
        assert_eq!(get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::Background)]);
        assert_eq!(get_sync_metadata(&conn, "pending_download_hashes").unwrap(), None);
        let legacy_exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='blob_state')", [], |row| row.get(0)).unwrap();
        assert!(!legacy_exists);
    }

    #[test]
    fn completing_one_asset_request_cannot_erase_a_concurrent_request() {
        let conn = fk_conn();
        let first = test_content_hash();
        let second = ContentHash::parse(&"b".repeat(64)).unwrap();
        add_asset_request(&conn, BlobKind::Book, &first, TransferOrigin::Background).unwrap();
        add_asset_request(&conn, BlobKind::Book, &second, TransferOrigin::Background).unwrap();
        complete_asset_request(&conn, BlobKind::Book, &first).unwrap();

        assert_eq!(get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(second, TransferOrigin::Background)]);
    }

    #[test]
    fn placement_publication_guard_requires_exact_live_book_path_and_hash() {
        let conn = fk_conn();
        let hash = test_content_hash();
        let placement = BookPlacement::new(BookId::new(7), RelativeBookPath::parse("/A/f.epub").unwrap());
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at) VALUES ('7', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash) VALUES ('d1', '7', 'f.epub', '', ?1)", [hash.as_str()]).unwrap();

        assert!(book_placement_expects_hash(&conn, &placement, &hash).unwrap());
        conn.execute("UPDATE book_dir SET content_hash=?1", ["b".repeat(64)]).unwrap();
        assert!(!book_placement_expects_hash(&conn, &placement, &hash).unwrap());
        conn.execute("UPDATE book_dir SET content_hash=?1, deleted_at=1", [hash.as_str()]).unwrap();
        assert!(!book_placement_expects_hash(&conn, &placement, &hash).unwrap());
    }

    #[test]
    fn typed_download_identity_marks_and_stamps_the_matching_placement() {
        let conn = fk_conn();
        let book_id = BookId::new(7);
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at) VALUES ('7', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('d1', '7', 'f.epub', 'old', 'HASH', 0)", []).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("f.epub");
        std::fs::write(&file, b"downloaded bytes").unwrap();

        let rel_path = RelativeBookPath::parse("/A/f.epub").unwrap();
        stamp_book_dir_local_hash_by_rel_path(&conn, &rel_path, book_id, &file).unwrap();
        set_book_downloaded(&conn, &rel_path, book_id, true).unwrap();

        assert!(is_book_downloaded(&conn, book_id).unwrap());
        let stored: String = conn.query_row("SELECT local_hash FROM book_dir WHERE book_id = '7'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, local_file_fingerprint(&file).unwrap().to_string());
    }

    #[test]
    fn remote_book_dir_add_remove_converge_regardless_of_order() {
        // The #4 divergence: device A applied B's add last (present), device B
        // applied A's remove last (absent). With the HLC gate, the higher-HLC op
        // (remove@20) wins no matter the application order.
        let add_then_remove = final_present(&[(true, 10, 0), (false, 20, 0)]);
        let remove_then_add = final_present(&[(false, 20, 0), (true, 10, 0)]);
        assert_eq!(add_then_remove, remove_then_add, "order must not change the outcome");
        assert!(!add_then_remove, "higher-HLC remove must win → absent");

        // Symmetric case: a newer add beats an older remove, both orders.
        assert!(final_present(&[(false, 20, 0), (true, 30, 0)]));
        assert!(final_present(&[(true, 30, 0), (false, 20, 0)]));

        // Exact HLC tie: remove wins deterministically, both orders → absent.
        assert!(!final_present(&[(true, 15, 0), (false, 15, 0)]));
        assert!(!final_present(&[(false, 15, 0), (true, 15, 0)]));
    }

    #[test]
    fn winning_add_cancels_queued_file_removal() {
        let mut conn = lww_conn();
        let tx = conn.transaction().unwrap();
        let dir_id = test_dir_id("d1");
        let file_name = test_file_name("f.epub");
        apply_remote_book_dir(&tx, &book_dir_change(false, 10, 0), BookId::new(1), &dir_id, &file_name, RemotePlacementState::Removed).unwrap();
        let queued: i64 = tx.query_row("SELECT COUNT(*) FROM pending_fs_operations", [], |row| row.get(0)).unwrap();
        assert_eq!(queued, 1);

        let content_hash = test_content_hash();
        apply_remote_book_dir(&tx, &book_dir_change(true, 20, 0), BookId::new(1), &dir_id, &file_name, RemotePlacementState::Present(&content_hash)).unwrap();
        let queued: i64 = tx.query_row("SELECT COUNT(*) FROM pending_fs_operations", [], |row| row.get(0)).unwrap();
        assert_eq!(queued, 0, "a newer live placement must supersede stale cleanup");
        tx.commit().unwrap();
    }

    #[test]
    fn remote_directory_delete_is_durably_drained() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let added = dir_state_change("d1", "__root__", "Shelf", false, 10);
        let deleted = server_change_from(SyncOp::DirState { dir_id: test_dir_id("d1"), directory: DirectoryState::Deleted { parent_id: test_dir_id("__root__"), name: test_file_name("Shelf") } }, 20, 0, "remote", 2, 2);
        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &added).unwrap();
        apply_remote_change_in_tx(&tx, &deleted).unwrap();
        tx.commit().unwrap();

        let root = tempfile::tempdir().unwrap();
        let shelf = root.path().join("Shelf");
        fs::create_dir(&shelf).unwrap();
        fs::write(shelf.join(".biblos_uuid"), "d1").unwrap();
        assert_eq!(pending_fs_operation_count(&conn).unwrap(), 1);

        drain_pending_fs_operations(&conn, root.path()).unwrap();
        assert!(!shelf.exists());
        assert_eq!(pending_fs_operation_count(&conn).unwrap(), 0);
        drain_pending_fs_operations(&conn, root.path()).unwrap();
    }

    #[test]
    fn pending_filesystem_rows_are_validated_when_decoded() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO pending_fs_operations(kind, dir_id, book_id, file_name, rel_path, created_at) VALUES ('dir_remove', 'd1', '1', 'f.epub', 'Shelf', 1)", []).unwrap();
        assert!(pending_fs_operations(&conn).is_err());

        conn.execute("DELETE FROM pending_fs_operations", []).unwrap();
        conn.execute("INSERT INTO pending_fs_operations(kind, dir_id, book_id, file_name, rel_path, created_at) VALUES ('book_file_remove', 'd1', '1', 'f.epub', '../f.epub', 1)", []).unwrap();
        assert!(pending_fs_operations(&conn).is_err());
        let queued: i64 = conn.query_row("SELECT COUNT(*) FROM pending_fs_operations", [], |row| row.get(0)).unwrap();
        assert_eq!(queued, 1, "invalid persisted work must remain queued rather than being treated as completed");
    }

    #[test]
    fn pending_removal_checks_propagate_database_errors() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("DROP TABLE pending_fs_operations", []).unwrap();
        let tx = conn.transaction().unwrap();
        assert!(is_book_file_pending_removal_tx(&tx, "d1", "f.epub").is_err());
    }

    #[test]
    fn pending_filesystem_drain_requires_an_available_library_root() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO pending_fs_operations(kind, dir_id, rel_path, created_at) VALUES ('dir_remove', 'd1', 'Shelf', 1)", []).unwrap();
        let missing_root = tempfile::tempdir().unwrap().path().join("missing");
        assert!(drain_pending_fs_operations(&conn, &missing_root).is_err());
        let queued: i64 = conn.query_row("SELECT COUNT(*) FROM pending_fs_operations", [], |row| row.get(0)).unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn pending_directory_removal_requires_the_expected_marker_identity() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO pending_fs_operations(kind, dir_id, rel_path, created_at) VALUES ('dir_remove', 'd1', 'Shelf', 1)", []).unwrap();
        let root = tempfile::tempdir().unwrap();
        let shelf = root.path().join("Shelf");
        fs::create_dir(&shelf).unwrap();
        fs::write(shelf.join(".biblos_uuid"), "different-directory").unwrap();

        assert!(drain_pending_fs_operations(&conn, root.path()).is_err());
        assert!(shelf.exists());
        assert_eq!(fs::read_to_string(shelf.join(".biblos_uuid")).unwrap(), "different-directory");
        let queued: i64 = conn.query_row("SELECT COUNT(*) FROM pending_fs_operations", [], |row| row.get(0)).unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn remote_book_delete_creates_a_versioned_tombstone_before_any_add() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let book_id = BookId::new(77);
        let deletion = server_change_from(SyncOp::BookDeleted { book_id }, 10, 0, "delete-device", 1, 1);
        let stale_add = server_change_from(SyncOp::BookAdded { book_id, title: "Stale".into(), added_at: 1 }, 9, 0, "add-device", 1, 2);

        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &deletion).unwrap();
        apply_remote_change_in_tx(&tx, &stale_add).unwrap();
        tx.commit().unwrap();

        let deleted: bool = conn.query_row("SELECT deleted_at IS NOT NULL FROM book WHERE id='77'", [], |row| row.get(0)).unwrap();
        assert!(deleted, "an unseen deletion must survive a later stale add");
    }

    #[test]
    fn resolve_removed_and_live_placements_support_relocation() {
        // The inputs the move (relocate-on-delete) decision relies on.
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('dx','__root__','X')", []).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('dy','__root__','Y')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at) VALUES ('1', 1)", []).unwrap();
        // Old placement: tombstoned (the move's source), holding content H.
        conn.execute("INSERT INTO book_dir(dir_id,book_id,file_name,local_hash,content_hash,is_downloaded,deleted_at) VALUES ('dx','1','f.epub','fp',?1,1,1)", [TEST_CONTENT_HASH]).unwrap();
        // New placement: live (the move's target), same content, no file yet.
        conn.execute("INSERT INTO book_dir(dir_id,book_id,file_name,local_hash,content_hash,is_downloaded) VALUES ('dy','1','f.epub',?1,?1,0)", [TEST_CONTENT_HASH]).unwrap();

        // A tombstoned placement resolves with its path and content hash.
        assert_eq!(resolve_removed_book_dir(&conn, "dx", BookId::new(1), "f.epub").unwrap(), Some((RelativeBookPath::parse("/X/f.epub").unwrap(), Some(test_content_hash()))));
        // A live placement does not resolve as removed (so its file is never touched).
        assert_eq!(resolve_removed_book_dir(&conn, "dy", BookId::new(1), "f.epub").unwrap(), None);
        // Live placements of content H = the relocation target, excluding the tombstone.
        assert_eq!(get_book_paths_by_content_hash(&conn, &test_content_hash()).unwrap(), vec![BookPath { book_id: BookId::new(1), rel_path: RelativeBookPath::parse("/Y/f.epub").unwrap() }]);
    }

    #[test]
    fn carousels_hide_books_with_no_live_placement() {
        // A book whose every placement is removed keeps its metadata row but must
        // not surface in the carousels (un-openable), and must reappear when a
        // placement returns.
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1','__root__','F')", []).unwrap();
        conn.execute("INSERT INTO book(id, title, added_at, last_read) VALUES ('1','T', 5, 100)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id,book_id,file_name,local_hash,content_hash,is_downloaded) VALUES ('d1','1','f.epub','fp','H',1)", []).unwrap();

        let shows = |connection: &Connection| get_recently_read(connection, 10).unwrap().iter().any(|book| book.id() == BookId::new(1));
        assert!(shows(&conn), "book with a live placement must appear");
        assert_eq!(count_all_books(&conn), 1);

        // Remove the only placement: the row survives but drops out of the carousel.
        conn.execute("UPDATE book_dir SET deleted_at=1 WHERE book_id='1'", []).unwrap();
        assert!(!shows(&conn), "orphaned book must not appear in the carousel");
        assert_eq!(count_all_books(&conn), 0, "count must match what is displayed");
        let row_kept: i64 = conn.query_row("SELECT COUNT(*) FROM book WHERE id='1' AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(row_kept, 1, "the book metadata row must be preserved, not tombstoned");

        // A returning placement makes it reappear with no resurrection plumbing.
        conn.execute("UPDATE book_dir SET deleted_at=NULL WHERE book_id='1'", []).unwrap();
        assert!(shows(&conn), "book must reappear once a live placement returns");
    }

    fn count_all_books(conn: &Connection) -> i64 {
        let mut n = 0;
        conn.count_all_books(|r| {
            n = r.get(0)?;
            Ok(())
        })
        .unwrap();
        n
    }

    #[test]
    fn winning_remote_removal_reports_applied_for_file_deletion() {
        let mut conn = lww_conn();
        let tx = conn.transaction().unwrap();
        let dir_id = test_dir_id("d1");
        let file_name = test_file_name("f.epub");
        // Present placement.
        apply_remote_book_dir(&tx, &book_dir_change(true, 10, 0), BookId::new(1), &dir_id, &file_name, RemotePlacementState::Present(&test_content_hash())).unwrap();
        // Higher-HLC removal wins -> caller should delete the local file.
        let applied = apply_remote_book_dir(&tx, &book_dir_change(false, 20, 0), BookId::new(1), &dir_id, &file_name, RemotePlacementState::Removed).unwrap();
        assert!(applied, "winning removal must report applied so the file is deleted");
        // A stale (lower-HLC) removal loses -> no deletion.
        let stale = apply_remote_book_dir(&tx, &book_dir_change(false, 5, 0), BookId::new(1), &dir_id, &file_name, RemotePlacementState::Removed).unwrap();
        assert!(!stale, "stale removal must not trigger a deletion");
        tx.commit().unwrap();
    }

    #[test]
    fn sweep_deletes_vanished_after_grace_keeps_present_and_spares_undownloaded() {
        // VANISH_GRACE_SCANS = 2: a file must be missing for >= 2 scans.
        let conn = new_schema_conn();
        let current = 10_i64;
        conn.execute_batch(
            "INSERT INTO book_dir (dir_id, book_id, file_name, local_hash, content_hash, is_downloaded, last_scan) VALUES
                 ('d1','b1','keep.epub','fp1','H1',1,10),    -- present: stamped this scan (delta 0)
                 ('d1','b4','blip.epub','fp4','H4',1,9),     -- missed once (delta 1): within grace
                 ('d1','b2','gone.epub','fp2','H2',1,8),     -- missed twice (delta 2): past grace
                 ('d1','b2','moved.epub','fp9','H2',1,10),   -- same book relocated, seen this scan
                 ('d2','b3','remote.epub','H3','H3',0,0),    -- synced, never downloaded
                 ('d2','b5','fresh.epub','H5','H5',1,0);     -- downloaded, never scanned (last_scan 0)",
        )
        .unwrap();

        mark_vanished_book_dirs_deleted(&conn, current).unwrap();

        assert!(is_alive(&conn, "d1", "b1", "keep.epub"), "present file must survive");
        assert!(is_alive(&conn, "d1", "b4", "blip.epub"), "single transient miss must survive the grace window");
        assert!(!is_alive(&conn, "d1", "b2", "gone.epub"), "file missing past the grace window must be deleted");
        // A move is exactly this pair: old placement gone, new placement kept.
        assert!(is_alive(&conn, "d1", "b2", "moved.epub"), "relocated file's new path must survive");
        assert!(is_alive(&conn, "d2", "b3", "remote.epub"), "synced-but-undownloaded placement must never be swept");
        assert!(is_alive(&conn, "d2", "b5", "fresh.epub"), "downloaded-but-never-scanned file must not be deleted");

        // The blip is the one pending candidate awaiting a re-scan.
        assert_eq!(count_pending_vanish_candidates(&conn, current).unwrap(), 1);
    }

    #[test]
    fn migration_rebuilds_onto_path_key_and_preserves_rows() {
        let conn = old_schema_conn();
        // Under the old key, the same content (one local_hash) in two folders
        // could only be stored once. Seed a single legacy row.
        conn.execute("INSERT INTO book_dir (dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('d1','b1','f.epub','fp1','HASH',1)", []).unwrap();

        migrate_book_dir_primary_key(&conn).unwrap();

        // Row survived the rebuild with its columns intact.
        assert_eq!(count_rows(&conn), 1);
        let downloaded: i64 = conn.query_row("SELECT is_downloaded FROM book_dir WHERE dir_id='d1'", [], |r| r.get(0)).unwrap();
        assert_eq!(downloaded, 1);

        // The new key is the path triple: the same content in a second folder is
        // now a distinct placement (would have failed the old local_hash PK).
        conn.execute("INSERT INTO book_dir (dir_id, book_id, file_name, local_hash, content_hash) VALUES ('d2','b1','f.epub','fp1','HASH')", []).unwrap();
        assert_eq!(count_rows(&conn), 2);

        // Re-inserting the same path triple still conflicts (one row per path).
        let dup = conn.execute("INSERT INTO book_dir (dir_id, book_id, file_name, local_hash, content_hash) VALUES ('d1','b1','f.epub','fp1','HASH')", []);
        assert!(dup.is_err(), "duplicate path triple must violate the primary key");
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = old_schema_conn();
        migrate_book_dir_primary_key(&conn).unwrap();
        let ddl_after_first: String = conn.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='book_dir'", [], |r| r.get(0)).unwrap();
        // A second run sees the already-migrated DDL and does nothing.
        migrate_book_dir_primary_key(&conn).unwrap();
        let ddl_after_second: String = conn.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='book_dir'", [], |r| r.get(0)).unwrap();
        assert_eq!(ddl_after_first, ddl_after_second);
        assert!(ddl_after_second.contains("dir_id, book_id, file_name"));
    }

    #[test]
    fn client_projection_revives_a_multi_node_cycle_and_its_descendant() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        for change in [dir_added_change("a", "__root__", "A", 10), dir_added_change("b", "a", "B", 11), dir_added_change("c", "b", "C", 12), dir_added_change("d", "c", "D", 13)] {
            apply_remote_change_in_tx(&tx, &change).unwrap();
        }
        apply_remote_change_in_tx(&tx, &dir_added_change("a", "c", "A", 20)).unwrap();
        assert!(get_live_dir_paths_tx(&tx).unwrap().is_empty(), "the cycle and every unreachable descendant must be suppressed");

        apply_remote_change_in_tx(&tx, &dir_added_change("a", "__root__", "A", 30)).unwrap();
        let paths = get_live_dir_paths_tx(&tx).unwrap();
        assert_eq!(paths.get(&test_dir_id("a")).unwrap().as_str(), "A");
        assert_eq!(paths.get(&test_dir_id("b")).unwrap().as_str(), "A/B");
        assert_eq!(paths.get(&test_dir_id("c")).unwrap().as_str(), "A/B/C");
        assert_eq!(paths.get(&test_dir_id("d")).unwrap().as_str(), "A/B/C/D");
        tx.commit().unwrap();
    }

    #[test]
    fn client_projection_revives_a_suppressed_directory_subtree_after_collision_clears() {
        let mut conn = fk_conn();
        let tx = conn.transaction().unwrap();
        apply_remote_change_in_tx(&tx, &dir_added_change("winner", "__root__", "Shared", 20)).unwrap();
        apply_remote_change_in_tx(&tx, &dir_added_change("loser", "__root__", "Shared", 10)).unwrap();
        apply_remote_change_in_tx(&tx, &dir_added_change("child", "loser", "Child", 11)).unwrap();
        let suppressed = get_live_dir_paths_tx(&tx).unwrap();
        assert_eq!(suppressed.len(), 1);
        assert_eq!(suppressed.get(&test_dir_id("winner")).unwrap().as_str(), "Shared");

        apply_remote_change_in_tx(&tx, &dir_added_change("winner", "__root__", "Other", 30)).unwrap();
        let revived = get_live_dir_paths_tx(&tx).unwrap();
        assert_eq!(revived.get(&test_dir_id("winner")).unwrap().as_str(), "Other");
        assert_eq!(revived.get(&test_dir_id("loser")).unwrap().as_str(), "Shared");
        assert_eq!(revived.get(&test_dir_id("child")).unwrap().as_str(), "Shared/Child");
        tx.commit().unwrap();
    }

    #[test]
    fn client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row() {
        let mut conn = lww_conn();
        conn.execute(
            "INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, intent_deleted, deleted_at)
             VALUES ('d1', '1', 'f.epub', 'old', ?1, 10, 0, 'stored-actor', 1, '00000000-0000-0000-0000-000000000000', 0, NULL)",
            [TEST_CONTENT_HASH],
        )
        .unwrap();
        let tx = conn.transaction().unwrap();
        let replacement = ContentHash::parse(&"b".repeat(64)).unwrap();
        let result = apply_remote_book_dir(&tx, &book_dir_change(true, 20, 0), BookId::new(1), &test_dir_id("d1"), &test_file_name("f.epub"), RemotePlacementState::Present(&replacement));
        assert!(result.is_err(), "a nil stored event id must fail closed");
        let stored: String = tx.query_row("SELECT content_hash FROM book_dir WHERE dir_id='d1' AND book_id='1' AND file_name='f.epub'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, TEST_CONTENT_HASH);
        tx.rollback().unwrap();
    }

    #[test]
    fn legacy_version_repair_is_deterministic_across_client_databases() {
        let repaired_event = || {
            let conn = old_schema_conn();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event)
                 VALUES ('d1', 'b1', 'f.epub', 'legacy-fingerprint', ?1, 42, 3, '', 7, '')",
                [TEST_CONTENT_HASH],
            )
            .unwrap();
            migrate_book_dir_primary_key(&conn).unwrap();
            repair_legacy_versions(&conn, "book_dir", "dir_id || ':' || book_id || ':' || file_name").unwrap();
            conn.query_row("SELECT hlc_actor, hlc_event FROM book_dir", [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).unwrap()
        };
        let first = repaired_event();
        let second = repaired_event();
        assert_eq!(first, second);
        assert_eq!(first.0, "legacy");
        EventId::parse(&first.1).expect("legacy repair must produce a valid non-nil event id");
    }
}

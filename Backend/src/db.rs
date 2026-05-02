use crate::library::{Book, Dir};
use crate::BookID;
use include_sqlite_sql::{impl_sql, include_sql};
use rusqlite::{Connection, Transaction};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::{Change, ChangeOp, Hlc};
use uuid::Uuid;

include_sql!("src/database/create_library.sql");
include_sql!("src/database/library.sql");
include_sql!("src/database/views.sql");

struct RemoteChangeGuard<'a> {
    conn: &'a Connection,
    active: bool,
}

impl<'a> RemoteChangeGuard<'a> {
    fn new(conn: &'a Connection) -> Self {
        let active = match conn.set_sync_metadata("change_origin", "remote") {
            Ok(_) => true,
            Err(err) => {
                log::warn!("Failed to mark sync context as remote – triggers will still run: {}", err);
                false
            }
        };
        Self { conn, active }
    }
}

impl Drop for RemoteChangeGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.conn.set_sync_metadata("change_origin", "local");
        }
    }
}

fn reset_development_schema(conn: &Connection) {
    let drop_stmt = r#"
    PRAGMA foreign_keys = OFF;

    DROP TRIGGER IF EXISTS trig_dir_added_insert;
    DROP TRIGGER IF EXISTS trig_dir_added_reactivate;
    DROP TRIGGER IF EXISTS trig_dir_moved;
    DROP TRIGGER IF EXISTS trig_dir_deleted;
    DROP TRIGGER IF EXISTS trig_book_added;
    DROP TRIGGER IF EXISTS trig_book_added_reactivate;
    DROP TRIGGER IF EXISTS trig_book_deleted;
    DROP TRIGGER IF EXISTS trig_book_dir_added_insert;
    DROP TRIGGER IF EXISTS trig_book_dir_added_reactivate;
    DROP TRIGGER IF EXISTS trig_book_read_pos;
    DROP TRIGGER IF EXISTS trig_book_last_read;
    DROP TRIGGER IF EXISTS trig_event_log_hlc;

    DROP TABLE IF EXISTS book_dir;
    DROP TABLE IF EXISTS book;
    DROP TABLE IF EXISTS dir;
    DROP TABLE IF EXISTS event_log;
    DROP TABLE IF EXISTS sync_metadata;

    DROP VIEW IF EXISTS dir_paths;

    PRAGMA foreign_keys = ON;
    "#;

    if let Err(err) = conn.execute_batch(drop_stmt) {
        log::warn!("Failed to reset development schema: {}", err);
    }
}

pub fn init_db(conn: &Connection) -> String {
    reset_development_schema(conn);

    conn.create_author_table().unwrap();
    conn.create_book_table().unwrap();
    conn.create_dir_table().unwrap();
    conn.insert_root().unwrap();
    conn.create_book_dir_table().unwrap();
    conn.create_event_log_table().unwrap();
    conn.create_transfer_jobs_table().unwrap();
    conn.create_blob_state_table().unwrap();
    conn.create_blob_local_ref_table().unwrap();
    conn.create_views().unwrap();
    conn.execute_batch(include_str!("database/triggers.sql")).unwrap();
    let _ = conn.execute("ALTER TABLE event_log ADD COLUMN event_kind INTEGER NOT NULL DEFAULT 0", ());
    conn.create_sync_metadata_table().unwrap();
    let _ = conn.set_sync_metadata("change_origin", "local");
    if get_sync_metadata(conn, "first_sync_done").is_none() {
        let _ = conn.set_sync_metadata("first_sync_done", "0");
    }
    if get_sync_metadata(conn, "hlc_pt").is_none() {
        let _ = conn.set_sync_metadata("hlc_pt", "0");
    }
    if get_sync_metadata(conn, "hlc_l").is_none() {
        let _ = conn.set_sync_metadata("hlc_l", "0");
    }

    // Enable WAL mode AFTER creating all tables
    // WAL mode is persistent across connections once set
    log::info!("Setting WAL mode and busy timeout");
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "busy_timeout", "10000").unwrap();

    // Verify it was applied
    let journal_mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0)).unwrap();
    let busy_timeout: i32 = conn.pragma_query_value(None, "busy_timeout", |row| row.get(0)).unwrap();
    log::info!("Database configured: journal_mode={}, busy_timeout={}", journal_mode, busy_timeout);

    get_or_create_client_id(conn)
}

pub fn get_or_create_client_id(db: &Connection) -> String {
    let mut client_id: Option<String> = None;
    let _ = db.get_sync_metadata_value("client_id", |row| {
        client_id = Some(row.get(0)?);
        Ok(())
    });

    match client_id {
        Some(id) => id,
        None => {
            let new_id = Uuid::new_v4().to_string();
            let _ = db.set_sync_metadata("client_id", &new_id);
            new_id
        }
    }
}

pub fn get_dir_contents(conn: &Connection, dir_path: &str) -> (Vec<Dir>, Vec<Book>) {
    let mut books: Vec<Book> = Vec::new();
    let mut sub_dirs: Vec<Dir> = Vec::new();

    conn.get_sub_dirs(dir_path, |row| {
        let id = row.get(0)?;
        let name = row.get(1)?;
        sub_dirs.push(Dir { id, name });
        Ok(())
    })
    .unwrap();

    conn.get_books_in_dir(dir_path, |row| {
        let book_id: String = row.get(0)?;
        let title: Option<String> = row.get(1).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(2).ok();
        books.push(Book::new(book_id.parse().unwrap(), title, author.unwrap_or_default()));
        Ok(())
    })
    .unwrap();
    (sub_dirs, books)
}

pub fn get_recently_read(conn: &Connection, limit: i32) -> Vec<Book> {
    let mut books: Vec<Book> = Vec::new();

    conn.get_recently_read(limit, |row| {
        let book_id: String = row.get(0)?;
        let title: Option<String> = row.get(1).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(2).ok();
        let read_progress = row.get::<_, Option<f32>>(3)?.unwrap_or(0.0);
        let last_read = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
        let mut book = Book::new(book_id.parse().unwrap(), title, author.unwrap_or_default());
        book.read_progress = read_progress;
        book.last_read = last_read;
        books.push(book);
        Ok(())
    })
    .unwrap();
    books
}

pub fn get_recently_added(conn: &Connection, limit: i32) -> Vec<Book> {
    let mut books: Vec<Book> = Vec::new();
    conn.get_recently_added(limit, |row| {
        let book_id: String = row.get(0)?;
        let title: Option<String> = row.get(1).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(2).ok();
        let read_progress = row.get::<_, Option<f32>>(3)?.unwrap_or(0.0);
        let added_at = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
        let mut book = Book::new(book_id.parse().unwrap(), title, author.unwrap_or_default());
        book.read_progress = read_progress;
        book.added_at = added_at;
        books.push(book);
        Ok(())
    })
    .unwrap();
    books
}

pub fn get_authors(conn: &Connection) -> Vec<(String, usize)> {
    let mut authors = Vec::new();
    let _ = conn.get_authors(|row| {
        let name: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        authors.push((name, count.max(0) as usize));
        Ok(())
    });
    authors
}

pub fn get_author_books_grouped(conn: &Connection, author_name: &str) -> Vec<(String, String, Vec<Book>)> {
    let mut grouped: HashMap<String, (String, Vec<Book>)> = HashMap::new();
    let _ = conn.get_author_books(author_name, |row| {
        let dir_id: String = row.get(0)?;
        let folder_path: String = row.get(1)?;
        let book_id: String = row.get(2)?;
        let title: Option<String> = row.get(3).ok();
        let title = title.unwrap_or_else(|| format!("Untitled ({})", book_id));
        let author: Option<String> = row.get(4).ok();
        let book = Book::new(book_id.parse().unwrap(), title, author.unwrap_or_default());
        grouped.entry(dir_id).or_insert_with(|| (folder_path, Vec::new())).1.push(book);
        Ok(())
    });

    grouped.into_iter().map(|(dir_id, (folder_path, books))| (dir_id, folder_path, books)).collect()
}

pub fn insert_book_with_title(conn: &Connection, book_id: &str, book_title: &str) {
    let added_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;

    match conn.insert_book(book_id, book_title, added_at) {
        Ok(_) => {}
        Err(_) => {
            let _ = conn.update_book_title(book_id, book_title);
        }
    }
}

pub fn get_book_file_path(conn: &Connection, book_id: BookID) -> Option<String> {
    let mut file_path: Option<String> = None;

    conn.get_book_file_path(&book_id.to_string(), |row| {
        file_path = Some(row.get(0)?);
        Ok(())
    })
    .unwrap();
    file_path
}

pub fn get_book_file_path_as_pathbuf(conn: &Connection, book_id: BookID) -> Option<PathBuf> {
    let mut components: Vec<String> = Vec::new();

    let result = conn.get_book_file_path_components(&book_id.to_string(), |row| {
        let component: String = row.get(0)?;
        components.push(component);
        Ok(())
    });

    if result.is_err() {
        return None;
    }

    if components.is_empty() {
        return None;
    }

    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }

    Some(path)
}

pub fn get_book_reading_position(conn: &Connection, book_id: &str) -> Option<String> {
    let mut position: Option<String> = None;

    conn.get_book_reading_position(book_id, |row| {
        let pos: String = row.get(0)?;
        if pos != "0" && !pos.is_empty() {
            position = Some(pos);
        }
        Ok(())
    })
    .unwrap();
    position
}

pub fn is_book_downloaded(conn: &Connection, book_id: &str) -> bool {
    let mut downloaded = false;
    let _ = conn.is_book_downloaded(book_id, |row| {
        downloaded = row.get::<_, bool>(0)?;
        Ok(())
    });
    downloaded
}

#[derive(Debug, Clone)]
pub struct BookAsset {
    pub book_id: String,
    pub local_hash: String,
    pub file_path: String,
    pub is_downloaded: bool,
}

pub fn get_book_assets(conn: &Connection) -> Vec<BookAsset> {
    let mut assets = Vec::new();
    let _ = conn.get_book_assets(|row| {
        assets.push(BookAsset {
            book_id: row.get(0)?,
            local_hash: row.get(1)?,
            file_path: row.get(2)?,
            is_downloaded: row.get::<_, i64>(3)? != 0,
        });
        Ok(())
    });
    assets
}

#[derive(Debug, Clone)]
pub struct TransferJobRow {
    pub id: i64,
    pub kind: String,
    pub key1: String,
    pub rel_path: String,
    pub local_path: String,
    pub book_id: String,
    pub attempts: i64,
}

fn now_ts_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

pub fn upsert_transfer_job(conn: &Connection, kind: &str, key1: &str, rel_path: &str, local_path: &str, book_id: &str) {
    let now = now_ts_secs();
    let _ = conn.upsert_transfer_job(kind, key1, rel_path, local_path, book_id, now);
}

pub fn claim_due_transfer_job(conn: &Connection) -> Option<TransferJobRow> {
    let now = now_ts_secs();
    let mut job: Option<TransferJobRow> = None;
    let _ = conn.claim_due_transfer_job(now, |row| {
        job = Some(TransferJobRow {
            id: row.get(0)?,
            kind: row.get(1)?,
            key1: row.get(2)?,
            rel_path: row.get(3)?,
            local_path: row.get(4)?,
            book_id: row.get(5)?,
            attempts: row.get(6)?,
        });
        Ok(())
    });
    job
}

pub fn complete_transfer_job(conn: &Connection, id: i64) {
    let _ = conn.complete_transfer_job(id);
}

pub fn fail_transfer_job(conn: &Connection, id: i64, attempts: i64, error: &str) {
    let now = now_ts_secs();
    let backoff = (2_i64.pow((attempts as u32).min(8)) * 2).min(600);
    let jitter = (now % 3).abs();
    let next_retry_at = now + backoff + jitter;
    let _ = conn.fail_transfer_job(id, error, next_retry_at, now);
}

pub fn reset_stale_claims(conn: &Connection, stale_after_secs: i64) {
    let stale_before = now_ts_secs() - stale_after_secs.max(1);
    let _ = conn.reset_stale_claims(stale_before);
}

pub fn delete_unclaimed_transfer_jobs(conn: &Connection) {
    let _ = conn.delete_unclaimed_transfer_jobs();
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferJobKey {
    pub kind: String,
    pub key1: String,
    pub rel_path: String,
    pub local_path: String,
    pub book_id: String,
}

pub fn prune_unclaimed_transfer_jobs(conn: &Connection, desired: &[TransferJobKey]) {
    let desired_set: HashSet<TransferJobKey> = desired.iter().cloned().collect();
    let mut rows: Vec<(i64, TransferJobKey)> = Vec::new();
    let _ = conn.get_unclaimed_transfer_jobs(|row| {
        rows.push((
            row.get(0)?,
            TransferJobKey {
                kind: row.get(1)?,
                key1: row.get(2)?,
                rel_path: row.get(3)?,
                local_path: row.get(4)?,
                book_id: row.get(5)?,
            },
        ));
        Ok(())
    });

    for (id, key) in rows {
        if !desired_set.contains(&key) {
            let _ = conn.delete_transfer_job_by_id(id);
        }
    }
}

pub fn upsert_blob_local_ref(conn: &Connection, kind: &str, hash: &str, path: &str, exists: bool) {
    let now = now_ts_secs();
    let exists_flag = if exists { 1 } else { 0 };
    let _ = conn.upsert_blob_local_ref(kind, hash, path, exists_flag, now);
}

pub fn upsert_blob_local_ref_tx(tx: &Transaction, kind: &str, hash: &str, path: &str, exists: bool) {
    let now = now_ts_secs();
    let exists_flag = if exists { 1 } else { 0 };
    let _ = tx.upsert_blob_local_ref(kind, hash, path, exists_flag, now);
}

pub fn derive_blob_state_from_local(conn: &Connection, kind: &str, hash: &str) {
    let mut exists = 0_i32;
    let _ = conn.get_blob_local_presence(kind, hash, |row| {
        exists = row.get::<_, i32>(0)?;
        Ok(())
    });
    let now = now_ts_secs();
    let state = if exists > 0 { 1 } else { 0 };
    let _ = conn.upsert_blob_state(kind, hash, state, now);
}

pub fn derive_blob_state_from_local_tx(tx: &Transaction, kind: &str, hash: &str) {
    let mut exists = 0_i32;
    let _ = tx.get_blob_local_presence(kind, hash, |row| {
        exists = row.get::<_, i32>(0)?;
        Ok(())
    });
    let now = now_ts_secs();
    let state = if exists > 0 { 1 } else { 0 };
    let _ = tx.upsert_blob_state(kind, hash, state, now);
}

pub fn get_blob_hashes_by_state(conn: &Connection, kind: &str, state: i32) -> Vec<String> {
    let mut hashes = Vec::new();
    let _ = conn.get_blob_hashes_by_state(kind, state, |row| {
        hashes.push(row.get(0)?);
        Ok(())
    });
    hashes
}

pub fn get_blob_local_path(conn: &Connection, kind: &str, hash: &str) -> Option<String> {
    let mut path: Option<String> = None;
    let _ = conn.get_blob_local_path(kind, hash, |row| {
        path = Some(row.get(0)?);
        Ok(())
    });
    path
}

pub fn get_blob_any_path(conn: &Connection, kind: &str, hash: &str) -> Option<String> {
    let mut path: Option<String> = None;
    let _ = conn.get_blob_any_path(kind, hash, |row| {
        path = Some(row.get(0)?);
        Ok(())
    });
    path
}

pub fn get_book_paths_by_local_hash(conn: &Connection, local_hash: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let _ = conn.get_book_paths_by_local_hash(local_hash, |row| {
        paths.push(row.get(0)?);
        Ok(())
    });
    paths
}

pub fn is_blob_local(conn: &Connection, kind: &str, hash: &str) -> bool {
    let mut exists = 0_i32;
    let _ = conn.get_blob_local_presence(kind, hash, |row| {
        exists = row.get::<_, i32>(0)?;
        Ok(())
    });
    exists > 0
}

pub fn set_blob_state_checked(conn: &Connection, kind: &str, hash: &str, state: i32) {
    let now = now_ts_secs();
    let _ = conn.set_blob_state_checked(kind, hash, state, now);
}

pub fn set_blob_state_error(conn: &Connection, kind: &str, hash: &str, error: &str) {
    let now = now_ts_secs();
    let _ = conn.set_blob_state_error(kind, hash, error, now);
}

pub fn get_pending_download_hashes(conn: &Connection) -> Vec<String> {
    let raw = get_sync_metadata(conn, "pending_download_hashes").unwrap_or_else(|| "[]".to_string());
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

pub fn set_pending_download_hashes(conn: &Connection, hashes: &[String]) {
    let json = serde_json::to_string(hashes).unwrap_or_else(|_| "[]".to_string());
    set_sync_metadata(conn, "pending_download_hashes", &json);
}

pub fn add_pending_download_hash(conn: &Connection, hash: &str) {
    let mut hashes = get_pending_download_hashes(conn);
    if !hashes.iter().any(|h| h == hash) {
        hashes.push(hash.to_string());
        set_pending_download_hashes(conn, &hashes);
    }
}

pub fn update_reading_position(conn: &Connection, book_id: BookID, cfi: &str) {
    let book_id = book_id.to_string();
    conn.update_reading_position(&book_id, cfi).unwrap();
}

pub fn update_last_read(conn: &Connection, book_id: &str) {
    let last_read_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;
    conn.update_last_read(book_id, last_read_time).unwrap();
}
pub fn get_unpushed_events(conn: &Connection) -> Vec<Change> {
    let last_local_seq_sent = get_sync_metadata(conn, "last_pushed_local_seq")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut events = Vec::new();
    let _ = conn.get_unpushed_events(last_local_seq_sent, |row| {
        let entity_id: String = row.get(0)?;
        let entity_type: String = row.get(1)?;
        let payload_raw: String = row.get(2)?;
        let changed_at: i64 = row.get(3)?;
        let local_seq: i64 = row.get(4)?;
        let hlc_pt: i64 = row.get(5)?;
        let hlc_l: i64 = row.get(6)?;
        let payload = serde_json::from_str(&payload_raw).unwrap_or_else(|_| serde_json::json!({}));
        let op = if entity_type.ends_with("_deleted") {
            ChangeOp::Delete
        } else {
            ChangeOp::Upsert
        };

        events.push(Change {
            entity_type,
            entity_id,
            op,
            payload,
            changed_at: changed_at.max(0) as u64,
            hlc: Hlc {
                pt: hlc_pt.max(0) as u64,
                l: hlc_l.max(0) as u64,
            },
            local_seq: Some(local_seq.max(0) as u64),
            server_seq: None,
        });
        Ok(())
    });
    events
}
pub fn mark_events_pushed(conn: &Connection, up_to_local_seq: u64, _server_seq: u64) {
    set_sync_metadata(conn, "last_pushed_local_seq", &up_to_local_seq.to_string());
}

pub fn get_sync_metadata(conn: &Connection, key: &str) -> Option<String> {
    let mut value: Option<String> = None;

    let _ = conn.get_sync_metadata_value(key, |row| {
        value = Some(row.get(0)?);
        Ok(())
    });
    value
}

pub fn set_sync_metadata(conn: &Connection, key: &str, value: &str) {
    let _ = conn.set_sync_metadata(key, value);
}

pub fn delete_dir(conn: &Connection, dir_id: &str) {
    let _ = conn.mark_dir_deleted(dir_id);
}

pub fn apply_remote_change_in_tx(tx: &Transaction, change: &Change) -> Result<(), Box<dyn std::error::Error>> {
    observe_remote_hlc(tx, change.hlc)?;
    struct TxOriginGuard<'a> {
        tx: &'a Transaction<'a>,
        active: bool,
    }
    impl<'a> TxOriginGuard<'a> {
        fn activate(tx: &'a Transaction<'a>) -> Result<Self, rusqlite::Error> {
            tx.set_sync_metadata("change_origin", "remote")?;
            Ok(Self { tx, active: true })
        }
    }
    impl Drop for TxOriginGuard<'_> {
        fn drop(&mut self) {
            if self.active {
                let _ = self.tx.set_sync_metadata("change_origin", "local");
            }
        }
    }

    let _guard = TxOriginGuard::activate(tx)?;
    match change.entity_type.as_str() {
        "dir_added" => {
            if let Some(path) = change.payload.get("path").and_then(|v| v.as_str()) {
                let _ = ensure_dir_for_path(tx, path)?;
            }
        }
        "dir_moved" => {
            let old_path = change.payload.get("old_path").and_then(|v| v.as_str());
            let new_path = change.payload.get("new_path").and_then(|v| v.as_str());
            match (old_path, new_path) {
                (Some(old_path), Some(new_path)) => {
                    let Some(dir_id) = find_dir_id_by_path(tx, old_path)? else {
                        stash_pending_remote_change(tx, change)?;
                        return Ok(());
                    };
                    let (new_parent, new_name) = split_dir_path(new_path);
                    let Some(new_name) = new_name else {
                        stash_pending_remote_change(tx, change)?;
                        return Ok(());
                    };
                    let Some(parent_id) = find_dir_id_by_path(tx, &new_parent)? else {
                        stash_pending_remote_change(tx, change)?;
                        return Ok(());
                    };
                    let _ = tx.upsert_dir(&dir_id, &parent_id, &new_name);
                }
                _ => {
                    stash_pending_remote_change(tx, change)?;
                    return Ok(());
                }
            }
        }
        "dir_deleted" => {
            if let Some(path) = change.payload.get("path").and_then(|v| v.as_str()) {
                if let Some(dir_id) = find_dir_id_by_path(tx, path)? {
                    let _ = tx.mark_dir_deleted(&dir_id);
                }
            }
        }
        "book_added" => {
            let title = change
                .payload
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&change.entity_id);
            let added_at = change
                .payload
                .get("added_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(change.changed_at as i64);
            let _ = tx.insert_book(&change.entity_id, title, added_at);
        }
        "book_deleted" => {
            let book_id = change
                .payload
                .get("book_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&change.entity_id);
            let _ = tx.mark_book_deleted(book_id, change.changed_at as i64);
        }
        "book_dir_added" | "book_dir_removed" | "book_dir_deleted" => {
            let file_path = change
                .payload
                .get("file_path")
                .and_then(|v| v.as_str())
                .or_else(|| change.payload.get("path").and_then(|v| v.as_str()));
            let dir_path = change.payload.get("dir_path").and_then(|v| v.as_str());
            let book_id = change
                .payload
                .get("book_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&change.entity_id);

            let (resolved_dir_path, file_name) = match (dir_path, file_path) {
                (_, Some(fp)) => split_file_path(fp).unwrap_or_else(|| ("/".to_string(), fp.to_string())),
                (Some(dp), None) => {
                    let file_name = change
                        .payload
                        .get("file_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    (dp.to_string(), file_name)
                }
                (None, None) => ("/".to_string(), String::new()),
            };

            if !file_name.is_empty() {
                let Some(dir_id) = find_dir_id_by_path(tx, &resolved_dir_path)? else {
                    stash_pending_remote_change(tx, change)?;
                    return Ok(());
                };
                let local_hash = remote_book_dir_hash(book_id, &dir_id, &file_name);
                let _ = tx.first_book_insert(book_id, change.changed_at as i64);
                if change.op == ChangeOp::Delete || change.entity_type != "book_dir_added" {
                    let _ = tx.mark_book_dir_deleted(&dir_id, book_id, &file_name);
                } else {
                    let _ = tx.insert_book_dir(&dir_id, book_id, &file_name, &local_hash, 0);
                }
            }
        }
        "reading_position_updated" => {
            if let Some(cfi) = change.payload.get("cfi").and_then(|v| v.as_str()) {
                tx.update_reading_position(&change.entity_id, cfi)?;
            }
        }
        "book_opened" => {
            if let Some(last_read) = change.payload.get("last_read").and_then(|v| v.as_i64()) {
                tx.update_last_read(&change.entity_id, last_read)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn read_local_hlc(tx: &Transaction) -> Result<Hlc, rusqlite::Error> {
    let mut pt = 0_u64;
    let mut l = 0_u64;
    let _ = tx.get_sync_metadata_value("hlc_pt", |row| {
        let value: String = row.get(0)?;
        pt = value.parse::<u64>().unwrap_or(0);
        Ok(())
    });
    let _ = tx.get_sync_metadata_value("hlc_l", |row| {
        let value: String = row.get(0)?;
        l = value.parse::<u64>().unwrap_or(0);
        Ok(())
    });
    Ok(Hlc { pt, l })
}

fn write_local_hlc(tx: &Transaction, hlc: Hlc) -> Result<(), rusqlite::Error> {
    tx.set_sync_metadata("hlc_pt", &hlc.pt.to_string())?;
    tx.set_sync_metadata("hlc_l", &hlc.l.to_string())?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn observe_remote_hlc(tx: &Transaction, remote_hlc: Hlc) -> Result<(), Box<dyn std::error::Error>> {
    let prev = read_local_hlc(tx)?;
    let next = Hlc::tick_recv(prev, remote_hlc, now_ms());
    write_local_hlc(tx, next)?;
    Ok(())
}

pub fn drain_pending_remote_changes_in_tx(tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
    let pending = load_pending_remote_changes(tx)?;
    if pending.is_empty() {
        return Ok(());
    }

    tx.set_sync_metadata("pending_remote_changes", "[]")?;
    for change in pending {
        apply_remote_change_in_tx(tx, &change)?;
    }
    Ok(())
}

fn normalize_dir_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').trim_end_matches('/').to_string()
}

fn find_dir_id_by_path(tx: &Transaction, path: &str) -> Result<Option<String>, rusqlite::Error> {
    let normalized = normalize_dir_path(path);
    let mut stmt = tx.prepare("SELECT id FROM dir_paths WHERE path = ?1 LIMIT 1")?;
    let mut rows = stmt.query([normalized])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

fn ensure_dir_for_path(tx: &Transaction, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let normalized = normalize_dir_path(path);
    if normalized.is_empty() {
        return Ok("__root__".to_string());
    }

    let mut current_id = "__root__".to_string();
    for part in normalized.split('/').filter(|p| !p.is_empty()) {
        let existing_id: Option<String> = tx
            .query_row(
                "SELECT id FROM dir WHERE parent_id = ?1 AND name = ?2 LIMIT 1",
                rusqlite::params![current_id, part],
                |row| row.get(0),
            )
            .ok();
        let next_id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let _ = tx.upsert_dir(&next_id, &current_id, part);
        current_id = next_id;
    }
    Ok(current_id)
}

fn split_file_path(path: &str) -> Option<(String, String)> {
    let p = Path::new(path);
    let file_name = p.file_name()?.to_string_lossy().to_string();
    let parent = p.parent().map(|pp| pp.to_string_lossy().to_string()).unwrap_or_else(|| "/".to_string());
    let dir_path = if parent.is_empty() || parent == "." { "/".to_string() } else { parent };
    Some((dir_path, file_name))
}

fn split_dir_path(path: &str) -> (String, Option<String>) {
    let normalized = normalize_dir_path(path);
    if normalized.is_empty() {
        return ("/".to_string(), None);
    }
    if let Some((parent, name)) = normalized.rsplit_once('/') {
        let p = if parent.is_empty() { "/".to_string() } else { format!("/{parent}") };
        (p, Some(name.to_string()))
    } else {
        ("/".to_string(), Some(normalized))
    }
}

fn load_pending_remote_changes(tx: &Transaction) -> Result<Vec<Change>, Box<dyn std::error::Error>> {
    let mut raw: Option<String> = None;
    let _ = tx.get_sync_metadata_value("pending_remote_changes", |row| {
        raw = Some(row.get(0)?);
        Ok(())
    });
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let parsed: Vec<Change> = serde_json::from_str(&raw).unwrap_or_default();
    Ok(parsed)
}

fn stash_pending_remote_change(tx: &Transaction, change: &Change) -> Result<(), Box<dyn std::error::Error>> {
    let mut pending = load_pending_remote_changes(tx)?;
    let already = pending.iter().any(|c| {
        c.entity_type == change.entity_type
            && c.entity_id == change.entity_id
            && c.payload == change.payload
            && c.changed_at == change.changed_at
    });
    if !already {
        pending.push(change.clone());
    }
    let serialized = serde_json::to_string(&pending)?;
    tx.set_sync_metadata("pending_remote_changes", &serialized)?;
    Ok(())
}
fn remote_book_dir_hash(book_id: &str, dir_id: &str, file_name: &str) -> String {
    format!("remote:{dir_id}:{book_id}:{file_name}")
}

pub fn set_book_downloaded(conn: &Connection, file_path: &str, book_id: &str, downloaded: bool) {
    let mut entry: Option<(String, String, String)> = None;
    let _ = conn.get_book_dir_entry_by_path(file_path, |row| {
        entry = Some((row.get(0)?, row.get(1)?, row.get(2)?));
        Ok(())
    });

    let Some((found_book_id, dir_id, file_name)) = entry else {
        return;
    };
    if found_book_id != book_id {
        return;
    }
    let value = if downloaded { 1 } else { 0 };
    let _ = conn.set_book_downloaded(&dir_id, book_id, &file_name, value);
}

pub fn book_has_remaining_paths(conn: &Connection, book_id: &str) -> bool {
    let mut count = 0;

    let _ = conn.count_book_paths(book_id, |row| {
        count = row.get(0)?;
        Ok(())
    });

    count > 0
}

pub fn delete_book_metadata(conn: &Connection, book_id: &str) {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;
    let _ = conn.mark_book_deleted(book_id, ts);
}

pub fn upsert_dir(transaction: &Transaction, dir_uuid: &str, parent_uuid: &str, name: &str) {
    match transaction.upsert_dir(dir_uuid, parent_uuid, name) {
        Ok(_) => {}
        Err(e) => {
            let error_msg = format!("{}", e);
            if error_msg.contains("locked") {
                log::warn!("Failed to upsert directory '{}': {} (will retry on next scan)", name, e);
            } else {
                log::error!("Failed to upsert directory '{}': {}", name, e);
            }
        }
    }
}

pub struct BookDir {
    pub book_id: String,
    pub parent_id: String,
    pub file_name: String,
}

pub fn insert_book(transaction: &Transaction, book_id: u64) {
    let added_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;

    log::debug!("Attempting to insert book: {}", book_id);
    match transaction.first_book_insert(&book_id.to_string(), added_at) {
        Ok(_) => log::debug!("Successfully inserted book: {}", book_id),
        Err(error) => {
            let error_msg = format!("{}", error);
            log::error!("Failed to insert book {}: {}", book_id, error_msg);

            // Log detailed error information
            if error_msg.contains("locked") {
                log::error!("DATABASE LOCKED ERROR detected for book {}", book_id);
                log::error!("This suggests another transaction is holding a write lock");
                log::error!("Error details: {:?}", error);
            } else if error_msg.contains("UNIQUE") {
                log::warn!("Book {} already exists (duplicate insert)", book_id);
            } else {
                log::error!("Unexpected database error: {:?}", error);
            }
        }
    }
}

pub fn insert_local_book_dir(transaction: &Transaction, dir_uuid: &str, book_id: String, file_name: &str, local_hash: u64) {
    match transaction.insert_book_dir(dir_uuid, &book_id, file_name, &local_hash.to_string(), 1) {
        Ok(_) => {}
        Err(e) => {
            let error_msg = format!("{}", e);
            if error_msg.contains("locked") || error_msg.contains("UNIQUE") {
                log::warn!("Failed to insert book_dir for {}: {} (may already exist)", book_id, e);
            } else {
                log::error!("Failed to insert book_dir for {}: {}", book_id, e);
            }
        }
    }
}

pub fn get_book_dir_by_hash(transaction: &Transaction, hash: u64) -> Option<BookDir> {
    let mut result: Option<BookDir> = None;
    let hash = hash.to_string();
    let query_result = transaction.get_book_dir_by_hash(&hash, |row| {
        result = Some(BookDir { book_id: row.get(0)?, parent_id: row.get(1)?, file_name: row.get(2)? });
        Ok(())
    });
    if let Err(err) = query_result {
        eprintln!("Error: {}", err);
    }
    result
}

pub fn get_books_without_title(transaction: &Transaction) -> Vec<(u64, PathBuf)> {
    let mut relative_book_paths = Vec::new();
    let query_result = transaction.get_book_paths_without_title(|row| {
        let book_id: String = row.get(0).unwrap();
        let path_str: String = row.get(1).unwrap();
        let book_path = PathBuf::from(path_str);
        relative_book_paths.push((book_id.parse().unwrap(), book_path));
        Ok(())
    });
    eprintln!("Getting books without title");
    if let Err(err) = query_result {
        eprintln!("Error: {}", err);
    }
    relative_book_paths
}

fn get_or_create_author_id(transaction: &Transaction, author_name: &str) -> Option<i64> {
    let normalized = author_name.trim();
    if normalized.is_empty() {
        return None;
    }
    let _ = transaction.execute("INSERT OR IGNORE INTO author(name) VALUES (?1)", [normalized]);
    transaction
        .query_row("SELECT id FROM author WHERE name = ?1 LIMIT 1", [normalized], |row| row.get(0))
        .ok()
}

pub fn update_book_metadata(transaction: &Transaction, book_id: u64, title: &str, author: &str) {
    let author_id = get_or_create_author_id(transaction, author);
    let query_result = transaction.execute(
        "UPDATE book SET title = ?1, author_id = ?2 WHERE id = ?3",
        rusqlite::params![title, author_id, book_id.to_string()],
    );
    match query_result {
        Ok(_) => log::debug!("Updated metadata for book {}: '{}' / '{}'", book_id, title, author),
        Err(err) => {
            let error_msg = format!("{}", err);
            if error_msg.contains("locked") {
                log::error!("DATABASE LOCKED when updating metadata for book {}: {}", book_id, err);
            } else {
                log::error!("Failed to update metadata for book {}: {}", book_id, err);
            }
        }
    }
}

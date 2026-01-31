use crate::library::{Book, Dir};
use include_sqlite_sql::{impl_sql, include_sql};
use rusqlite::{Connection, Transaction};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::{Event, EventPayload};
use uuid::Uuid;
use crate::BookID;

include_sql!("src/database/create_library.sql");
include_sql!("src/database/library.sql");
include_sql!("src/database/triggers.sql");
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
                log::warn!(
                    "Failed to mark sync context as remote – triggers will still run: {}",
                    err
                );
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
    DROP TRIGGER IF EXISTS trig_dir_deleted;
    DROP TRIGGER IF EXISTS trig_book_added;
    DROP TRIGGER IF EXISTS trig_book_deleted;
    DROP TRIGGER IF EXISTS trig_book_dir_added_insert;
    DROP TRIGGER IF EXISTS trig_book_dir_added_reactivate;
    DROP TRIGGER IF EXISTS trig_book_read_pos;
    DROP TRIGGER IF EXISTS trig_book_last_read;

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
    conn.create_book_table().unwrap();
    conn.create_dir_table().unwrap();
    conn.insert_root().unwrap();
    conn.create_book_dir_table().unwrap();
    conn.create_event_log_table().unwrap();
    conn.create_views().unwrap();
    let _ = conn.execute(
        "ALTER TABLE event_log ADD COLUMN event_kind INTEGER NOT NULL DEFAULT 0",
        (),
    );
    conn.create_sync_metadata_table().unwrap();
    let _ = conn.set_sync_metadata("change_origin", "local");
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

pub fn get_library_state(conn: &Connection) -> (HashSet<String>, HashSet<PathBuf>, HashSet<PathBuf>) {
    let mut book_ids = HashSet::new();
    let mut dir_paths = HashSet::new();
    let mut book_paths = HashSet::new();

    conn.get_book_ids(|row| {
        book_ids.insert(row.get(0).unwrap());
        Ok(())
    })
    .unwrap();

    conn.get_dir_paths(|row| {
        let value: String = row.get(0).unwrap();
        dir_paths.insert(PathBuf::from(value));
        Ok(())
    })
    .unwrap();

    conn.get_book_paths(|row| {
        let value: String = row.get(0).unwrap();
        book_paths.insert(PathBuf::from(value));
        Ok(())
    })
    .unwrap();

    (book_ids, dir_paths, book_paths)
}

pub fn get_sync_state(
    conn: &Connection,
) -> (HashSet<String>, HashSet<String>, HashSet<(String, String)>) {
    let mut book_ids = HashSet::new();
    let mut dir_paths = HashSet::new();
    let mut reading_positions = HashSet::new();

    conn.get_book_ids(|row| {
        book_ids.insert(row.get(0).unwrap());
        Ok(())
    })
    .unwrap();

    conn.get_dir_paths(|row| {
        let value: String = row.get(0).unwrap();
        dir_paths.insert(value);
        Ok(())
    })
    .unwrap();

    conn.get_all_reading_positions(|row| {
        let book_id: String = row.get(0).unwrap();
        let read_pos: String = row.get(1).unwrap();
        reading_positions.insert((book_id, read_pos));
        Ok(())
    })
    .unwrap();

    (book_ids, dir_paths, reading_positions)
}

pub fn get_dir_contents(conn: &Connection, dir_path: &str) -> (Vec<Dir>, Vec<Book>) {
    let mut books: Vec<Book> = Vec::new();
    let mut sub_dirs: Vec<Dir> = Vec::new();

    conn.get_sub_dirs(dir_path, |row| {
        let id= row.get(0)?;
        let name = row.get(1)?;
        sub_dirs.push(Dir { id, name });
        Ok(())
    })
    .unwrap();

    conn.get_books_in_dir(dir_path, |row| {
        let book_id: String = row.get(0)?;
        let title: String = row.get(1)?;
        books.push(Book::new(book_id.parse().unwrap(), title));
        Ok(())
    })
    .unwrap();
    (sub_dirs, books)
}

pub fn get_recently_read(conn: &Connection, limit: i32) -> Vec<Book> {
    let mut books: Vec<Book> = Vec::new();

    conn.get_recently_read(limit, |row| {
        let book_id: String = row.get(0)?;
        let title: String = row.get(1)?;
        books.push(Book::new(book_id.parse().unwrap(), title));
        Ok(())
    })
    .unwrap();
    books
}

pub fn get_recently_added(conn: &Connection, limit: i32) -> Vec<Book> {
    let mut books: Vec<Book> = Vec::new();
    conn.get_recently_added(limit, |row| {
        let book_id: String = row.get(0)?;
        let title: String = row.get(1)?;

        books.push(Book::new(book_id.parse().unwrap(), title));
        Ok(())
    })
    .unwrap();
    books
}

pub fn insert_book_with_title(conn: &Connection, book_id: &str, book_title: &str) {
    let added_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;

    match conn.insert_book(book_id, book_title, added_at) {
        Ok(_) => {}
        Err(_) => {
            let _ = conn.update_book_title(book_id, book_title);
        }
    }
}

pub fn get_book_file_path(conn: &Connection, book_id: &str) -> Option<String> {
    let mut file_path: Option<String> = None;

    conn.get_book_file_path(book_id, |row| {
        file_path = Some(row.get(0)?);
        Ok(())
    })
    .unwrap();
    file_path
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

pub fn update_reading_position(conn: &Connection, book_id: BookID, cfi: &str) {
    let book_id = book_id.to_string();
    conn.update_reading_position(&book_id, cfi).unwrap();
}

pub fn update_last_read(conn: &Connection, book_id: &str) {
    let last_read_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    conn.update_last_read(book_id, last_read_time).unwrap();
}

pub fn get_unpushed_events(conn: &Connection) -> Vec<Event> {
    let last_sent_seq = get_sync_metadata(conn, "last_pushed_local_seq")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let mut events = Vec::new();

    let _ = conn.get_unpushed_events(last_sent_seq, |row| {
        let entity_id: String = row.get("entity_id")?;
        let payload_text: String = row.get("payload")?;
        let timestamp: i64 = row.get("timestamp")?;
        let local_seq: i64 = row.get("local_seq")?;
        if let Some(payload) = EventPayload::from_json(&payload_text) {
            events.push(Event {
                entity_id,
                payload,
                timestamp: timestamp as u64,
                local_seq: Some(local_seq as u64),
                server_seq: None,
            });
        } else {
            log::warn!("Failed to parse event payload: {}", payload_text);
        }
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

pub fn apply_remote_event_in_tx(
    conn: &rusqlite::Connection,
    event: &Event,
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = RemoteChangeGuard::new(conn);
todo!()
}

pub fn apply_authoritative_event(conn: &Connection, event: &Event) {
    let _ = (conn, event);
    todo!()
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

pub fn set_book_uploaded(conn: &Connection, book_id: &str, uploaded: bool) {
    let value = if uploaded { 1 } else { 0 };
    let _ = conn.set_book_uploaded(book_id, value);
}

pub fn set_book_thumbnail_uploaded(conn: &Connection, book_id: &str, uploaded: bool) {
    let value = if uploaded { 1 } else { 0 };
    let _ = conn.set_book_thumbnail_uploaded(book_id, value);
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
    let _ = conn.delete_book_by_id(book_id);
}

pub fn upsert_dir(transaction: &Transaction, dir_uuid: &str, parent_uuid: &str, name: &str) {
    let _ = transaction.upsert_dir(dir_uuid, parent_uuid, name).unwrap();
}

pub struct BookDir {
    pub book_id: String,
    pub parent_id: String,
    pub file_name: String,
}

pub fn insert_book(transaction: &Transaction, book_id: u64) {
    let added_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    if let Err(error) = transaction.first_book_insert(&book_id.to_string(), added_at) {
        eprintln!("Error: {}", error);
        panic!();
    }
}

pub fn insert_local_book_dir(transaction: &Transaction, dir_uuid: &str, book_id: u64, file_name: &str, local_hash: u64) {
    transaction.insert_book_dir(dir_uuid, &book_id.to_string(), file_name, &local_hash.to_string(), 1).unwrap();
}

pub fn get_book_dir_by_hash(transaction: &Transaction, hash: u64) -> Option<BookDir> {
    let mut result: Option<BookDir> = None;
    let hash = hash.to_string();
    let query_result = transaction.get_book_dir_by_hash(&hash, |row| {
        result = Some(BookDir {
            book_id: row.get(0)?,
            parent_id: row.get(1)?,
            file_name: row.get(2)?,
        });
        Ok(())
    });
    if let Err(err) = query_result {
        eprintln!("Error: {}", err);
    }
    result
}



pub fn get_books_without_title(transaction: &Transaction ) -> Vec<(u64, PathBuf)> {
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

pub fn update_book_title(transaction: &Transaction, book_id: u64, title: &str) {
    let query_result = transaction.update_book_title(&book_id.to_string(), title);
    if let Err(err) = query_result {
        eprintln!("Error: {}", err);
    }
}

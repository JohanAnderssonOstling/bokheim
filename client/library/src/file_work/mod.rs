//! Device-local file operations committed with their metadata changes.
use rusqlite::Connection;
use rusqlite::Transaction;
use sync_common::ContentHash;
use sync_common::DirId;

/// Rewrite only materialized paths beneath the directory that was moved.
/// Must run in the caller's projection-publish transaction.
pub fn move_file_projection_paths(tx: &Transaction<'_>, old: &str, new: &str) -> rusqlite::Result<usize> {
    let old = old.trim_matches('/');
    let prefix = format!("/{old}/");
    // Under SQLite's BINARY ordering, '/' immediately precedes '0'. This
    // half-open range exactly matches the prefix, including Unicode names,
    // without LIKE wildcard/case semantics or touching sibling FooBar paths.
    let upper = format!("/{old}0");
    let replacement = format!("/{}/", new.trim_matches('/'));
    tx.execute(include_str!("sql/file_work/move_file_projection_paths_update.sql"), rusqlite::params![prefix, replacement, upper])
}

#[cfg(test)]
mod move_path_tests;

pub fn queue_file_work(conn: &Connection, operation: &str, hash: &ContentHash, path: &str) -> rusqlite::Result<()> {
    #[cfg(target_arch = "wasm32")]
    let _ = (conn, operation, hash, path);
    #[cfg(not(target_arch = "wasm32"))]
    conn.execute(include_str!("sql/file_work/queue_file_work_insert.sql"), rusqlite::params![operation, hash.as_str(), path])?;
    Ok(())
}

/// Replacing the row gives each new intent a distinct token. An older pass
/// can only acknowledge the intent it read, even if another pass runs concurrently.
pub fn queue_directory_work(conn: &Connection, dir: &DirId) -> rusqlite::Result<()> {
    #[cfg(target_arch = "wasm32")]
    let _ = (conn, dir);
    #[cfg(not(target_arch = "wasm32"))]
    conn.execute(include_str!("sql/file_work/queue_directory_work_insert.sql"), [dir.to_string()])?;
    Ok(())
}

pub fn queue_book_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    #[cfg(target_arch = "wasm32")]
    let _ = (conn, hash);
    #[cfg(not(target_arch = "wasm32"))]
    conn.execute(include_str!("sql/file_work/queue_book_work_insert.sql"), [hash.as_str()])?;
    Ok(())
}

pub fn has_pending_file_work(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/file_work/has_pending_file_work_select.sql"), [], |row| row.get(0))
}

/// File work that can still change this book's physical availability.
/// Include materialized placements because a move may have removed its source
/// from book_dir while the local file still lives there.
pub fn has_pending_book_file_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/file_work/has_pending_book_file_work_with.sql"), [hash.as_str()], |row| row.get(0))
}

pub fn placement_path(conn: &Connection, dir: &DirId, name: &str) -> rusqlite::Result<String> {
    let components = super::assets::get_dir_path_components(conn, &dir.to_string())?;
    let mut parts = components.into_iter().map(|component| component.name).filter(|name| !name.is_empty()).collect::<Vec<_>>();
    parts.push(name.to_owned());
    Ok(format!("/{}", parts.join("/")))
}

/// Inspect durable work without requiring the current schema to be installed.
pub fn has_pending_file_work_in_existing_schema(connection: &rusqlite::Connection) -> rusqlite::Result<bool> {
    for (table, sql) in [
        ("local_directory_work", include_str!("sql/pending_local_directory_work.sql")),
        ("local_book_work", include_str!("sql/pending_local_book_work.sql")),
        ("local_file_work", include_str!("sql/pending_local_file_work.sql")),
        ("local_purge_work", include_str!("sql/pending_local_purge_work.sql")),
    ] {
        let exists: bool = connection.query_row(include_str!("sql/has_pending_file_jobs_select.sql"), [table], |row| row.get(0))?;
        if exists && connection.query_row(sql, [], |row| row.get::<_, bool>(0))? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Publish a bounded group of book intents in the caller's transaction.
/// REPLACE allocates fresh tokens just like individual queue_book_work calls.
pub(crate) fn queue_book_work_batch(conn: &Connection, hashes: &[ContentHash]) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_arch = "wasm32")]
    let _ = (conn, hashes);
    #[cfg(not(target_arch = "wasm32"))]
    for chunk in hashes.chunks(100) {
        let keys = serde_json::to_string(&chunk.iter().map(ContentHash::as_str).collect::<Vec<_>>())?;
        conn.prepare_cached(include_str!("sql/file_work/queue_book_work_batch.sql"))?.execute([keys])?;
    }
    Ok(())
}

/// Whether book eviction still has physical work to finish.
pub fn has_pending_purge_work(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/pending_local_purge_work.sql"), [], |row| row.get(0))
}

/// Wake hints from durable library tables; workers still reconstruct their work.
#[derive(Clone, Copy, Debug, Default)]
pub struct WriteNotifications {
    pub metadata: bool,
    pub assets: bool,
}

pub fn write_notifications(action: rusqlite::hooks::Action, table: &str) -> WriteNotifications {
    use rusqlite::hooks::Action::SQLITE_DELETE;
    let metadata = match table {
        "sync_outbox" => action != SQLITE_DELETE,
        // Completing an upload releases already queued book state.
        "local_book_upload" => action == SQLITE_DELETE,
        _ => false,
    };
    let assets = match table {
        "asset_work" => action != SQLITE_DELETE,
        // Completing file work can unblock an already queued transfer.
        "local_file_work" | "local_directory_work" | "local_book_work" | "local_purge_work" | "local_thumbnail_work" => true,
        _ => false,
    };
    WriteNotifications { metadata, assets }
}

/// Discard queued restores when book bytes are deliberately evicted or purged.
pub fn cancel_book_restore(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<usize> {
    conn.execute(include_str!("sql/cancel_book_restore.sql"), [hash.as_str()])
}

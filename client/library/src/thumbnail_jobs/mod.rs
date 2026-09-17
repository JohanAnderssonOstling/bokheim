//! Device-local cover recovery, independent of account and network state.
use crate::storage_values::unix_secs;
use include_sqlite_sql::include_sql;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

pub fn request_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.request_thumbnail(hash.as_str())?;
    Ok(())
}

pub fn thumbnail_work_state(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<Option<(String, i64)>> {
    conn.thumbnail_work_state(hash.as_str(), |row| Ok((row.get(0)?, row.get(1)?))).optional()
}

pub fn complete_thumbnail_work(conn: &Connection, hash: &ContentHash, has_cover: bool) -> rusqlite::Result<()> {
    complete_thumbnail(conn, hash, has_cover, "local")
}

pub fn complete_downloaded_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    complete_thumbnail(conn, hash, true, "remote")
}

/// Finding cached bytes does not establish who produced them.
pub fn confirm_thumbnail_available(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    set_thumbnail_result(conn, hash, true)
}

fn complete_thumbnail(conn: &Connection, hash: &ContentHash, has_cover: bool, origin: &str) -> rusqlite::Result<()> {
    let transaction = if conn.is_autocommit() { Some(conn.unchecked_transaction()?) } else { None };
    conn.record_thumbnail_source(hash.as_str(), origin)?;
    set_thumbnail_result(conn, hash, has_cover)?;
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    Ok(())
}

fn set_thumbnail_result(conn: &Connection, hash: &ContentHash, has_cover: bool) -> rusqlite::Result<()> {
    conn.set_thumbnail_result(hash.as_str(), if has_cover { "ready" } else { "no_cover" })?;
    Ok(())
}

pub fn retry_thumbnail_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.retry_thumbnail(hash.as_str(), unix_secs()?.saturating_add(30))?;
    Ok(())
}

pub fn has_pending_thumbnail_work(conn: &Connection) -> rusqlite::Result<bool> {
    conn.has_pending_thumbnail_work(|row| row.get(0))
}

/// Missing files need recovery, but rescanning must not reset failed-job backoff
/// or repeatedly inspect a book known to contain no cover.
pub fn request_missing_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.request_missing_thumbnail(hash.as_str())?;
    Ok(())
}

/// Pending jobs only. Keep retry deadlines in the page so callers can retain
/// backoff without rescanning unrelated books or losing deferred work.
pub fn pending_thumbnail_jobs_page(conn: &Connection, after: &str) -> rusqlite::Result<Vec<(ContentHash, i64)>> {
    let mut jobs = Vec::new();
    conn.pending_thumbnail_jobs_page(after, |row| {
        jobs.push((ContentHash::new(&row.get::<_, String>(0)?), row.get(1)?));
        Ok(())
    })?;
    Ok(jobs)
}

/// Number of pending jobs, including jobs whose retry deadline is in the future.
pub fn pending_thumbnail_job_count(conn: &Connection) -> rusqlite::Result<u64> {
    Ok(conn.pending_thumbnail_job_count(|row| row.get::<_, i64>(0))?.max(0) as u64)
}

pub fn has_remote_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<bool> {
    conn.has_remote_thumbnail(hash.as_str(), |row| row.get(0))
}

pub fn discard_thumbnail_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.discard_thumbnail_work(hash.as_str())?;
    Ok(())
}

/// Retire local recovery and queue remote reconciliation atomically.
pub fn defer_thumbnail_to_download(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    discard_thumbnail_work(&transaction, hash)?;
    crate::assets::enqueue_asset_work(&transaction, hash)?;
    transaction.commit()
}

include_sql!("src/thumbnail_jobs/sql/thumbnail_jobs.sql");

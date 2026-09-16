//! Device-local cover recovery, independent of account and network state.
use crate::storage_values::unix_secs;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

pub fn request_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/thumbnail_jobs/request_thumbnail_insert.sql"), [hash.as_str()])?;
    Ok(())
}

pub fn thumbnail_work_state(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<Option<(String, i64)>> {
    conn.query_row(include_str!("sql/thumbnail_jobs/thumbnail_work_state_select.sql"), [hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).optional()
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
    conn.execute(include_str!("sql/thumbnail_jobs/complete_thumbnail_insert.sql"), rusqlite::params![hash.as_str(), origin])?;
    set_thumbnail_result(conn, hash, has_cover)?;
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    Ok(())
}

fn set_thumbnail_result(conn: &Connection, hash: &ContentHash, has_cover: bool) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/thumbnail_jobs/set_thumbnail_result_insert.sql"), rusqlite::params![hash.as_str(), if has_cover { "ready" } else { "no_cover" }])?;
    Ok(())
}

pub fn retry_thumbnail_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/thumbnail_jobs/retry_thumbnail_work_insert.sql"), rusqlite::params![hash.as_str(), unix_secs()?.saturating_add(30)])?;
    Ok(())
}

pub fn has_pending_thumbnail_work(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/thumbnail_jobs/has_pending_thumbnail_work_select.sql"), [], |row| row.get(0))
}

/// Missing files need recovery, but rescanning must not reset failed-job backoff
/// or repeatedly inspect a publication known to contain no cover.
pub fn request_missing_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/thumbnail_jobs/request_missing_thumbnail_insert.sql"), [hash.as_str()])?;
    Ok(())
}

/// Pending jobs only. Keep retry deadlines in the page so callers can retain
/// backoff without rescanning unrelated books or losing deferred work.
pub fn pending_thumbnail_jobs_page(conn: &Connection, after: &str) -> rusqlite::Result<Vec<(ContentHash, i64)>> {
    let mut statement = conn.prepare(include_str!("sql/thumbnail_jobs/pending_thumbnail_jobs_page_select.sql"))?;
    statement.query_map([after], |row| Ok((ContentHash::new(&row.get::<_, String>(0)?), row.get(1)?)))?.collect()
}

/// Number of pending jobs, including jobs whose retry deadline is in the future.
pub fn pending_thumbnail_job_count(conn: &Connection) -> rusqlite::Result<u64> {
    Ok(conn.query_row(include_str!("sql/thumbnail_jobs/pending_thumbnail_job_count.sql"), [], |row| row.get::<_, i64>(0))?.max(0) as u64)
}

pub fn has_remote_thumbnail(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/thumbnail_jobs/has_remote_thumbnail.sql"), [hash.as_str()], |row| row.get(0))
}

pub fn discard_thumbnail_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/thumbnail_jobs/discard_thumbnail_work.sql"), [hash.as_str()])?;
    Ok(())
}

/// Retire local recovery and queue remote reconciliation atomically.
pub fn defer_thumbnail_to_download(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    discard_thumbnail_work(&transaction, hash)?;
    crate::assets::enqueue_asset_work(&transaction, hash)?;
    transaction.commit()
}

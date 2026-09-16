//! Durable candidates for transfer work. Metadata and availability writes enqueue
//! affected books in the same transaction; settled books are never scanned again.
#[cfg(test)]
use crate::schema::init_db;
#[cfg(test)]
use crate::sync_state::get_pending_mutations;
#[cfg(test)]
use crate::sync_state::set_sync_metadata;
#[cfg(test)]
use crate::thumbnail_jobs::complete_thumbnail_work;
use rusqlite::Connection;
use sync_common::ContentHash;

pub fn enqueue_asset_work(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/work/enqueue_asset_work_insert.sql"), [hash.as_str()])?;
    Ok(())
}

pub fn asset_work_watermark(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(include_str!("sql/work/asset_work_watermark_select.sql"), [], |row| row.get(0))
}

pub fn asset_work_page(conn: &Connection, after: i64, through: i64) -> rusqlite::Result<Vec<(i64, ContentHash)>> {
    conn.prepare(include_str!("sql/work/asset_work_page_select.sql"))?.query_map(rusqlite::params![after, through], |row| Ok((row.get(0)?, ContentHash::new(&row.get::<_, String>(1)?))))?.collect()
}

pub fn complete_asset_work(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    // A newer write replaces the id, so finishing old work cannot erase it.
    conn.execute(include_str!("sql/work/complete_asset_work_delete.sql"), [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_work_is_atomic_durable_and_preserves_newer_writes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.db");
        let mut conn = Connection::open(&path).unwrap();
        init_db(&conn).unwrap();
        let hash = ContentHash::new(&"a".repeat(64));
        conn.execute(include_str!("sql/work/legacy_asset_trigger_upgrades_once_without_losing_work_insert.sql"), [hash.as_str()]).unwrap();
        let first = asset_work_watermark(&conn).unwrap();
        assert!(first > 0);
        {
            let tx = conn.transaction().unwrap();
            tx.execute(include_str!("sql/work/asset_work_is_atomic_durable_and_preserves_newer_writes_update.sql"), [hash.as_str()]).unwrap();
            // A rollback must also roll back its job.
        }
        assert_eq!(asset_work_watermark(&conn).unwrap(), first);
        conn.execute(include_str!("sql/work/asset_work_is_atomic_durable_and_preserves_newer_writes_update_2.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("sql/work/asset_work_is_atomic_durable_and_preserves_newer_writes_insert_2.sql"), [hash.as_str()]).unwrap();
        assert_eq!(asset_work_watermark(&conn).unwrap(), first, "metadata must not wake the asset worker");
        conn.execute(include_str!("sql/work/asset_work_is_atomic_durable_and_preserves_newer_writes_update.sql"), [hash.as_str()]).unwrap();
        let newer = asset_work_watermark(&conn).unwrap();
        assert!(newer > first);
        complete_asset_work(&conn, first).unwrap();
        assert_eq!(asset_work_page(&conn, 0, newer).unwrap(), vec![(newer, hash)]);
        drop(conn);
        let conn = Connection::open(&path).unwrap();
        init_db(&conn).unwrap();
        assert_eq!(asset_work_page(&conn, 0, newer).unwrap(), vec![(newer, hash)]);
        complete_asset_work(&conn, newer).unwrap();
        assert!(asset_work_page(&conn, 0, i64::MAX).unwrap().is_empty());
    }

    #[test]
    fn remote_commits_and_thumbnail_completion_enqueue_without_local_outbox_writes() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        set_sync_metadata(&conn, "change_origin", "remote").unwrap();
        let hash = ContentHash::new(&"b".repeat(64));
        conn.execute(include_str!("sql/work/legacy_asset_trigger_upgrades_once_without_losing_work_insert.sql"), [hash.as_str()]).unwrap();
        let first = asset_work_watermark(&conn).unwrap();
        assert!(first > 0);
        assert!(get_pending_mutations(&conn).unwrap().is_empty());
        complete_asset_work(&conn, first).unwrap();
        complete_thumbnail_work(&conn, &hash, true).unwrap();
        assert_eq!(asset_work_page(&conn, 0, i64::MAX).unwrap().len(), 1);
    }
}

/// Whether this asset has a user-requested download.
pub fn has_requested_book_download(conn: &rusqlite::Connection, hash: &sync_common::ContentHash) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/has_requested_book_download.sql"), [hash.as_str()], |row| row.get(0))
}

/// Whether this asset has an upload rejection within the retry window.
pub fn has_recent_rejected_upload(conn: &rusqlite::Connection, hash: &sync_common::ContentHash) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/has_recent_rejected_upload.sql"), [hash.as_str()], |row| row.get(0))
}

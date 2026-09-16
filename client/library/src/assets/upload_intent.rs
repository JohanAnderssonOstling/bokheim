//! Device-local upload provenance, separate from generic asset candidates.
//! Only import/scan observations may create intent; remote observations establish
//! a rescan baseline without becoming upload candidates.
use crate::assets::enqueue_asset_work;
#[cfg(test)]
use crate::placements::transfer_book_placement_record;
#[cfg(test)]
use crate::schema::init_db;
use crate::storage_values::invalid_text_column;
#[cfg(test)]
use crate::sync_state::rebind_sync_account;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use sync_common::ContentHash;
use sync_common::DirId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookUploadIntent {
    pub id: i64,
    pub content_hash: ContentHash,
    pub checksum: ContentHash,
    pub size_bytes: u64,
}

fn observe(tx: &Transaction<'_>, hash: &ContentHash, checksum: &ContentHash, size: u64, origin: &str) -> rusqlite::Result<bool> {
    let size = i64::try_from(size).map_err(|_| invalid_text_column(0, "publication size exceeds SQLite range"))?;
    Ok(tx.execute(include_str!("sql/upload_intent/observe_insert.sql"), rusqlite::params![hash.as_str(), checksum.as_str(), size, origin])? != 0)
}

/// Record a verified local version atomically with its metadata/placement.
/// Known versions (including downloaded bytes) do not create fresh work when
/// scanned again, moved, copied to another placement, or imported again.
pub fn observe_local_publication(tx: &Transaction<'_>, hash: &ContentHash, checksum: &ContentHash, size: u64) -> rusqlite::Result<()> {
    if !observe(tx, hash, checksum, size, "local")? {
        return Ok(());
    }
    enqueue_version(tx, hash, checksum, size)
}

fn enqueue_version(tx: &Transaction<'_>, hash: &ContentHash, checksum: &ContentHash, size: u64) -> rusqlite::Result<()> {
    tx.execute(include_str!("sql/upload_intent/enqueue_version_insert.sql"), rusqlite::params![hash.as_str(), checksum.as_str(), size as i64])?;
    tx.execute(include_str!("sql/upload_intent/enqueue_version_delete.sql"), [hash.as_str()])?;
    enqueue_asset_work(tx, hash)
}

/// The placement baseline distinguishes a new placement of known bytes from
/// an edit reverting an existing placement to an older, already seen checksum.
pub fn observe_local_placement_publication(tx: &Transaction<'_>, dir: &DirId, hash: &ContentHash, checksum: &ContentHash, size: u64) -> rusqlite::Result<()> {
    let previous: Option<String> = tx.query_row(include_str!("sql/upload_intent/observe_local_placement_publication_select.sql"), rusqlite::params![dir.to_string(), hash.as_str()], |row| row.get(0)).optional()?;
    if previous.as_deref() == Some(checksum.as_str()) {
        return Ok(());
    }
    let unseen = observe(tx, hash, checksum, size, "local")?;
    let changed = previous.is_some();
    if unseen || changed {
        enqueue_version(tx, hash, checksum, size)?;
    }
    let origin: String = if changed { "local".to_owned() } else { tx.query_row(include_str!("sql/upload_intent/observe_local_placement_publication_select_2.sql"), rusqlite::params![hash.as_str(), checksum.as_str()], |row| row.get(0))? };
    remember_placement(tx, dir, hash, checksum, &origin)
}

fn remember_placement(tx: &Transaction<'_>, dir: &DirId, hash: &ContentHash, checksum: &ContentHash, origin: &str) -> rusqlite::Result<()> {
    tx.execute(include_str!("sql/upload_intent/remember_placement_insert.sql"), rusqlite::params![dir.to_string(), hash.as_str(), checksum.as_str(), origin])?;
    Ok(())
}

pub fn observe_downloaded_placement_publication(tx: &Transaction<'_>, dir: &DirId, hash: &ContentHash, checksum: &ContentHash, size: u64) -> rusqlite::Result<()> {
    observe_downloaded_publication(tx, hash, checksum, size)?;
    remember_placement(tx, dir, hash, checksum, "remote")
}

/// Successful downloads establish provenance but never enqueue upload intent,
/// nor erase a different pending local version.
pub fn observe_downloaded_publication(tx: &Transaction<'_>, hash: &ContentHash, checksum: &ContentHash, size: u64) -> rusqlite::Result<()> {
    observe(tx, hash, checksum, size, "remote")?;
    Ok(())
}

pub fn book_upload_intent(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<Option<BookUploadIntent>> {
    conn.query_row(include_str!("sql/upload_intent/book_upload_intent_select.sql"), [hash.as_str()], |row| {
        Ok(BookUploadIntent { id: row.get(0)?, content_hash: *hash, checksum: ContentHash::new(&row.get::<_, String>(1)?), size_bytes: row.get::<_, i64>(2)? as u64 })
    })
    .optional()
}

/// Only confirmation for the exact queued generation completes the job.
pub fn complete_book_upload(tx: &Transaction<'_>, job: &BookUploadIntent) -> rusqlite::Result<bool> {
    Ok(tx.execute(include_str!("sql/upload_intent/complete_book_upload_delete.sql"), rusqlite::params![job.id, job.content_hash.as_str(), job.checksum.as_str()])? != 0)
}

/// Preserve source provenance across a metadata-only copy/move. No upload work.
pub(crate) fn copy_publication_baseline(tx: &Transaction<'_>, source: &DirId, destination: &DirId, hash: &ContentHash) -> rusqlite::Result<()> {
    tx.execute(include_str!("sql/upload_intent/copy_publication_baseline_insert.sql"), rusqlite::params![source.to_string(), destination.to_string(), hash.as_str()])?;
    Ok(())
}

/// A new account needs its own binding for locally sourced bytes, but receiving
/// or downloading books never enrolls them. Existing uncompleted jobs win.
pub(crate) fn requeue_local_uploads(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(include_str!("sql/upload_intent/requeue_local_uploads_with.sql"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_storage_reenable_includes_downloaded_local_copies() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let hash = test_support::fixture_content_hash(11);
        conn.execute(include_str!("sql/work/legacy_asset_trigger_upgrades_once_without_losing_work_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("sql/upload_intent/account_rebind_requeues_local_sources_but_not_downloads_and_rotates_pending_tokens_insert_2.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str(), "book.epub"])
            .unwrap();
        let tx = conn.transaction().unwrap();
        observe_downloaded_placement_publication(&tx, &sync_common::ROOT_DIR_ID, &hash, &hash, 100).unwrap();
        tx.commit().unwrap();
        assert!(book_upload_intent(&conn, &hash).unwrap().is_none());
        crate::assets::reset_cloud_presence(&conn, true).unwrap();
        assert_eq!(book_upload_intent(&conn, &hash).unwrap().unwrap().checksum, hash);
    }

    #[test]
    fn account_rebind_requeues_local_sources_but_not_downloads_and_rotates_pending_tokens() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let local = test_support::fixture_content_hash(10);
        let remote = test_support::fixture_content_hash(11);
        for hash in [local, remote] {
            conn.execute(include_str!("sql/work/legacy_asset_trigger_upgrades_once_without_losing_work_insert.sql"), [hash.as_str()]).unwrap();
            conn.execute(
                include_str!("sql/upload_intent/account_rebind_requeues_local_sources_but_not_downloads_and_rotates_pending_tokens_insert_2.sql"),
                rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str(), format!("{hash}.epub")],
            )
            .unwrap();
        }
        let tx = conn.transaction().unwrap();
        observe_local_placement_publication(&tx, &sync_common::ROOT_DIR_ID, &local, &local, 100).unwrap();
        observe_downloaded_placement_publication(&tx, &sync_common::ROOT_DIR_ID, &remote, &remote, 100).unwrap();
        let original = book_upload_intent(&tx, &local).unwrap().unwrap();
        complete_book_upload(&tx, &original).unwrap();
        let destination = DirId::new_v4();
        tx.execute(include_str!("sql/upload_intent/account_rebind_requeues_local_sources_but_not_downloads_and_rotates_pending_tokens_insert_3.sql"), rusqlite::params![destination.to_string(), sync_common::ROOT_DIR_ID.to_string()])
            .unwrap();
        transfer_book_placement_record(&tx, &local, &sync_common::ROOT_DIR_ID, &destination, true).unwrap();
        transfer_book_placement_record(&tx, &remote, &sync_common::ROOT_DIR_ID, &destination, false).unwrap();
        assert!(book_upload_intent(&tx, &local).unwrap().is_none(), "moving is not an upload");
        assert!(book_upload_intent(&tx, &remote).unwrap().is_none(), "copying downloaded bytes is not an upload");
        tx.commit().unwrap();
        rebind_sync_account(&conn, "new-account").unwrap();
        let rebound = book_upload_intent(&conn, &local).unwrap().unwrap();
        assert!(rebound.id > original.id);
        assert!(book_upload_intent(&conn, &remote).unwrap().is_none());
        rebind_sync_account(&conn, "another-account").unwrap();
        let latest = book_upload_intent(&conn, &local).unwrap().unwrap();
        assert!(latest.id > rebound.id);
        assert_eq!(latest.checksum, local);
        let tx = conn.transaction().unwrap();
        assert!(!complete_book_upload(&tx, &rebound).unwrap());
        tx.commit().unwrap();
    }

    #[test]
    fn placement_upload_intent_ignores_copies_but_tracks_edits_and_reversions() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let book = test_support::fixture_content_hash(1);
        let downloaded = test_support::fixture_content_hash(2);
        let edited = test_support::fixture_content_hash(3);
        let original = DirId::new_v4();
        let copy = DirId::new_v4();
        conn.execute(include_str!("sql/work/legacy_asset_trigger_upgrades_once_without_losing_work_insert.sql"), [book.as_str()]).unwrap();
        let tx = conn.transaction().unwrap();
        observe_downloaded_placement_publication(&tx, &original, &book, &downloaded, 100).unwrap();
        observe_local_placement_publication(&tx, &original, &book, &downloaded, 100).unwrap();
        observe_local_placement_publication(&tx, &copy, &book, &downloaded, 100).unwrap();
        assert!(book_upload_intent(&tx, &book).unwrap().is_none());
        observe_local_placement_publication(&tx, &original, &book, &edited, 120).unwrap();
        let edit = book_upload_intent(&tx, &book).unwrap().unwrap();
        observe_local_placement_publication(&tx, &original, &book, &edited, 120).unwrap();
        assert_eq!(book_upload_intent(&tx, &book).unwrap(), Some(edit.clone()));
        assert!(complete_book_upload(&tx, &edit).unwrap());
        observe_local_placement_publication(&tx, &original, &book, &downloaded, 100).unwrap();
        let revert = book_upload_intent(&tx, &book).unwrap().unwrap();
        assert_eq!(revert.checksum, downloaded);
        assert!(revert.id > edit.id);
        assert!(!complete_book_upload(&tx, &edit).unwrap());
        // A downloaded second placement cannot erase the first placement's edit.
        observe_downloaded_placement_publication(&tx, &copy, &book, &downloaded, 100).unwrap();
        assert_eq!(book_upload_intent(&tx, &book).unwrap(), Some(revert));
        let origins: Vec<String> =
            tx.prepare(include_str!("sql/upload_intent/placement_upload_intent_ignores_copies_but_tracks_edits_and_reversions_select.sql")).unwrap().query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(origins, ["local", "remote"]);
        tx.commit().unwrap();
    }

    #[test]
    fn provenance_intent_survives_restart_and_newer_versions_survive_old_ack() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.db");
        let mut conn = Connection::open(&path).unwrap();
        init_db(&conn).unwrap();
        let book = test_support::fixture_content_hash(1);
        let downloaded = test_support::fixture_content_hash(2);
        let edited = test_support::fixture_content_hash(3);
        conn.execute(include_str!("sql/work/legacy_asset_trigger_upgrades_once_without_losing_work_insert.sql"), [book.as_str()]).unwrap();
        let tx = conn.transaction().unwrap();
        observe_downloaded_publication(&tx, &book, &downloaded, 100).unwrap();
        observe_local_publication(&tx, &book, &downloaded, 100).unwrap();
        assert!(book_upload_intent(&tx, &book).unwrap().is_none());
        observe_local_publication(&tx, &book, &edited, 120).unwrap();
        let first = book_upload_intent(&tx, &book).unwrap().unwrap();
        observe_local_publication(&tx, &book, &edited, 120).unwrap();
        assert_eq!(book_upload_intent(&tx, &book).unwrap(), Some(first.clone()));
        tx.commit().unwrap();
        drop(conn);
        let mut conn = Connection::open(path).unwrap();
        init_db(&conn).unwrap();
        assert_eq!(book_upload_intent(&conn, &book).unwrap(), Some(first.clone()));
        let newer = test_support::fixture_content_hash(4);
        let tx = conn.transaction().unwrap();
        observe_local_publication(&tx, &book, &newer, 130).unwrap();
        assert!(!complete_book_upload(&tx, &first).unwrap());
        tx.rollback().unwrap();
        assert_eq!(book_upload_intent(&conn, &book).unwrap(), Some(first.clone()));
        let tx = conn.transaction().unwrap();
        observe_local_publication(&tx, &book, &newer, 130).unwrap();
        let second = book_upload_intent(&tx, &book).unwrap().unwrap();
        assert!(!complete_book_upload(&tx, &first).unwrap());
        observe_downloaded_publication(&tx, &book, &downloaded, 100).unwrap();
        assert_eq!(book_upload_intent(&tx, &book).unwrap(), Some(second.clone()));
        assert!(complete_book_upload(&tx, &second).unwrap());
        observe_local_publication(&tx, &book, &newer, 130).unwrap();
        assert!(book_upload_intent(&tx, &book).unwrap().is_none());
        tx.commit().unwrap();
        let schema: i64 = conn.pragma_query_value(None, "schema_version", |row| row.get(0)).unwrap();
        init_db(&conn).unwrap();
        assert_eq!(conn.pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0)).unwrap(), schema);
    }
}

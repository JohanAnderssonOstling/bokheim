use crate::storage_values::unix_secs;
use library_replica::BlobKind;
use rusqlite::Connection;
use std::collections::HashSet;
use sync_common::ContentHash;

pub fn rejected_asset_upload_hashes(conn: &Connection, kind: BlobKind) -> Result<HashSet<ContentHash>, rusqlite::Error> {
    // Keep in sync with the five-minute expiry predicates in assets/sql.
    const REJECTION_BACKOFF_SECONDS: i64 = 5 * 60;
    let retry_cutoff = unix_secs()?.saturating_sub(REJECTION_BACKOFF_SECONDS);
    conn.execute(include_str!("sql/rejections/rejected_asset_upload_hashes_delete.sql"), [retry_cutoff])?;
    let mut statement = conn.prepare(include_str!("sql/rejections/rejected_asset_upload_hashes_select.sql"))?;
    let rows = statement.query_map(rusqlite::params![kind.storage(), retry_cutoff], |row| row.get::<_, String>(0))?;
    rows.map(|row| row.map(|hash| ContentHash::new(&hash))).collect()
}

pub fn record_rejected_asset_upload(conn: &Connection, kind: BlobKind, content_hash: &ContentHash, reason: &str) -> Result<(), rusqlite::Error> {
    conn.execute(include_str!("sql/rejections/record_rejected_asset_upload_insert.sql"), rusqlite::params![kind.storage(), content_hash.as_str(), reason, unix_secs()?])?;
    Ok(())
}

/// Earliest future expiry of an upload rejection, as Unix seconds.
pub fn next_upload_retry_at(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(include_str!("sql/rejections/next_upload_retry_at.sql"), [], |row| row.get(0))
}

/// Current durable rejection for a locally authored book upload.
pub fn book_upload_rejection_reason(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<Option<String>> {
    conn.query_row(include_str!("sql/rejections/book_upload_rejection_reason.sql"), [hash.as_str()], |row| row.get(0))
}

/// Rejected asset work whose persistence must be settled atomically.
pub enum RejectedAsset<'a> {
    BookDownload(&'a ContentHash),
    BookUpload { hash: &'a ContentHash, intent: Option<&'a super::BookUploadIntent> },
    ThumbnailUpload(&'a ContentHash),
}

pub fn settle_rejected_asset(conn: &mut Connection, asset: RejectedAsset<'_>, reason: &str) -> rusqlite::Result<()> {
    let tx = crate::schema::library_write_transaction(conn)?;
    match asset {
        RejectedAsset::BookDownload(hash) => super::complete_asset_request(&tx, BlobKind::Book, hash)?,
        RejectedAsset::BookUpload { hash, intent } => {
            if intent.is_none() || super::book_upload_intent(&tx, hash)?.as_ref() == intent {
                record_rejected_asset_upload(&tx, BlobKind::Book, hash, reason)?;
            }
        }
        RejectedAsset::ThumbnailUpload(hash) => record_rejected_asset_upload(&tx, BlobKind::Thumbnail, hash, reason)?,
    }
    tx.commit()
}

/// Whether pending book uploads have a recent server quota rejection.
pub fn book_uploads_waiting_for_storage(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/rejections/book_uploads_waiting_for_storage.sql"), [], |row| row.get(0))
}

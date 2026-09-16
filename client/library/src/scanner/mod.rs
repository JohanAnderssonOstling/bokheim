use crate::books::queries::BooksSql;
use crate::scanner::queries::ScannerSql;
use crate::storage_values::invalid_text_column;
use crate::storage_values::parse_uuid_column;
use crate::storage_values::unix_millis;
use crate::sync_state::get_sync_metadata;
use crate::sync_state::set_sync_metadata;
use library_model::BookFormat;
use rusqlite::Connection;
use rusqlite::Transaction;
use std::collections::HashMap;
use std::collections::HashSet;
use sync_common::ContentHash;
use sync_common::DirId;
use sync_common::FileName;

pub fn next_scan_id(conn: &Connection) -> Result<i64, rusqlite::Error> {
    let current = get_sync_metadata(conn, "scan_seq")?.map_or(Ok(0), |value| value.parse::<i64>().map_err(|error| invalid_text_column(0, error.to_string())))?;
    let next = current.checked_add(1).ok_or_else(|| invalid_text_column(0, "scan sequence overflow"))?;
    set_sync_metadata(conn, "scan_seq", &next.to_string())?;
    Ok(next)
}

#[cfg(any(test, feature = "scanner"))]
pub const VANISH_GRACE_SCANS: i64 = 2;

#[cfg(any(test, feature = "scanner"))]
pub fn count_pending_vanish_candidates(conn: &Connection, scan_id: i64) -> Result<i64, rusqlite::Error> {
    let mut count = 0;
    conn.count_pending_vanish_candidates(scan_id, VANISH_GRACE_SCANS, |row| {
        count = row.get(0)?;
        Ok(())
    })?;
    Ok(count)
}

#[cfg(any(test, feature = "scanner"))]
#[derive(Clone)]
pub struct BookDir {
    pub content_hash: ContentHash,
    pub dir_id: DirId,
    pub file_name: FileName,
    pub description_missing: bool,
}

#[cfg(any(test, feature = "scanner"))]
pub fn get_book_dirs_by_local_fingerprint(conn: &Connection) -> Result<HashMap<u64, BookDir>, rusqlite::Error> {
    let mut result = HashMap::new();
    conn.get_local_fingerprint_book_dirs(|row| {
        let Ok(fingerprint) = row.get::<_, String>(0)?.parse::<u64>() else { return Ok(()) };
        result.entry(fingerprint).or_insert(BookDir { content_hash: ContentHash::new(&row.get::<_, String>(1)?), dir_id: parse_uuid_column(row.get::<_, String>(2)?, 2)?, file_name: row.get(3)?, description_missing: row.get(5)? });
        Ok(())
    })?;
    Ok(result)
}

#[cfg(any(test, feature = "scanner"))]
pub fn get_tombstoned_book_file_keys(conn: &Connection) -> Result<HashSet<(DirId, FileName)>, rusqlite::Error> {
    let mut statement = conn.prepare_cached(include_str!("sql/scanner/get_tombstoned_book_file_keys_select.sql"))?;
    statement.query_map([], |row| Ok((parse_uuid_column(row.get(0)?, 0)?, row.get(1)?)))?.collect()
}

#[cfg(any(test, feature = "scanner"))]
pub fn insert_book(transaction: &Transaction, content_hash: ContentHash, format: BookFormat) -> Result<(), rusqlite::Error> {
    transaction.insert_scanned_book(content_hash.as_str(), unix_millis()?, format.canonical_extension())?;
    Ok(())
}

pub fn insert_local_book_dir(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash, file_name: &FileName, local_hash: u64, last_scan: i64) -> Result<(), rusqlite::Error> {
    transaction.insert_book_dir(&dir_id.to_string(), content_hash.as_str(), file_name, &local_hash.to_string(), last_scan, 1)?;
    transaction.execute(include_str!("sql/scanner/insert_local_book_dir_update.sql"), rusqlite::params![dir_id.to_string(), content_hash.as_str(), file_name])?;
    Ok(())
}

pub fn insert_imported_book_dir(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash, file_name: &FileName, local_hash: Option<u64>, last_scan: i64) -> Result<(), rusqlite::Error> {
    transaction.insert_book_dir(&dir_id.to_string(), content_hash.as_str(), file_name, &local_hash.map(|value| value.to_string()).unwrap_or_default(), last_scan, 1)?;
    Ok(())
}

#[cfg(any(test, feature = "scanner"))]
pub fn mark_known_book_dirs_seen(transaction: &Transaction, scan_id: i64, placements: &[(DirId, FileName, u64)]) -> Result<(), rusqlite::Error> {
    for placements in placements.chunks(250) {
        if placements.is_empty() {
            continue;
        }
        let values = placements.iter().map(|(dir_id, file_name, local_hash)| (dir_id.to_string(), file_name, local_hash.to_string())).collect::<Vec<_>>();
        let values = serde_json::to_string(&values).map_err(|error| invalid_text_column(0, error.to_string()))?;
        transaction.execute(include_str!("sql/mark_known_book_dirs_seen.sql"), rusqlite::params![scan_id, values])?;
    }
    Ok(())
}

#[cfg(any(test, feature = "scanner"))]
pub fn book_has_live_placement(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash) -> Result<bool, rusqlite::Error> {
    let mut exists = false;
    transaction.book_has_live_placement(&dir_id.to_string(), content_hash.as_str(), |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}

/// Called only after a complete, readable scan of the original library root.
#[cfg(any(test, feature = "scanner"))]
pub fn delete_vanished_placements(conn: &Connection, scan_id: i64) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    delete_vanished_placements_in_tx(&tx, scan_id)?;
    tx.commit()
}

#[cfg(any(test, feature = "scanner"))]
pub fn delete_vanished_placements_in_tx(tx: &Transaction, scan_id: i64) -> rusqlite::Result<()> {
    let missing = tx
        .prepare(include_str!("sql/scanner/delete_vanished_placements_in_tx_select.sql"))?
        .query_map(rusqlite::params![scan_id, VANISH_GRACE_SCANS], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (hash, dir) in missing {
        super::placements::remove_book_placement_record(&tx, &ContentHash::new(&hash), &parse_uuid_column(dir, 0)?)?;
    }
    Ok(())
}

pub(crate) mod queries;

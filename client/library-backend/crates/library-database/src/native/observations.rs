//! Local file facts shared by import, scan, download, and physical reconciliation.
pub(crate) use crate::DatabaseError;
use crate::SqlParam;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use rusqlite::Connection;
pub(crate) use sync_common::ContentHash;

pub(crate) enum Revision {
    Local { size: i64 },
    Remote { size: i64 },
    Retained,
}

/// Record a verified local file, including an already-existing destination.
/// Membership and upload scheduling are deliberately separate operations.
pub(crate) fn record_file(connection: &Connection, directory: &str, hash: &ContentHash, path: &str, fingerprint: Option<&str>, revision: Revision) -> Result<(), DatabaseError> {
    #[cfg(not(target_arch = "wasm32"))]
    connection.shared_scanner_publish_projection(hash, directory, path)?;
    #[cfg(target_arch = "wasm32")]
    let _ = path;
    match revision {
        Revision::Local { size } => {
            let mut seen = false;
            connection.shared_bytes_seen(hash, |row| {
                seen = row.get(0)?;
                Ok(())
            })?;
            if !seen {
                connection.shared_scanner_enqueue_upload(hash, size)?;
            }
            connection.shared_record_size(directory, hash, size)?;
        }
        Revision::Remote { size } => {
            connection.shared_record_size(directory, hash, size)?;
        }
        Revision::Retained => {}
    }
    connection.observations_mark_downloaded(&directory, &(&hash), &fingerprint)?;
    // Browser placements share one OPFS object; native placements are files.
    #[cfg(target_arch = "wasm32")]
    connection.shared_set_book_hash_downloaded(&hash, 1)?;
    Ok(())
}

/// Forget one observed file without changing synchronized membership.
pub(crate) fn forget_file(connection: &Connection, directory: &str, hash: &ContentHash) -> Result<(), DatabaseError> {
    connection.observations_mark_missing(&directory, &(&hash))?;
    connection.observations_remove_file(&directory, &(&hash))?;
    Ok(())
}

include_sqlite_sql::include_sql!("src/native/sql/observations.sql");

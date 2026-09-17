use crate::assets::AssetsSql;
use library_replica::BlobKind;
use rusqlite::Connection;
use std::collections::HashSet;
use sync_common::ContentHash;

pub fn get_remote_asset_hashes(conn: &Connection, kind: BlobKind) -> Result<HashSet<ContentHash>, rusqlite::Error> {
    let mut hashes = HashSet::new();
    conn.get_remote_asset_hashes(kind.storage(), |row| {
        hashes.insert(ContentHash::new(&row.get::<_, String>(0)?));
        Ok(())
    })?;
    Ok(hashes)
}

pub fn record_remote_asset(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    conn.record_remote_asset(kind.storage(), content_hash.as_str())?;
    Ok(())
}

pub fn forget_remote_asset(conn: &Connection, kind: BlobKind, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    conn.forget_remote_asset(kind.storage(), content_hash.as_str())?;
    Ok(())
}

/// Re-negotiate local bytes after account-wide retention changes.
pub fn reset_cloud_presence(conn: &Connection, enabled: bool) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(include_str!("sql/reset_cloud_presence.sql"))?;
    tx.execute_batch(include_str!("sql/requeue_cloud_storage.sql"))?;
    set_cloud_storage_policy(&tx, enabled)?;
    tx.commit()
}

pub fn set_cloud_storage_policy(conn: &Connection, enabled: bool) -> rusqlite::Result<()> {
    conn.execute(include_str!("sql/cloud_storage_policy.sql"), [if enabled { "1" } else { "0" }])?;
    Ok(())
}

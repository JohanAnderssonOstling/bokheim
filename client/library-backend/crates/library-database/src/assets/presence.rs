use super::TransfersSql;
use crate::*;
use library_model::BlobKind;
use std::collections::HashSet;
use sync_common::ContentHash;

impl Database {
    /// Records that the server already holds these bytes.
    pub fn record_remote_asset(&self, kind: BlobKind, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.record_remote_asset(kind.storage(), hash)?;
        Ok(())
    }

    /// Forgets server-side presence, e.g. after a local edit.
    pub fn forget_remote_asset(&self, kind: BlobKind, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.forget_remote_asset(kind.storage(), hash)?;
        Ok(())
    }

    /// Hashes the server is known to hold for one asset kind.
    pub fn remote_asset_hashes(&self, kind: BlobKind) -> Result<HashSet<ContentHash>, DatabaseError> {
        let mut hashes = HashSet::new();
        self.connection.remote_asset_hashes(kind.storage(), |row| {
            hashes.insert(row.get::<_, ContentHash>(0)?);
            Ok(())
        })?;
        Ok(hashes)
    }

    /// Requested book downloads, oldest first.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn book_download_requests(&self, after_at: i64, after_hash: Option<&ContentHash>, limit: i64) -> Result<Vec<(ContentHash, i64)>, DatabaseError> {
        let mut rows = Vec::new();
        self.connection.book_download_requests(after_at, after_hash, limit, |row| {
            rows.push((row.get::<_, ContentHash>(0)?, row.get(1)?));
            Ok(())
        })?;
        Ok(rows)
    }

}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn download_cursor_pages_raw_hashes_at_equal_timestamps() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let first = ContentHash::from_bytes([0;32]);
        let second = ContentHash::from_bytes([0xff;32]);
        for hash in [second, first] {
            db.connection.fixtures_insert_book_7(&hash).unwrap();
            db.connection.fixtures_insert_book_transfer(&hash).unwrap();
        }
        assert_eq!(db.book_download_requests(-1,None,1).unwrap(),vec![(first,10)]);
        assert_eq!(db.book_download_requests(10,Some(&first),1).unwrap(),vec![(second,10)]);
        assert!(db.book_download_requests(10,Some(&second),1).unwrap().is_empty());
    }
}

#[cfg(test)]
use crate::sql_fixtures::FixturesSql;

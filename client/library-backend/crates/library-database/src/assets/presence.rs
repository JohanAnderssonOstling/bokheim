use super::TransfersSql;
use crate::*;
use library_model::{BlobKind, TransferOrigin};
use std::collections::HashSet;
use sync_common::ContentHash;

impl Database {
    /// Records that the server already holds these bytes.
    pub fn record_remote_asset(&self, kind: BlobKind, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.record_remote_asset(kind.storage(), hash.as_str())?;
        Ok(())
    }

    /// Forgets server-side presence, e.g. after a local edit.
    pub fn forget_remote_asset(&self, kind: BlobKind, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.forget_remote_asset(kind.storage(), hash.as_str())?;
        Ok(())
    }

    /// Hashes the server is known to hold for one asset kind.
    pub fn remote_asset_hashes(&self, kind: BlobKind) -> Result<HashSet<ContentHash>, DatabaseError> {
        let mut hashes = HashSet::new();
        self.connection.remote_asset_hashes(kind.storage(), |row| {
            hashes.insert(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    /// Requested book downloads, oldest first.
    pub fn book_download_requests(&self) -> Result<Vec<(ContentHash, TransferOrigin)>, DatabaseError> {
        let mut rows = Vec::new();
        self.connection.book_download_requests(|row| {
            rows.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        rows.into_iter()
            .map(|(hash, origin)| {
                let origin = TransferOrigin::parse_storage(&origin).ok_or_else(|| DatabaseError::message("invalid transfer origin"))?;
                Ok((ContentHash::new(&hash), origin))
            })
            .collect()
    }

}

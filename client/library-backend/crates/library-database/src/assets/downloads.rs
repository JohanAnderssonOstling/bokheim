pub(crate) use super::TransfersSql;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::*;
pub(crate) use library_model::BlobKind;
pub(crate) use library_model::DownloadState;
pub(crate) use library_model::{BookPlacement, RelativeBookPath};
pub(crate) use sync_common::ContentHash;

impl Database {
    pub fn is_book_downloaded(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        Ok(self.download_flags(hash)?.0)
    }

    /// Whether the book's bytes are marked downloaded, and whether a live
    /// placement has a queued download request.
    pub(crate) fn download_flags(&self, hash: &ContentHash) -> Result<(bool, bool), DatabaseError> {
        let mut flags = (false, false);
        self.connection.transfer_download_state(hash.as_str(), |row| {
            flags = (row.get(0)?, row.get(1)?);
            Ok(())
        })?;
        Ok(flags)
    }

    pub fn evict_local_book(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        self.write(|transaction| {
            if !self.is_book_downloaded(hash)? {
                return Ok(false);
            }
            // Eviction keeps the entry but drops its bytes. Without a server copy
            // those bytes are the only copy, so refusing is what keeps the entry
            // recoverable.
            if !self.remote_asset_hashes(BlobKind::Book)?.contains(hash) {
                return Err(DatabaseError::message("this book is only stored on this device"));
            }
            transaction.shared_transfer_complete_download(hash.as_str())?;
            transaction.execute("UPDATE book_dir SET is_downloaded=0,local_hash='',checksum=NULL,size_bytes=NULL,checksum_seq=NULL WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [hash.as_str()])?;
            transaction.execute("DELETE FROM local_file_projection WHERE content_hash=?1", [hash.as_str()])?;
            #[cfg(not(target_arch = "wasm32"))]
            transaction.shared_cancel_book_materialization(hash.as_str())?;
            Ok(true)
        })
    }

    pub fn has_pending_asset_work(&self) -> Result<bool, DatabaseError> {
        let mut pending = false;
        let live_dirs = serde_json::to_string(
            &({
                use crate::placements::*;
                let connection: &rusqlite::Connection = &self.connection;

                walk_down(connection, vec![sync_common::ROOT_DIR_ID.to_string()], include_str!("../placements/sql/directory_live_children.sql"))?
            }),
        )
        .map_err(DatabaseError::operation)?;
        self.connection.transfer_has_pending_asset_work(&live_dirs, super::rejection_cutoff()?, |row| {
            pending = row.get(0)?;
            Ok(())
        })?;
        Ok(pending)
    }

    pub fn complete_local_book_request_on(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<(), DatabaseError> {
        #[cfg(target_arch = "wasm32")]
        {
            let retired = connection.query_row("SELECT NOT EXISTS(SELECT 1 FROM book WHERE content_hash=?1)", [hash.as_str()], |row| row.get::<_, bool>(0))?;
            if retired {
                // A download can publish after the original purge work was acknowledged.
                connection.shared_purge_book_record_insert(hash.as_str())?;
                return Ok(());
            }
            connection.shared_set_book_hash_downloaded(hash.as_str(), 1)?;
        }
        let mut placements = Vec::new();
        connection.shared_live_book_placements(hash.as_str(), |row| {
            placements.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let mut folders = crate::placements::FolderPaths::new(connection);
        for (dir_id, file_name) in placements {
            if let Some(path) = folders.placement(&dir_id, &file_name)? {
                connection.transfer_queue_materialization(hash.as_str(), path.as_str())?;
            }
        }
        connection.shared_transfer_complete_download(hash.as_str())?;
        Ok(())
    }

    pub fn sync_request_book_downloads(&self, hashes: &[ContentHash], origin: library_model::TransferOrigin) -> Result<(), DatabaseError> {
        let requested_at = crate::sync::unix_millis()?;
        self.write(|transaction| {
            for hash in hashes {
                let mut placed = false;
                transaction.transfer_download_placement(hash.as_str(), |row| {
                    placed = row.get(0)?;
                    Ok(())
                })?;
                if !placed {
                    return Err(DatabaseError::message(format!("book {hash} has no live placement")));
                }
                transaction.shared_request_book_download(hash.as_str(), origin.storage(), requested_at)?;
            }
            Ok(())
        })
    }

    pub fn transfer_download_state(&self, hash: &ContentHash, bytes_present: bool) -> Result<DownloadState, DatabaseError> {
        Ok(match (self.download_flags(hash)?, bytes_present) {
            ((true, _), true) => DownloadState::Downloaded,
            ((_, true), _) => DownloadState::Queued,
            _ => DownloadState::NotDownloaded,
        })
    }

    pub fn transfer_first_file_names(&self, hashes: &[ContentHash]) -> Result<Vec<(ContentHash, String)>, DatabaseError> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let payload = serde_json::to_string(&hashes.iter().map(ContentHash::to_string).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut names = Vec::new();
        self.connection.transfer_first_file_names(&payload, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            if let Some(name) = row.get::<_, Option<String>>(1)? {
                names.push((hash, name));
            }
            Ok(())
        })?;
        Ok(names)
    }

    pub fn is_book_download_requested(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        Ok(self.download_flags(hash)?.1)
    }
}

#[cfg(any(test, feature = "fixture-sql"))]
impl Database {
    pub fn complete_local_book_request(&self, hash: ContentHash) -> Result<(), DatabaseError> {
        Self::complete_local_book_request_on(&self.connection, &hash)
    }
}

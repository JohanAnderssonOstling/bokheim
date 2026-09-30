use super::TransfersSql;
use crate::shared_sql::SharedSql;
use crate::*;
use library_model::DownloadState;
use library_replica::BlobKind;
use sync_common::ContentHash;

impl Database {
    pub fn is_book_downloaded(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        Ok(self.download_flags(hash)?.0)
    }

    /// Whether the book's bytes are marked downloaded, and whether a live
    /// placement has a queued download request.
    fn download_flags(&self, hash: &ContentHash) -> Result<(bool, bool), DatabaseError> {
        let mut flags = (false, false);
        self.connection.transfer_download_state(hash.as_str(), |row| {
            flags = (row.get(0)?, row.get(1)?);
            Ok(())
        })?;
        Ok(flags)
    }

    pub fn evict_local_book(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        if !self.is_book_downloaded(hash)? {
            return Ok(false);
        }
        // Eviction keeps the entry but drops its bytes. Without a server copy
        // those bytes are the only copy, so refusing is what keeps the entry
        // recoverable.
        if !self.remote_asset_hashes(BlobKind::Book)?.contains(hash) {
            return Err(DatabaseError::message("this book is only stored on this device"));
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.shared_transfer_complete_download(hash.as_str())?;
        transaction.shared_set_book_hash_downloaded(hash.as_str(), 0)?;
        #[cfg(not(target_arch = "wasm32"))]
        transaction.shared_cancel_book_restore(hash.as_str())?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn local_file_work_status(&self) -> Result<(bool, Option<String>), DatabaseError> {
        #[cfg(target_arch = "wasm32")]
        return Ok((false, None));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut pending = false;
            self.connection.transfer_has_pending_file_work(|row| {
                pending = row.get(0)?;
                Ok(())
            })?;
            Ok((pending, None))
        }
    }

    /// The book's first remembered or live path on this device.
    fn book_path(&self, hash: &ContentHash) -> Result<Option<RelativeBookPath>, DatabaseError> {
        self.book_paths(hash)?.into_iter().next().map(|path| RelativeBookPath::parse(&path).map_err(DatabaseError::operation)).transpose()
    }

    pub fn has_pending_asset_work(&self) -> Result<bool, DatabaseError> {
        let mut pending = false;
        let live_dirs = serde_json::to_string(&crate::placements::live_directories(&self.connection)?).map_err(DatabaseError::operation)?;
        self.connection.transfer_has_pending_asset_work(&live_dirs, |row| {
            pending = row.get(0)?;
            Ok(())
        })?;
        Ok(pending)
    }

    pub fn requested_book_download(&self, hash: ContentHash) -> Result<Option<BookPlacement>, DatabaseError> {
        match self.book_path(&hash)? {
            Some(path) => Ok(Some(BookPlacement::new(hash, path))),
            None => {
                self.connection.shared_transfer_complete_download(hash.as_str())?;
                Ok(None)
            }
        }
    }

    pub fn pending_download_requests(&self) -> Result<Vec<ContentHash>, DatabaseError> {
        Ok(self.book_download_requests()?.into_iter().map(|(hash, _)| hash).collect())
    }

    pub fn complete_local_book_request(&self, hash: ContentHash) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        Self::complete_local_book_request_on(&transaction, &hash)?;
        transaction.commit().map_err(Into::into)
    }

    pub(super) fn complete_local_book_request_on(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<(), DatabaseError> {
        connection.shared_set_book_hash_downloaded(hash.as_str(), 1)?;
        let mut placements = Vec::new();
        connection.transfer_live_placements(hash.as_str(), |row| {
            placements.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let mut folders = crate::placements::FolderPaths::new(connection);
        for (dir_id, file_name) in placements {
            if let Some(path) = folders.placement(&dir_id, &file_name)? {
                connection.transfer_queue_restore(hash.as_str(), &path)?;
            }
        }
        connection.shared_transfer_complete_download(hash.as_str())?;
        Ok(())
    }

    pub fn complete_retained_download(&self, hash: &ContentHash, placement: &BookPlacement) -> Result<bool, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let matches = Self::complete_retained_download_on(&transaction, hash, placement)?;
        transaction.commit()?;
        Ok(matches)
    }

    pub(super) fn complete_retained_download_on(connection: &rusqlite::Connection, hash: &ContentHash, placement: &BookPlacement) -> Result<bool, DatabaseError> {
        if !Self::placement_holds(connection, placement, hash)? {
            return Ok(false);
        }
        connection.shared_set_book_hash_downloaded(hash.as_str(), 1)?;
        connection.shared_transfer_complete_download(hash.as_str())?;
        Ok(true)
    }

    pub fn cancel_book_download(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        Self::cancel_book_download_on(&transaction, hash)?;
        transaction.commit().map_err(Into::into)
    }

    pub(super) fn cancel_book_download_on(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<(), DatabaseError> {
        connection.forget_remote_asset(BlobKind::Book.storage(), hash.as_str())?;
        connection.shared_transfer_complete_download(hash.as_str())?;
        Ok(())
    }

    pub fn book_placement_is_current(&self, placement: &BookPlacement, hash: &ContentHash) -> Result<bool, DatabaseError> {
        if placement.content_hash != *hash {
            return Ok(false);
        }
        Self::placement_holds(&self.connection, placement, hash)
    }

    /// Whether the placement's path still resolves to a live placement of `hash`.
    fn placement_holds(connection: &rusqlite::Connection, placement: &BookPlacement, hash: &ContentHash) -> Result<bool, DatabaseError> {
        Ok(crate::placements::book_at_path(connection, placement.rel_path.as_str())?.is_some_and(|(current, _, _)| current == *hash))
    }

    pub fn native_download_directory(&self, placement: &BookPlacement, hash: &ContentHash) -> Result<Option<sync_common::DirId>, DatabaseError> {
        if placement.content_hash != *hash {
            return Ok(None);
        }
        Ok(crate::placements::book_at_path(&self.connection, placement.rel_path.as_str())?.filter(|(current, _, _)| current == hash).map(|(_, directory, _)| directory))
    }

    pub fn sync_request_book_downloads(&self, hashes: &[ContentHash], origin: library_replica::TransferOrigin) -> Result<(), DatabaseError> {
        let requested_at = crate::sync::unix_millis()?;
        let transaction = self.connection.unchecked_transaction()?;
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
        transaction.commit()?;
        Ok(())
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

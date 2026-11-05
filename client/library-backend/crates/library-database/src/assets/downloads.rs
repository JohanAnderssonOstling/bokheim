pub(crate) use super::TransfersSql;
use crate::content_identity::freeze_content;
#[cfg(not(target_arch = "wasm32"))]
use crate::query_methods::query_rows;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::*;
use content_address::ContentHash;
pub(crate) use library_model::DownloadState;
pub(crate) use sync_common::BookId;

impl Database {
    /// A shared book object belongs to the identity, regardless of where its
    /// live placements moved while the bytes were being downloaded.
    pub fn complete_stored_book_download(&self, hash: BookId, checksum: ContentHash, size_bytes: u64, thumbnail_set_exists: bool) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            let stored_size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
            if transaction.transfer_mark_stored_book(&hash, stored_size)? == 0 {
                return Err(DatabaseError::message("book no longer has a live placement"));
            }
            freeze_content(transaction, hash, checksum, Some(size_bytes))?;
            transaction.transfer_observe_stored_book(&hash, &checksum)?;
            transaction.record_remote_asset(library_model::BlobKind::Book.storage(), &hash)?;
            transaction.shared_transfer_complete_download(&hash)?;
            if !thumbnail_set_exists {
                transaction.shared_request_thumbnail(&hash)?;
            }
            Ok(())
        })
    }

    pub fn is_book_downloaded(&self, hash: &BookId) -> Result<bool, DatabaseError> {
        Ok(self.download_flags(hash)?.0)
    }

    /// Whether the book's bytes are marked downloaded, and whether a live
    /// placement has a queued download request.
    pub(crate) fn download_flags(&self, hash: &BookId) -> Result<(bool, bool), DatabaseError> {
        Ok(self.connection.transfer_download_state(hash, |row| Ok((row.get(0)?, row.get(1)?)))?)
    }

    /// Checks eviction eligibility without changing local availability. Native
    /// callers must run this before removing any files, then commit eviction.
    pub fn check_local_book_eviction(&self, hash: &BookId) -> Result<bool, DatabaseError> {
        if !self.is_book_downloaded(hash)? {
            return Ok(false);
        }
        // Eviction keeps the entry but drops its bytes. Without a server copy
        // those bytes are the only copy, so refusing keeps the entry recoverable.
        if !self.remote_book_available(hash)? {
            return Err(DatabaseError::message("this book is only stored on this device; upload it before removing the download"));
        }
        Ok(true)
    }

    pub fn evict_local_book(&self, hash: &BookId) -> Result<bool, DatabaseError> {
        self.write(|transaction| {
            if !self.check_local_book_eviction(hash)? {
                return Ok(false);
            }
            transaction.shared_transfer_complete_download(hash)?;
            transaction.asset_clear_downloaded_placements(&(&hash))?;
            transaction.asset_clear_book_paths(&(&hash))?;
            #[cfg(not(target_arch = "wasm32"))]
            transaction.shared_cancel_book_materialization(hash)?;
            Ok(true)
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn complete_local_book_request_on(connection: &rusqlite::Connection, hash: &BookId) -> Result<(), DatabaseError> {
        let placements = query_rows(|read| connection.shared_live_book_placements(hash, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for (dir_id, _) in placements {
            connection.transfer_queue_materialization(hash, &dir_id)?;
        }
        connection.shared_transfer_complete_download(hash)?;
        Ok(())
    }

    pub fn sync_request_book_downloads(&self, hashes: &[BookId], origin: library_model::TransferOrigin) -> Result<(), DatabaseError> {
        let requested_at = crate::sync::unix_millis()?;
        self.write(|transaction| {
            for hash in hashes {
                if !transaction.transfer_download_placement(hash, |row| row.get::<_, bool>(0))? {
                    return Err(DatabaseError::message("book has no live placement"));
                }
                transaction.shared_request_book_download(hash, origin.storage(), requested_at)?;
            }
            Ok(())
        })
    }

    pub fn transfer_download_state(&self, hash: &BookId, bytes_present: bool) -> Result<DownloadState, DatabaseError> {
        Ok(match (self.download_flags(hash)?, bytes_present) {
            ((true, _), true) => DownloadState::Downloaded,
            ((_, true), _) => DownloadState::Queued,
            _ => DownloadState::NotDownloaded,
        })
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn complete_stored_book_download(hash: BookId, checksum: ContentHash, size_bytes: u64, thumbnail_set_exists: bool) -> ();
    arguments (BookId, ContentHash, u64, bool,) = (hash, checksum, size_bytes, thumbnail_set_exists,);
    execute database, arguments => database.complete_stored_book_download(arguments.0, arguments.1, arguments.2, arguments.3);

    fn is_book_downloaded(hash: &BookId) -> bool;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.is_book_downloaded(&arguments.0);

    fn check_local_book_eviction(hash: &BookId) -> bool;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.check_local_book_eviction(&arguments.0);

    fn evict_local_book(hash: &BookId) -> bool;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.evict_local_book(&arguments.0);

    fn sync_request_book_downloads(hashes: &[BookId], origin: library_model::TransferOrigin) -> ();
    arguments (Vec<BookId>, library_model::TransferOrigin,) = (hashes.to_vec(), origin,);
    execute database, arguments => database.sync_request_book_downloads(&arguments.0, arguments.1);

    fn transfer_download_state(hash: &BookId, bytes_present: bool) -> DownloadState;
    arguments (BookId, bool,) = (hash.clone(), bytes_present,);
    execute database, arguments => database.transfer_download_state(&arguments.0, arguments.1);
}

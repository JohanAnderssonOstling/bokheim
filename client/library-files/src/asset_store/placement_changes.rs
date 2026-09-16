//! Placement mutations coupled to physical storage reconciliation.
//! Callers own the transaction and publish notifications only after commit.
use super::{AssetStore, AssetStoreError};
use crate::{ContentHash, DirId};

impl AssetStore {
    pub fn transfer_book_placement(&self, transaction: &rusqlite::Transaction<'_>, hash: &ContentHash, source: &DirId, destination: &DirId, moving: bool) -> rusqlite::Result<String> {
        #[cfg(not(target_arch = "wasm32"))]
        let old_path = {
            let name = client_library::placements::live_file_name(transaction, source, hash)?;
            client_library::file_work::placement_path(transaction, source, &name)?
        };
        let name = client_library::placements::transfer_book_placement_record(transaction, hash, source, destination, moving)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Publish the destination before retiring the source. Only this
            // book's paths need work; no whole-library projection is needed.
            client_library::file_work::queue_file_work(transaction, "restore", hash, &client_library::file_work::placement_path(transaction, destination, &name)?)?;
            if moving && source != destination {
                client_library::file_work::queue_file_work(transaction, "trash", hash, &old_path)?;
            }
        }
        Ok(name)
    }
    pub fn remove_book_placement(&self, transaction: &rusqlite::Transaction<'_>, content_hash: &ContentHash, source_id: &DirId) -> rusqlite::Result<bool> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let name = client_library::placements::live_file_name(transaction, source_id, content_hash)?;
            client_library::file_work::queue_file_work(transaction, "trash", content_hash, &client_library::file_work::placement_path(transaction, source_id, &name)?)?;
        }
        client_library::placements::remove_book_placement_record(transaction, content_hash, source_id)
    }
    pub fn ensure_restore_available(&self) -> Result<(), AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.placement().root_available() {
        return Err(AssetStoreError("library storage is unavailable".into()));
        }
        Ok(())
    }
    pub fn restore_book(&self, transaction: &rusqlite::Transaction<'_>, content_hash: &ContentHash) -> rusqlite::Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        let available = |path: &str| self.placement().directory_available(path);
        // Browser directories have no separate filesystem placement.
        #[cfg(target_arch = "wasm32")]
        let available = |_: &str| true;
        client_library::placements::restore_book_record_with_parent(transaction, content_hash, available)?;
        #[cfg(target_arch = "wasm32")]
        self.restore_opfs_availability(transaction, content_hash)?;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    fn restore_opfs_availability(&self, transaction: &rusqlite::Transaction<'_>, hash: &ContentHash) -> rusqlite::Result<()> {
        // The restore already queued a download. Retained OPFS bytes satisfy
        // it immediately, even while signed out; missing bytes keep the request.
        let available = self.exists(crate::BlobKind::Book, hash).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        client_library::placements::set_book_hash_downloaded(transaction, *hash, available)?;
        if available {
            client_library::assets::complete_asset_request(transaction, crate::BlobKind::Book, hash)?;
            if !self.thumbnail_set_exists(hash).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))? {
                client_library::thumbnail_jobs::request_missing_thumbnail(transaction, hash)?;
            }
        }
        Ok(())
    }
    pub fn restore_directory(&self, transaction: &rusqlite::Transaction<'_>, directory_id: &DirId, requested_parent: Option<&DirId>) -> rusqlite::Result<String> {
        #[cfg(target_arch = "wasm32")]
        let hashes = transaction
            .prepare(include_str!("sql/placement_changes/restore_directory_select.sql"))?
            .query_map([directory_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let name = client_library::placements::restore_directory_record(transaction, directory_id, requested_parent)?;
        #[cfg(target_arch = "wasm32")]
        for hash in hashes {
            self.restore_opfs_availability(transaction, &ContentHash::new(&hash))?;
        }
        Ok(name)
    }
}

impl AssetStore {
    /// A hash-addressed store can satisfy another placement from retained bytes.
    /// Native stores must materialize the requested filesystem placement instead.
    pub fn complete_retained_download(&self, conn: &rusqlite::Connection, hash: &ContentHash, placement: &client_library::BookPlacement) -> Result<bool, AssetStoreError> {
        #[cfg(target_arch = "wasm32")]
        if self.exists(crate::BlobKind::Book, hash)? && client_library::assets::book_placement_expects_hash(conn, placement, hash)? {
            client_library::placements::set_book_hash_downloaded(conn, *hash, true)?;
            client_library::assets::complete_asset_request(conn, crate::BlobKind::Book, hash)?;
            return Ok(true);
        }
        let _ = (conn, hash, placement);
        Ok(false)
    }
}

impl AssetStore {
    pub fn evict_local_book(&self, conn: &mut rusqlite::Connection, hash: &ContentHash) -> Result<(), AssetStoreError> {
        if !client_library::assets::is_book_downloaded(conn, *hash)? { return Ok(()); }
        #[cfg(not(target_arch = "wasm32"))]
        self.evict_publication_files(conn, hash)?;
        let tx = conn.transaction()?;
        client_library::assets::complete_asset_request(&tx, crate::BlobKind::Book, hash)?;
        client_library::placements::set_book_hash_downloaded(&tx, *hash, false)?;
        #[cfg(not(target_arch = "wasm32"))]
        client_library::file_work::cancel_book_restore(&tx, hash)?;
        tx.commit()?;
        self.remove(crate::BlobKind::Book, hash).map(|_| ())
    }
    pub fn placement_work(&self, conn: &rusqlite::Connection) -> rusqlite::Result<(bool, Option<String>)> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = client_library::file_work::has_pending_file_work(conn)?;
            let error = client_library::sync_state::get_sync_metadata(conn, "local_file_work_error")?.filter(|error| !error.is_empty());
            Ok((pending, error))
        }
        #[cfg(target_arch = "wasm32")]
        { let _ = conn; Ok((false, None)) }
    }
}

//! User-initiated library organization backed by the shared SQLite model.

use super::{database as db, filenames, LibraryBackend};
use crate::{BlobKind, ContentHash, DirId};
use library_model::{LibraryFolderDestination, LibraryTrashView};

impl LibraryBackend {
    pub(super) fn rename_library_directory(&self, directory_id: &DirId, requested_name: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.move_library_directory(directory_id, None, Some(requested_name))
    }

    pub(super) fn move_library_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
        let requested_name = requested_name.map(filenames::validate_component).transpose()?;
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        let chosen = db::move_directory_record(&transaction, directory_id, requested_parent, requested_name.as_ref().map(|name| name.as_str()))?;
        transaction.commit()?;
        Ok(chosen)
    }

    pub(super) fn copy_book_placement(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, Box<dyn std::error::Error>> {
        self.transfer_book_placement(content_hash, source_id, destination_id, false)
    }

    pub(super) fn move_book_placement(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, Box<dyn std::error::Error>> {
        self.transfer_book_placement(content_hash, source_id, destination_id, true)
    }

    fn transfer_book_placement(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId, remove_source: bool) -> Result<String, Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        let name = db::transfer_book_placement_record(&transaction, content_hash, source_id, destination_id, remove_source)?;
        transaction.commit()?;
        Ok(name)
    }

    pub(super) fn remove_book_placement(&self, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        let orphaned = db::remove_book_placement_record(&transaction, content_hash, source_id)?;
        transaction.commit()?;
        Ok(orphaned)
    }

    pub(super) fn trash_book_record(&self, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        db::trash_book_record(&conn, content_hash)?;
        Ok(())
    }

    pub(super) fn restore_book_record(&self, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        db::restore_book_record(&conn, content_hash)?;
        Ok(())
    }

    pub(super) fn load_trash(&self) -> Result<LibraryTrashView, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        Ok(db::library_trash(&conn)?)
    }

    pub(super) fn load_folder_destinations(&self) -> Result<Vec<LibraryFolderDestination>, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        Ok(db::folder_destinations(&conn)?)
    }

    pub(super) fn purge_book_record(&self, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        db::purge_book_record(&transaction, content_hash)?;
        db::remove_local_blob(&transaction, BlobKind::Book, content_hash)?;
        db::remove_local_blob(&transaction, BlobKind::Thumbnail, content_hash)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn purge_directory_record(&self, directory_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        db::purge_directory_record(&transaction, directory_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn empty_trash_records(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        let purged_books = db::empty_trash_records(&transaction)?;
        for content_hash in purged_books {
            db::remove_local_blob(&transaction, BlobKind::Book, &content_hash)?;
            db::remove_local_blob(&transaction, BlobKind::Thumbnail, &content_hash)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn trash_directory_record(&self, directory_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        db::trash_directory_record(&transaction, directory_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn restore_directory_record(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let transaction = conn.transaction()?;
        let name = db::restore_directory_record(&transaction, directory_id, requested_parent)?;
        transaction.commit()?;
        Ok(name)
    }
}

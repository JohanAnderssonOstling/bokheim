//! User-initiated library organization backed by the shared SQLite model.
use crate::BackendError;

use super::{LibrarySession, filenames};
use crate::{ContentHash, DirId};
use library_model::{LibraryFolderDestination, LibraryTrashView};

impl LibrarySession {
    fn commit_placement_change<T>(&self, change: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>) -> Result<T, BackendError> {
        let mut conn = self.db.get()?;
        // Reserve the main-database writer before reading placement state. A
        // deferred read snapshot cannot safely upgrade after another writer commits.
        let transaction = client_library::schema::library_write_transaction(&mut conn)?;
        let result = change(&transaction)?;
        transaction.commit()?;
        drop(conn);
        self.notify_contents_changed();
        Ok(result)
    }

    pub fn rename_directory(&self, directory_id: &DirId, requested_name: &str) -> Result<String, BackendError> {
        self.move_library_directory(directory_id, None, Some(requested_name))
    }

    pub fn move_directory(&self, directory_id: &DirId, parent_id: &DirId) -> Result<String, BackendError> {
        self.move_library_directory(directory_id, Some(parent_id), None)
    }

    /// Duplicates a folder and its contents under another folder. The books it
    /// holds gain a second placement; their bytes are not duplicated. Returns
    /// the copy's directory ID, which is what an undo of this acts on.
    pub fn copy_directory(&self, directory_id: &DirId, destination_parent_id: &DirId) -> Result<DirId, BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::copy_directory_record(transaction, directory_id, destination_parent_id))
    }

    /// Puts a book back in a folder it was removed from.
    pub fn restore_book_placement(&self, content_hash: &ContentHash, directory_id: &DirId) -> Result<(), BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::restore_book_placement_record(transaction, content_hash, directory_id))
    }

    fn move_library_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, BackendError> {
        let requested_name = requested_name.map(filenames::validate_component).transpose()?;
        self.commit_placement_change(|transaction| client_library::placements::move_directory_record(transaction, directory_id, requested_parent, requested_name.as_deref()))
    }

    pub fn copy_book_to_directory(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, BackendError> {
        self.transfer_book_placement(content_hash, source_id, destination_id, false)
    }

    pub fn move_book_to_directory(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, BackendError> {
        self.transfer_book_placement(content_hash, source_id, destination_id, true)
    }

    fn transfer_book_placement(&self, hash: &ContentHash, source: &DirId, destination: &DirId, moving: bool) -> Result<String, BackendError> {
        self.commit_placement_change(|transaction| self.assets.transfer_book_placement(transaction, hash, source, destination, moving))
    }

    /// Removes one placement. Returns `true` when the final placement
    /// is removed and the book consequently moves to Trash.
    pub fn remove_book_from_directory(&self, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, BackendError> {
        self.commit_placement_change(|transaction| self.assets.remove_book_placement(transaction, content_hash, source_id))
    }

    pub fn move_book_to_trash(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::trash_book_record(transaction, content_hash))
    }

    pub fn restore_book(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.assets.ensure_restore_available()?;
        self.commit_placement_change(|transaction| self.assets.restore_book(transaction, content_hash))
    }

    pub(crate) fn trash(&self) -> Result<LibraryTrashView, BackendError> {
        let conn = self.db.get()?;
        Ok(client_library::placements::library_trash(&conn)?)
    }

    pub(crate) fn folder_destinations(&self) -> Result<Vec<LibraryFolderDestination>, BackendError> {
        let conn = self.db.get()?;
        Ok(client_library::placements::folder_destinations(&conn)?)
    }

    pub fn purge_book(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::purge_book_record(transaction, content_hash))
    }

    pub fn purge_directory(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::purge_directory_record(transaction, directory_id))
    }

    pub fn empty_trash(&self) -> Result<(), BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::empty_trash_records(transaction))
    }

    pub fn move_directory_to_trash(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.commit_placement_change(|transaction| client_library::placements::trash_directory_record(transaction, directory_id))
    }

    pub fn restore_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, BackendError> {
        self.commit_placement_change(|transaction| self.assets.restore_directory(transaction, directory_id, requested_parent))
    }
}

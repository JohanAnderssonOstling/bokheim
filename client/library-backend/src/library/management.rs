//! User-initiated library organization backed by the shared SQLite model.
use crate::BackendError;

use super::{filenames, LibrarySession};
use crate::{ContentHash, DirId};

impl LibrarySession {
    pub async fn rename_directory(&self, directory_id: &DirId, requested_name: &str) -> Result<String, BackendError> {
        self.move_library_directory(directory_id, None, Some(requested_name)).await
    }

    pub async fn move_directory(&self, directory_id: &DirId, parent_id: &DirId) -> Result<String, BackendError> {
        self.move_library_directory(directory_id, Some(parent_id), None).await
    }

    /// Duplicates a folder and its contents under another folder. The books it
    /// holds gain a second placement; their bytes are not duplicated. Returns
    /// the copy's directory ID, which is what an undo of this acts on.
    pub fn copy_directory(&self, directory_id: &DirId, destination_parent_id: &DirId) -> Result<DirId, BackendError> {
        let copy = self.db.copy_directory(directory_id, destination_parent_id)?;
        self.notify_contents_changed();
        Ok(copy)
    }

    /// Puts a book back in a folder it was removed from.
    pub fn restore_book_placement(&self, content_hash: &ContentHash, directory_id: &DirId) -> Result<(), BackendError> {
        self.db.restore_book_placement(content_hash, directory_id)?;
        self.notify_contents_changed();
        Ok(())
    }

    async fn move_library_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, BackendError> {
        let name = self.queue_directory_move(directory_id, requested_parent, requested_name)?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = crate::asset_workflow::prepare_directory(&self.assets, &self.db, *directory_id).await {
            log::warn!("directory rename will retry from the durable queue: {error}");
        }
        Ok(name)
    }

    /// The logical mutation is ordered; physical replay is durable background
    /// work and must not hold up later commands or reader events.
    pub(super) fn queue_directory_move(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, BackendError> {
        let requested_name = requested_name.map(filenames::validate_component).transpose()?;
        let name = self.db.move_directory(directory_id, requested_parent, requested_name.as_deref())?;
        self.notify_contents_changed();
        Ok(name)
    }

    pub(super) fn transfer_book_placement(&self, hash: &ContentHash, source: &DirId, destination: &DirId, moving: bool) -> Result<String, BackendError> {
        let name = self.db.transfer_book_placement(hash, source, destination, moving)?;
        self.notify_contents_changed();
        Ok(name)
    }

    /// Removes one placement. Returns `true` when the final placement
    /// is removed and the book consequently moves to Trash.
    pub fn remove_book_from_directory(&self, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, BackendError> {
        let orphaned = self.db.remove_book_placement(content_hash, source_id)?;
        self.notify_contents_changed();
        Ok(orphaned)
    }

    pub fn move_book_to_trash(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.db.trash_book(content_hash)?;
        self.notify_contents_changed();
        Ok(())
    }

    pub fn restore_book(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.db.restore_book(content_hash)?;
        self.notify_contents_changed();
        Ok(())
    }

    pub fn purge_book(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.db.purge_book(content_hash)?;
        self.notify_contents_changed();
        Ok(())
    }

    pub fn purge_directory(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.db.purge_directory(directory_id)?;
        self.notify_contents_changed();
        Ok(())
    }

    pub fn empty_trash(&self) -> Result<(), BackendError> {
        self.db.empty_trash()?;
        self.notify_contents_changed();
        Ok(())
    }

    pub fn move_directory_to_trash(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.db.trash_directory(directory_id)?;
        self.notify_contents_changed();
        Ok(())
    }

    pub fn restore_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, BackendError> {
        let name = self.db.restore_directory(directory_id, requested_parent)?;
        self.notify_contents_changed();
        Ok(name)
    }
}

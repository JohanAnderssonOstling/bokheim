//! User-initiated library organization backed by the shared SQLite model.
use client_protocol::BackendError;
#[cfg(not(target_arch = "wasm32"))]
use library_database::{NativePlacementEdit, NativePlacementResult};
use library_model::Dir;
use library_replica::filenames;
use sync_common::{BookId, DirId};

use crate::LibraryState;

impl LibraryState {
    pub async fn copy_directory(&self, source: &DirId, destination: &DirId) -> Result<DirId, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.edit_native(NativePlacementEdit::CopyDirectory { source: *source, destination: *destination }).await? {
                NativePlacementResult::CopiedDirectory(id) => Ok(id),
                _ => unreachable!(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let copy = self.database().copy_directory(source, destination).await?;
            self.events.notify_contents_changed();
            Ok(copy)
        }
    }

    pub async fn copy_book_to_directory(&self, book: &BookId, source: Option<&DirId>, destination: &DirId) -> Result<bool, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.edit_native(NativePlacementEdit::CopyBook { book: *book, source: source.copied(), destination: *destination }).await? {
                NativePlacementResult::CopiedBook(created) => Ok(created),
                _ => unreachable!(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let created = self.database().copy_book_to_directory(book, source, destination).await?;
            if created {
                self.events.notify_contents_changed();
            }
            Ok(created)
        }
    }

    pub async fn restore_book_placement(&self, book: &BookId, destination: &DirId) -> Result<(), BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.edit_native(NativePlacementEdit::RestoreBookPlacement { book: *book, destination: *destination }).await?;
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.database().restore_book_placement(book, destination).await?;
            self.events.notify_contents_changed();
            Ok(())
        }
    }

    pub async fn move_book_placement(&self, book: &BookId, source: &DirId, destination: &DirId) -> Result<bool, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.edit_native(NativePlacementEdit::TransferBook { book: *book, source: *source, destination: *destination, moving: true }).await? {
                NativePlacementResult::TransferredBook { created, .. } => Ok(created),
                _ => unreachable!(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let created = self.database().move_book_placement(book, source, destination).await?;
            self.events.notify_contents_changed();
            Ok(created)
        }
    }

    pub async fn queue_directory_move(&self, directory: &DirId, parent: Option<&DirId>, name: Option<&str>) -> Result<String, BackendError> {
        let name = name.map(filenames::validate_component).transpose()?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.edit_native(NativePlacementEdit::MoveDirectory { directory: *directory, parent: parent.copied(), name }).await? {
                NativePlacementResult::MovedDirectory(name) => Ok(name),
                _ => unreachable!(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let name = self.database().move_directory(directory, parent, name.as_deref()).await?;
            self.events.notify_contents_changed();
            Ok(name)
        }
    }

    pub async fn create_directory(&self, parent_id: &DirId, name: &str) -> Result<Dir, BackendError> {
        let name = filenames::validate_component(name.trim())?;
        #[cfg(not(target_arch = "wasm32"))]
        let created = {
            let id = DirId::new_v4();
            let (_, path) = self.database().directory_creation_path(parent_id, &name).await?;
            let chosen = self.assets().create_directory(&path, id).map_err(BackendError::operation)?;
            let parent = path.rsplit_once('/').map(|(parent, _)| parent);
            let path = parent.map_or_else(|| chosen.clone(), |parent| format!("{parent}/{chosen}"));
            self.database().register_created_directory(&id, parent_id, &chosen, &path).await?
        };
        #[cfg(target_arch = "wasm32")]
        let created = self.database().create_directory(parent_id, &name).await?;
        self.events.notify_contents_changed();
        Ok(created)
    }

    /// Removes one placement. Returns `true` when the final placement
    /// is removed and the book consequently moves to Trash.
    pub async fn remove_book_from_directory(&self, book_id: &BookId, source_id: &DirId) -> Result<bool, BackendError> {
        let orphaned = self.database().remove_book_placement(book_id, source_id).await?;
        self.events.notify_contents_changed();
        Ok(orphaned)
    }

    pub async fn move_book_to_trash(&self, book_id: &BookId) -> Result<(), BackendError> {
        self.database().trash_book(book_id).await?;
        self.events.notify_contents_changed();
        Ok(())
    }

    pub async fn restore_book(&self, book_id: &BookId) -> Result<(), BackendError> {
        self.database().restore_book(book_id).await?;
        self.events.notify_contents_changed();
        Ok(())
    }

    pub async fn purge_book(&self, book_id: &BookId) -> Result<(), BackendError> {
        self.database().purge_book(book_id).await?;
        self.events.notify_contents_changed();
        Ok(())
    }

    pub async fn purge_directory(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.database().purge_directory(directory_id).await?;
        self.events.notify_contents_changed();
        Ok(())
    }

    pub async fn empty_trash(&self) -> Result<(), BackendError> {
        self.database().empty_trash().await?;
        self.events.notify_contents_changed();
        Ok(())
    }

    pub async fn move_directory_to_trash(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.database().trash_directory(directory_id).await?;
        self.events.notify_contents_changed();
        Ok(())
    }

    pub async fn restore_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, BackendError> {
        let name = self.database().restore_directory(directory_id, requested_parent).await?;
        self.events.notify_contents_changed();
        Ok(name)
    }
}

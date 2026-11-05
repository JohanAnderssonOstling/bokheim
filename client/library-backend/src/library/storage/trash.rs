//! Moving library contents to trash, restoring them, and permanent removal.

use crate::library::LibraryHandle;
use crate::{BackendError, BookId, DirId};

impl LibraryHandle {
    pub async fn trash(&self) -> Result<library_model::LibraryTrashView, BackendError> {
        let database = self.database();
        Ok(database.library_trash().await?)
    }

    pub async fn restore_book(&self, book_id: BookId) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        Ok(storage.restore_book(&book_id).await?)
    }

    pub async fn restore_directory(&self, directory_id: DirId, parent_id: Option<DirId>) -> Result<String, BackendError> {
        let storage = self.storage().await?;
        Ok(storage.restore_directory(&directory_id, parent_id.as_ref()).await?)
    }

    pub async fn purge_book(&self, book_id: BookId) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        Ok(storage.purge_book(&book_id).await?)
    }

    pub async fn purge_directory(&self, directory_id: DirId) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        Ok(storage.purge_directory(&directory_id).await?)
    }

    pub async fn empty_trash(&self) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        Ok(storage.empty_trash().await?)
    }

    pub async fn move_book_to_trash(&self, book_id: BookId) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        Ok(storage.move_book_to_trash(&book_id).await?)
    }

    pub async fn move_directory_to_trash(&self, directory_id: DirId) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        Ok(storage.move_directory_to_trash(&directory_id).await?)
    }
}

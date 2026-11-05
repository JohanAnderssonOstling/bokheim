//! Directory changes and book placements within one library.

use crate::library::LibraryHandle;
use crate::{BackendError, BookId, DirId};

impl LibraryHandle {
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_directory_with_id(&self, parent_id: DirId, name: String) -> Result<DirId, BackendError> {
        let storage = self.storage().await?;
        Ok(storage.create_directory(&parent_id, &name).await?.id)
    }

    pub async fn create_directory(&self, parent_id: DirId, name: String) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        storage.create_directory(&parent_id, &name).await.map(|_| ())
    }

    pub async fn rename_directory(&self, directory_id: DirId, name: String) -> Result<String, BackendError> {
        let storage = self.storage().await?;
        storage.queue_directory_move(&directory_id, None, Some(&name)).await
    }

    pub async fn move_directory(&self, directory_id: DirId, parent_id: DirId) -> Result<String, BackendError> {
        let storage = self.storage().await?;
        storage.queue_directory_move(&directory_id, Some(&parent_id), None).await
    }

    pub async fn copy_directory(&self, directory_id: DirId, parent_id: DirId) -> Result<DirId, BackendError> {
        let storage = self.storage().await?;
        storage.copy_directory(&directory_id, &parent_id).await
    }

    pub async fn restore_book_placement(&self, book_id: BookId, directory_id: DirId) -> Result<(), BackendError> {
        let storage = self.storage().await?;
        storage.restore_book_placement(&book_id, &directory_id).await
    }

    pub async fn copy_book_to_directory(&self, book_id: BookId, source_id: Option<DirId>, destination_id: DirId) -> Result<bool, BackendError> {
        let storage = self.storage().await?;
        storage.copy_book_to_directory(&book_id, source_id.as_ref(), &destination_id).await
    }

    pub async fn remove_book_from_directory(&self, book_id: BookId, source_id: DirId) -> Result<bool, BackendError> {
        let storage = self.storage().await?;
        storage.remove_book_from_directory(&book_id, &source_id).await
    }

    pub async fn move_book_placement(&self, book_id: BookId, source_id: DirId, destination_id: DirId) -> Result<bool, BackendError> {
        let storage = self.storage().await?;
        storage.move_book_placement(&book_id, &source_id, &destination_id).await
    }
}

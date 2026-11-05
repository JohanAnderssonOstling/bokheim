//! Copy planning, checkpoints, and transferred-selection cleanup.

use crate::library::LibraryHandle;
use crate::{BackendError, BookId, DirId};

impl LibraryHandle {
    pub async fn storage_copy_plan(&self) -> Result<library_database::transfer::TransferPlan, BackendError> {
        let database = self.database();
        Ok(database.storage_copy_plan().await?)
    }

    pub async fn book_storage_titles(&self, books: Vec<BookId>) -> Result<Vec<(BookId, String)>, BackendError> {
        let database = self.database();
        Ok(database.book_storage_titles(&books).await?)
    }

    pub async fn storage_copy_book_present(&self, book: BookId, parent_id: DirId) -> Result<bool, BackendError> {
        self.run_io(async {
            let storage = self.storage().await?;
            if storage.database().transfer_destination_name(&book, &parent_id).await?.is_none() {
                return Ok(false);
            }
            let paths = storage.database().book_paths(&book).await?;
            storage.assets().book_exists(&book, &paths).await.map_err(BackendError::operation)
        })
        .await
    }

    pub async fn ensure_storage_copy_directory(&self, id: DirId, parent_id: DirId, name: String) -> Result<(), BackendError> {
        let library = &self.owner.resources;
        let storage = self.storage().await?;
        let database = storage.database();
        if let Some(existing) = database.folder_destinations().await?.into_iter().find(|folder| folder.id == id) {
            if existing.parent_id != Some(parent_id) || existing.label != name {
                return Err(BackendError::message("copied folder was changed; keep the existing copy and choose another destination"));
            }
        } else {
            database.create_directory_with_id(&id, &parent_id, &name).await?;
            library.events.notify_contents_changed();
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn transfer_plan(&self, selection: library_database::transfer::TransferSelection) -> Result<library_database::transfer::TransferPlan, BackendError> {
        let database = self.database();
        Ok(database.transfer_plan(selection).await?)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn remove_transferred_selection(&self, plan: library_database::transfer::TransferPlan) -> Result<(), BackendError> {
        let library = &self.owner.resources;
        let storage = self.storage().await?;
        let database = storage.database();
        database.remove_transferred_selection(&plan).await?;
        library.events.notify_contents_changed();
        Ok(())
    }

    pub async fn refresh_storage_copy(&self) -> Result<(), BackendError> {
        self.run_io(async {
            let _files = self.storage().await?;
            let library = &self.owner.resources;
            if !library.sync.remote_ready().await.map_err(BackendError::operation)? {
                return Err(BackendError::message("library is not registered for this account"));
            }
            library.sync.synchronize_state_mode(false).await.map(|_| ()).map_err(BackendError::operation)
        })
        .await
    }
}

use super::LibraryClient;
use crate::library::{ImportBookSource, LibraryCommand, PendingCommand, RuntimeCommand, SessionCommand};
use crate::{DirId, DirectoryImport, ImportFailure};

impl LibraryClient {
    /// Explicit imports only; automatic filesystem scanning is excluded.
    pub async fn directory_import_progress(&self) -> Result<Option<library_model::DirectoryImportProgress>, String> {
        self.request(LibraryCommand::Runtime(RuntimeCommand::DirectoryImportProgress)).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn directory_import_progress_for(&self, activity: uuid::Uuid) -> Result<Option<library_model::DirectoryImportProgress>, String> {
        self.request(LibraryCommand::Runtime(RuntimeCommand::ImportProgress { activity })).await
    }

    /// Browser imports transfer ownership to the shared import coordinator.
    pub async fn import_directory_contents(&self, parent_id: DirId, directory: DirectoryImport) -> Result<Vec<ImportFailure>, String> {
        self.import_selected_directory(parent_id, directory, false, None).await
    }

    pub async fn import_book_bytes(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> Result<crate::ContentHash, String> {
        self.request(LibraryCommand::Session(SessionCommand::Pending(PendingCommand::ImportBook { parent_id, file_name, source: ImportBookSource::Bytes(bytes) }))).await
    }
    pub async fn import_directory(&self, parent_id: DirId, directory: DirectoryImport) -> Result<Vec<ImportFailure>, String> {
        self.import_selected_directory(parent_id, directory, true, None).await
    }

    pub async fn import_selected_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool, activity: Option<uuid::Uuid>) -> Result<Vec<ImportFailure>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.handle().map_err(|error| error.to_string())?.import_selected_directory(parent_id, directory, create_root, activity).await.map_err(|error| error.to_string())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = activity;
            self.transport.import_directory(parent_id, directory, create_root).await
        }
    }
}

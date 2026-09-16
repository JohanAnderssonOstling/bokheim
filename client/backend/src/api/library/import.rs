use super::LibraryClient;
use crate::runtime::library_requests;

impl LibraryClient {
    /// Explicit imports only; automatic filesystem scanning is excluded.
    pub async fn directory_import_progress(&self) -> Result<Option<crate::ScanProgress>, String> {
        self.request(library_requests::DirectoryImportProgress).await
    }

    /// Browser imports transfer ownership to the shared import coordinator.
    pub async fn import_directory_contents(&self, parent_id: crate::DirId, directory: crate::DirectoryImport) -> Result<Vec<crate::ImportFailure>, String> {
        self.import_selected_directory(parent_id, directory, false).await
    }

    pub async fn import_book_bytes(&self, parent_id: crate::DirId, file_name: String, bytes: Vec<u8>) -> Result<crate::ContentHash, String> {
        self.request(library_requests::ImportBookBytes { parent_id, file_name, bytes }).await
    }
    pub async fn import_directory(&self, parent_id: crate::DirId, directory: crate::DirectoryImport) -> Result<Vec<crate::ImportFailure>, String> {
        self.import_selected_directory(parent_id, directory, true).await
    }

    async fn import_selected_directory(&self, parent_id: crate::DirId, directory: crate::DirectoryImport, create_root: bool) -> Result<Vec<crate::ImportFailure>, String> {
        self.app.transport.import_selected_directory(self, parent_id, directory, create_root).await
    }
}

use super::LibraryClient;
use crate::library::{LibraryCommand, PendingCommand};
use crate::{BackendError, DirId, DirectoryImport, ImportFailure};

impl LibraryClient {
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn reader_import_summary(&self, source: crate::ReaderImportSource) -> Result<crate::ReaderLibrarySummary, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::ReaderImportSummary { source })).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_reader(&self, import_id: uuid::Uuid, source: crate::ReaderImportSource, options: crate::ReaderImportOptions) -> Result<crate::ReaderImportReport, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportReader { import_id, source, options })).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn reader_import_progress(&self, import_id: uuid::Uuid) -> Result<Option<crate::ReaderImportProgress>, BackendError> {
        self.request(LibraryCommand::Ready(crate::ReadyCommand::ReaderImportProgress { import_id })).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn cancel_reader_import(&self, import_id: uuid::Uuid) -> Result<bool, BackendError> {
        self.request(LibraryCommand::Ready(crate::ReadyCommand::CancelReaderImport { import_id })).await
    }

    /// Copy or move complete paths into the library, then import them in place.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_paths(&self, parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool) -> Result<Vec<ImportFailure>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportPaths { parent_id, paths, moving })).await
    }

    /// Imports a file the caller staged on local disk; the caller deletes it afterwards.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_stored_book(&self, parent_id: DirId, file_name: String, location: std::path::PathBuf) -> Result<crate::BookId, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportBook { parent_id, file_name, location })).await
    }

    /// Downloads a public-domain catalog book and imports it under `parent_id`.
    pub async fn add_catalog_book(&self, parent_id: DirId, book_id: crate::BookId) -> Result<crate::BookId, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::AddCatalogBook { parent_id, book_id })).await
    }

    /// Downloads an external catalog audiobook from its source and imports it under `parent_id`.
    pub async fn add_external_catalog_audiobook(&self, parent_id: DirId, entry: library_model::CardKey) -> Result<crate::BookId, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::AddExternalCatalogAudiobook { parent_id, entry })).await
    }

    /// Imports a file downloaded elsewhere, such as from an OPDS feed.
    pub async fn import_downloaded_book(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> Result<crate::BookId, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportDownloadedBook { parent_id, file_name, bytes: serde_bytes::ByteBuf::from(bytes) })).await
    }

    /// Imports `directory` under `parent_id`, as a folder of its own when
    /// `create_root` is set and otherwise as loose books.
    pub async fn import_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<ImportFailure>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.handle().await?.import_selected_directory(parent_id, directory, create_root).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport.import_directory(parent_id, directory, create_root).await
        }
    }
}

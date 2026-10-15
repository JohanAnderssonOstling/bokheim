use super::LibraryClient;
use crate::library::{LibraryCommand, PendingCommand};
use crate::{DirId, DirectoryImport, ImportFailure};

impl LibraryClient {
    /// Copy or move complete paths into the library, then import them in place.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_paths(&self, parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool) -> Result<Vec<ImportFailure>, String> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportPaths { parent_id, paths, moving })).await
    }

    /// Imports a file the caller staged on local disk; the caller deletes it afterwards.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_stored_book(&self, parent_id: DirId, file_name: String, location: std::path::PathBuf) -> Result<crate::ContentHash, String> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportBook { parent_id, file_name, location })).await
    }

    /// Downloads a public-domain catalog book and imports it under `parent_id`.
    pub async fn add_catalog_book(&self, parent_id: DirId, content_hash: crate::ContentHash) -> Result<crate::ContentHash, String> {
        self.request(LibraryCommand::Pending(PendingCommand::AddCatalogBook { parent_id, content_hash })).await
    }

    /// Downloads an external catalog audiobook from its source and imports it under `parent_id`.
    pub async fn add_external_catalog_audiobook(&self, parent_id: DirId, entry: library_model::CardKey) -> Result<crate::ContentHash, String> {
        self.request(LibraryCommand::Pending(PendingCommand::AddExternalCatalogAudiobook { parent_id, entry })).await
    }

    /// Imports a file downloaded elsewhere, such as from an OPDS feed.
    pub async fn import_downloaded_book(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> Result<crate::ContentHash, String> {
        self.request(LibraryCommand::Pending(PendingCommand::ImportDownloadedBook { parent_id, file_name, bytes: serde_bytes::ByteBuf::from(bytes) })).await
    }

    /// Imports `directory` under `parent_id`, as a folder of its own when
    /// `create_root` is set and otherwise as loose books.
    pub async fn import_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<ImportFailure>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.handle().map_err(|error| error.to_string())?.import_selected_directory(parent_id, directory, create_root).await.map_err(|error| error.to_string())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.transport.import_directory(parent_id, directory, create_root).await
        }
    }
}

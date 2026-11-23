//! Library-owned staging for native browser downloads.

use crate::BackendError;
use crate::library::{LibraryHandle, LibraryResources};
use client_platform_runtime::filesystem as fs;
use client_protocol::Failure;
use library_placement::LibraryState;
use tempfile::TempDir;

impl LibraryResources {
    pub(in crate::library) fn prepare_browser_downloads(&self, storage: &LibraryState) -> Result<(TempDir, bool), BackendError> {
        let root = storage.assets().native_library_root();
        if !fs::is_dir(root) {
            return Err(BackendError::classified(Failure::LibraryFolderUnavailable, "this library's folder is unavailable"));
        }
        // Android WebView downloads require ordinary filesystem paths. Stage in
        // private device storage, then import into the retained document tree.
        let document_root = fs::is_document_path(root);
        let staging_root = if document_root { self.database.path().parent().ok_or_else(|| BackendError::message("library database has no parent directory"))? } else { root };
        let directory = tempfile::Builder::new().prefix(".bokheim-browser-").tempdir_in(staging_root).map_err(BackendError::operation)?;
        Ok((directory, !document_root))
    }
}

impl LibraryHandle {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn prepare_browser_downloads(&self) -> Result<(tempfile::TempDir, bool), BackendError> {
        let session = &self.owner.resources;
        let storage = self.storage().await?;
        session.prepare_browser_downloads(&storage)
    }
}

//! Library-owned staging for native browser downloads.

use crate::library::{LibraryHandle, LibraryResources};
use crate::BackendError;
use client_protocol::Failure;
use library_placement::LibraryState;
use tempfile::TempDir;

impl LibraryResources {
    pub(in crate::library) fn prepare_browser_downloads(&self, storage: &LibraryState) -> Result<TempDir, BackendError> {
        let root = storage.assets().native_library_root();
        if !root.is_dir() {
            return Err(BackendError::classified(Failure::LibraryFolderUnavailable, "this library's folder is unavailable"));
        }
        tempfile::Builder::new().prefix(".bokheim-browser-").tempdir_in(root).map_err(BackendError::operation)
    }
}

impl LibraryHandle {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn prepare_browser_downloads(&self) -> Result<tempfile::TempDir, BackendError> {
        let session = &self.owner.resources;
        let storage = self.storage().await?;
        session.prepare_browser_downloads(&storage)
    }
}

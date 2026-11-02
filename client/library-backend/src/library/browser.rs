//! Library-owned staging for native browser downloads.
use super::LibrarySession;
use crate::BackendError;
use client_runtime::Failure;
use tempfile::TempDir;

impl LibrarySession {
    pub(super) fn prepare_browser_downloads(&self) -> Result<TempDir, BackendError> {
        if self.is_opds() {
            return Err(BackendError::message("browser downloads require a local library"));
        }
        let root = self.assets.native_library_root();
        if !root.is_dir() {
            return Err(BackendError::classified(Failure::LibraryFolderUnavailable, "this library's folder is unavailable"));
        }
        tempfile::Builder::new().prefix(".bokheim-browser-").tempdir_in(root).map_err(BackendError::operation)
    }
}

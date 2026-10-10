//! The import activity bracket around `library_import`: begins and ends
//! the import announcement and holds back contents notifications meanwhile.

use super::LibrarySession;

/// An import's lifetime stays with its backend job, including in-flight writes.
pub(crate) struct DirectoryImportActivity<'a> {
    pub(crate) library: &'a LibrarySession,
    pub(crate) id: uuid::Uuid,
}

impl Drop for DirectoryImportActivity<'_> {
    fn drop(&mut self) {
        self.library.finish_directory_import(self.id);
    }
}

impl LibrarySession {
    pub(crate) fn directory_import_activity(&self) -> DirectoryImportActivity<'_> {
        let id = uuid::Uuid::new_v4();
        self.begin_directory_import(id);
        DirectoryImportActivity { library: self, id }
    }
}

impl LibrarySession {
    /// The import itself runs in `library_import`; the session brackets it so
    /// contents notifications wait for its single completion announcement.
    pub(crate) async fn import_selected_directory(&self, parent_id: crate::DirId, directory: crate::DirectoryImport, create_root: bool, cancelled: &dyn Fn() -> bool) -> Result<Vec<crate::ImportFailure>, crate::BackendError> {
        if cancelled() {
            return Err(crate::BackendError::message("directory import cancelled"));
        }
        let _activity = self.directory_import_activity();
        self.import.import_selected_directory(parent_id, directory, create_root, cancelled).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn import_paths(&self, parent: crate::DirId, paths: Vec<std::path::PathBuf>, moving: bool) -> Result<Vec<crate::ImportFailure>, crate::BackendError> {
        let _activity = self.directory_import_activity();
        self.import.import_paths(parent, paths, moving).await
    }
}

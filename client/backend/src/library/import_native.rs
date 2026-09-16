//! Native file and granted-reader import entry points. Completion uses the shared workflow.
use super::LibrarySession;
use crate::BackendError;
use crate::{ContentHash, DirId};
use std::io::Read;

impl LibrarySession {
    pub(crate) async fn import_book_reader_direct(&self, parent_id: &DirId, source: Box<dyn Read + Send>, file_name: String) -> Result<ContentHash, BackendError> {
        let mut import = self.validate_import(parent_id, &file_name)?;
        let relative = self.prepare_import_path(*parent_id, import.file_name.clone()).await?;
        let occupied = {
            let conn = self.db.get()?;
            client_library::placements::occupied_file_names(&conn, &parent_id.to_string())?
        };
        let (name, copied) = self.assets.copy_publication(relative, source, occupied).await?;
        import.file_name = name;
        self.finish_import(import, copied).await
    }

    pub(super) async fn prepare_import_path(&self, parent: DirId, name: String) -> Result<String, BackendError> {
        self.assets.prepare_import_path(&self.db, parent, name).await.map_err(BackendError::operation)
    }

    pub async fn import_book_path(&self, parent_id: &DirId, file_name: String, path: &std::path::Path) -> Result<ContentHash, BackendError> {
        let import = self.validate_import(parent_id, &file_name)?;
        let relative = self.prepare_import_path(*parent_id, import.file_name.clone()).await?;
        let source = std::fs::File::open(path)?;
        let staged = self.assets.stage_publication(relative, Box::new(source)).await?;
        std::fs::remove_file(path)?;
        self.finish_import(import, staged).await
    }
}

/// Native import lifetime stays with its backend job, including in-flight writes.
pub(crate) struct DirectoryImportActivity<'a> {
    library: &'a LibrarySession,
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

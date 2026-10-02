//! Native file and granted-reader import entry points. Completion uses the shared workflow.

pub(crate) use super::LibrarySession;
use crate::asset_workflow::stage_import;
pub(crate) use crate::BackendError;
pub(crate) use crate::{ContentHash, DirId};
pub(crate) use std::io::Read;
use std::path::PathBuf;

impl LibrarySession {
    pub(crate) async fn import_book_reader_direct(&self, parent_id: &DirId, source: Box<dyn Read + Send>, file_name: String) -> Result<ContentHash, BackendError> {
        let mut import = self.validate_import(parent_id, &file_name)?;
        let options = crate::asset_store::ImportOptions { direct: true, progress: None };
        let (name, staged) = stage_import(&self.assets, &self.db, *parent_id, import.file_name.clone(), source, options).await.map_err(BackendError::operation)?;
        import.file_name = name;
        self.finish_import(import, staged).await
    }

    pub(crate) fn cleanup_import_storage(&self, _job: &str, _files: u32) -> Result<(), BackendError> {
        // Native stored sources are consumed individually during import, so
        // there is no indexed staging batch to retire after completion.
        Ok(())
    }

    pub(crate) async fn import_stored_book(&self, parent_id: DirId, file_name: String, location: PathBuf, staged: Option<super::commands::StagedImportMetadata>) -> Result<ContentHash, BackendError> {
        let import = self.validate_import(&parent_id, &file_name)?;
        let source = std::fs::File::open(&location)?;
        if let Some(staged) = staged {
            if source.metadata()?.len() != staged.length {
                return Err(BackendError::message("staged import length changed"));
            }
        }
        // Native ingestion computes identity from the file rather than relying
        // on the coordinator's optional staged identity.
        let (_, staged) = stage_import(&self.assets, &self.db, parent_id, import.file_name.clone(), Box::new(source), Default::default()).await.map_err(BackendError::operation)?;
        std::fs::remove_file(location)?;
        self.finish_import(import, staged).await
    }
}

/// Native import lifetime stays with its backend job, including in-flight writes.
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
        self.directory_import_activity_with_id(uuid::Uuid::new_v4())
    }

    pub(crate) fn directory_import_activity_with_id(&self, id: uuid::Uuid) -> DirectoryImportActivity<'_> {
        self.begin_directory_import(id);
        DirectoryImportActivity { library: self, id }
    }
}

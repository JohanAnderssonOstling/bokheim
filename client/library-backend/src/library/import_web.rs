//! Adopt browser-staged bytes before entering the shared import workflow.

use super::commands::StagedImportMetadata;
use super::LibrarySession;
use crate::{BackendError, ContentHash};
use std::path::PathBuf;
impl LibrarySession {
    pub(crate) fn cleanup_import_storage(&self, job: &str, files: u32) -> Result<(), BackendError> {
        let prefix = format!("__libraries/{}/imports/{job}/", self.id);
        client_platform_web::web_storage::cleanup_staged_import(&prefix, files).map_err(BackendError::operation)
    }

    pub(crate) async fn import_stored_book(&self, parent_id: crate::DirId, file_name: String, location: PathBuf, staged: Option<StagedImportMetadata>) -> Result<ContentHash, BackendError> {
        let validated = self.validate_import(&parent_id, &file_name)?;
        let prefix = format!("__libraries/{}/imports/", self.id);
        let location = location.to_str().ok_or_else(|| BackendError::message("stored browser import path is not valid UTF-8"))?;
        if !location.starts_with(&prefix) {
            return Err(BackendError::message("Staged book belongs to a different library"));
        }
        let staged = staged.ok_or_else(|| BackendError::message("stored browser import requires staging metadata"))?;
        let writer = client_platform_web::web_storage::adopt_staged(location.to_owned(), staged.length).await?;
        let staged = self.assets.finish_staged_write(writer, staged.hash.to_string()).map_err(BackendError::operation)?;
        self.finish_import(validated, staged).await
    }
}

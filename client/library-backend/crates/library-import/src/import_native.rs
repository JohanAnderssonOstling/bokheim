//! Imports of files already stored by a caller. Completion uses the shared workflow.

use client_runtime::BackendError;
use library_placement::stage_import;
use sync_common::{ContentHash, DirId};

use crate::LibraryImport;

impl LibraryImport {
    pub async fn import_transferred_book(&self, parent_id: DirId, file_name: String, reader: library_files::BoxedBookReader, state: library_database::transfer::BookTransferState) -> Result<bool, BackendError> {
        let format = state.addition.facts.format;
        let mut import = self.validate_import(&parent_id, &format!("book.{}", format.canonical_extension()))?;
        let file_name = self.database.transfer_destination_name(&state.content_hash, &parent_id).map_err(BackendError::operation)?.unwrap_or(file_name);
        import.file_name = library_replica::filenames::validate_component(&file_name)?;
        let (name, copied) = stage_import(&self.assets, &self.database, parent_id, import.file_name.clone(), reader, Default::default()).await.map_err(BackendError::operation)?;
        import.file_name = name;
        let staged = copied.verify_transfer(state.content_hash).await.map_err(BackendError::operation)?;
        let content_hash = staged.content_hash();
        let size_bytes = staged.size_bytes();
        crate::import::validate_import_destination(&self.database, &parent_id)?;
        let _lease = self.assets.lease_book(&content_hash).await.map_err(BackendError::operation)?;
        let published = self.publish_import(&import, staged).await?;
        let request_thumbnail = !self.assets.thumbnail_set_exists(&content_hash).map_err(BackendError::operation)?;
        let restore_paths = self.import_restore_paths(&content_hash)?;
        let created = self
            .database
            .commit_transfer_import(
                library_database::ImportCommit {
                    parent_id,
                    file_name: import.file_name,
                    format,
                    content_hash,
                    size_bytes,
                    inspection_version: book_inspection::inspection_version(format),
                    inspection: None,
                    published,
                    request_thumbnail,
                    restore_paths,
                },
                state,
            )
            .map_err(BackendError::operation)?;
        self.events.notify_contents_changed();
        Ok(created)
    }

    pub async fn import_stored_book(&self, parent_id: DirId, file_name: String, location: std::path::PathBuf) -> Result<ContentHash, BackendError> {
        let mut import = self.validate_import(&parent_id, &file_name)?;
        let source = std::fs::File::open(&location)?;
        let (name, staged) = stage_import(&self.assets, &self.database, parent_id, import.file_name.clone(), Box::new(source), Default::default()).await.map_err(BackendError::operation)?;
        import.file_name = name;
        self.finish_import(import, staged).await
    }
}

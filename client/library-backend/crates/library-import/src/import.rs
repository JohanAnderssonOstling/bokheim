//! The shared import path: validate the destination, prepare folders,
//! inspect the book, publish its bytes, and commit the addition.

use book_model::BookFormat;
use client_protocol::BackendError;
use library_database::DatabaseClient;
use library_files::AssetStore;
use library_files::asset_store::{CopiedBook, StagedBook};
use library_model::PublishedStagedBook;
use library_replica::filenames;
use sync_common::{BookId, DirId};

use crate::LibraryImport;

pub(crate) struct ValidatedImport {
    pub(crate) parent_id: DirId,
    pub(crate) file_name: String,
    pub(crate) format: BookFormat,
}

impl ValidatedImport {
    pub(crate) fn new(parent_id: DirId, file_name: &str) -> Result<Self, BackendError> {
        let file_name = filenames::validate_component(file_name)?;
        let format = BookFormat::from_file_name(&file_name).ok_or("only EPUB, PDF, DRM-free MOBI/AZW/AZW3, FB2, CBZ, and M4B files can be added to a library")?;
        Ok(Self { parent_id, file_name, format })
    }
}

pub(crate) async fn validate_import_destination(db: &DatabaseClient, parent_id: &DirId) -> Result<(), BackendError> {
    if !db.is_live_destination(parent_id).await.map_err(BackendError::operation)? {
        return Err(format!("library folder does not exist: {parent_id}").into());
    }
    Ok(())
}

impl LibraryImport<'_> {
    pub(crate) async fn validate_import(&self, parent_id: &DirId, file_name: &str) -> Result<ValidatedImport, BackendError> {
        let import = ValidatedImport::new(*parent_id, file_name)?;
        validate_import_destination(self.storage.database(), parent_id).await?;
        Ok(import)
    }

    pub(crate) async fn finish_import(&self, import: ValidatedImport, staged: CopiedBook) -> Result<BookId, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        let (staged, inspection, source_identity) = {
            let cached_pdf = if import.format == BookFormat::Pdf {
                match staged.copied_digest() {
                    Some((checksum, _, _)) => self.storage.database().cached_pdf_reader(BookId::from(checksum)).await.map_err(BackendError::operation)?,
                    None => None,
                }
            } else {
                None
            };
            let ingested = filesystem_scanner::ingestion::ingest_copied_file_with_pdf_reader(&staged, import.file_name.clone(), &*self.cpu, cached_pdf)
                .await
                .map_err(|error| BackendError::operation(error).context(format!("invalid {} book", import.format.canonical_extension().to_ascii_uppercase())))?;
            let inspection = Some(ingested.inspection);
            (staged.into_staged(ingested.book_id, ingested.size_bytes), inspection, ingested.source_identity)
        };
        #[cfg(target_arch = "wasm32")]
        let inspection = {
            let reuse = import.format != BookFormat::Pdf && self.storage.database().has_import_inspection(&staged.book_id(), import.format, book_inspection::inspection_version(import.format)).await.map_err(BackendError::operation)?;
            if reuse {
                None
            } else {
                let reader = staged.open_reader().await.map_err(BackendError::operation)?;
                Some(
                    cpu_host::submit_book(
                        &*self.cpu,
                        cpu_host::Inspect {
                            source_name: import.file_name.clone(),
                            format: import.format,
                            book_id: Some(staged.book_id()),
                            cached_pdf: if import.format == BookFormat::Pdf { self.storage.database().cached_pdf_reader(staged.book_id()).await.map_err(BackendError::operation)? } else { None },
                        },
                        cpu_host::as_cpu_reader(reader),
                    )
                    .await
                    .map_err(|error| error.context(format!("invalid {} book", import.format.canonical_extension().to_ascii_uppercase())))?,
                )
            }
        };

        let byte_checksum = staged.byte_checksum();
        #[cfg(target_arch = "wasm32")]
        let source_identity = None;
        let content = self.resolve_import_content(byte_checksum, staged.size_bytes(), source_identity).await?;
        let book_id = content.book_id;
        let staged = staged.with_local_identity(self.storage.assets(), book_id).await.map_err(BackendError::operation)?;
        let _lease = self.lease_import_content(&content).await?;
        // Inspection may overlap a delete/move. Do not publish into a folder
        // that became unavailable while CPU or source I/O was in flight.
        validate_import_destination(self.storage.database(), &import.parent_id).await?;

        // Book precedes the metadata commit. On a database failure the
        // library file remains discoverable by the next scan.
        let published = self.publish_import(self.storage.assets(), &import, staged).await?;
        self.commit_import(import, content, inspection, published).await?;
        Ok(book_id)
    }

    pub(crate) async fn publish_import(&self, assets: &AssetStore, import: &ValidatedImport, staged: StagedBook) -> Result<PublishedStagedBook, BackendError> {
        library_placement::publish_import(assets, self.storage.database(), import.parent_id, &import.file_name, staged, import.format == BookFormat::Mp3Folder).await.map_err(|error| error.context("could not publish the imported book"))
    }

    /// Placement rows whose remembered path disagrees with storage, resolved
    /// from a snapshot so no filesystem check holds the writer.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn import_restore_paths(&self, book_id: &BookId) -> Result<Vec<String>, BackendError> {
        let candidates = self.storage.database().import_restore_candidates(book_id).await.map_err(BackendError::operation)?;
        Ok(candidates.into_iter().filter(|(path, materialized)| materialized.as_deref() != Some(path.as_str()) || !self.storage.assets().placement_file_exists(path)).map(|(path, _)| path.to_string()).collect())
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn import_restore_paths(&self, _book_id: &BookId) -> Result<Vec<String>, BackendError> {
        Ok(Vec::new())
    }
}

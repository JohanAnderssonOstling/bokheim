//! Verified imports preserve reader state across storage identities.
use crate::LibraryImport;
#[cfg(not(target_arch = "wasm32"))]
use crate::completion::ImportContent;
use crate::import::{ValidatedImport, validate_import_destination};
#[cfg(not(target_arch = "wasm32"))]
use crate::{reader_batch::ReaderImportBatch, reader_transfer::reader_inspection_version};
use book_model::BookFormat;
use client_protocol::BackendError;
use library_database::transfer::{BookTransferState, ImportedBook};
#[cfg(not(target_arch = "wasm32"))]
use library_files::BoxedBookReader;
use library_files::asset_store::StagedBook;
#[cfg(not(target_arch = "wasm32"))]
use library_placement::stage_import;
use sync_common::DirId;

// A metadata-only placement may still be named "Untitled". Keep its known
// format recognizable when publishing it as a library file.
fn transfer_file_name(name: &str, format: BookFormat) -> Result<String, BackendError> {
    let name = library_replica::filenames::validate_component(name)?;
    if format == BookFormat::Mp3Folder || BookFormat::from_file_name(&name) == Some(format) {
        return Ok(name);
    }
    let requested = format!("{}.{}", name, format.canonical_extension());
    Ok(library_replica::filenames::validate_component(&requested).unwrap_or_else(|_| library_replica::numbered_file_name(&requested, 1)))
}

impl LibraryImport<'_> {
    /// The source length pins transfers whose stable identity has no checksum yet.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_transferred_book(&self, parent_id: DirId, file_name: String, reader: BoxedBookReader, size_bytes: u64, state: BookTransferState) -> Result<ImportedBook, BackendError> {
        self.import_transferred_book_inner(parent_id, file_name, reader, state, Some(size_bytes), false, None).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn import_transferred_book_inner(
        &self, parent_id: DirId, file_name: String, reader: BoxedBookReader, state: BookTransferState, expected_size: Option<u64>, inspected: bool, mut batch: Option<&mut ReaderImportBatch>,
    ) -> Result<ImportedBook, BackendError> {
        let format = state.addition.facts.format;
        let import_name = format!("book.{}", format.canonical_extension());
        let mut import = match batch.as_ref() {
            Some(batch) => batch.validate_import(self, parent_id, &import_name).await?,
            None => self.validate_import(&parent_id, &import_name).await?,
        };
        let file_name = self.storage.database().transfer_destination_name(&state.book_id, &parent_id).await.map_err(BackendError::operation)?.unwrap_or(file_name);
        import.file_name = transfer_file_name(&file_name, format)?;
        let (name, copied) = if let Some(batch) = batch.as_deref_mut() {
            batch.prepare_directory(self, parent_id).await?;
            let relative = batch.native_book_path(self, parent_id, &import.file_name).await?;
            batch.assets.copy_import(relative, reader, Vec::new(), Default::default()).await?
        } else {
            stage_import(self.storage.assets(), self.storage.database(), parent_id, &import.file_name, reader, Default::default()).await?
        };
        import.file_name = name;
        let checksum = state.expected_checksum();
        let staged = copied.verify_transfer_bytes(checksum, expected_size).await.map_err(BackendError::operation)?;
        Box::pin(self.finish_transferred_book(import, staged, state, inspected, batch)).await
    }

    /// Native reader adapters have already hashed the source and prepared its
    /// metadata. A local Copy batch defers only the final metadata transaction.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn import_verified_reader_state(&self, parent: DirId, name: String, reader: BoxedBookReader, state: BookTransferState, mut batch: Option<&mut ReaderImportBatch>) -> Result<ImportedBook, BackendError> {
        let import = match batch.as_ref() {
            Some(batch) => batch.validate_import(self, parent, &name).await?,
            None => self.validate_import(&parent, &name).await?,
        };
        if import.format != state.addition.facts.format {
            return Err(BackendError::message("reader format does not match its state"));
        }
        if let Some(imported) = self.reuse_transferred_book(&import, &state, batch.as_deref_mut()).await? {
            return Ok(imported);
        }
        self.import_transferred_book_inner(parent, name, reader, state, None, true, batch).await
    }

    /// Verify an existing placement before allocating or writing another copy.
    /// Fresh reader state still goes through the same atomic merge transaction.
    #[cfg(not(target_arch = "wasm32"))]
    async fn reuse_transferred_book(&self, import: &ValidatedImport, state: &BookTransferState, mut batch: Option<&mut ReaderImportBatch>) -> Result<Option<ImportedBook>, BackendError> {
        use client_platform_runtime::executor::run_blocking;
        use content_address::hash_file;
        use library_files::filesystem::fingerprint;
        use library_model::PublishedStagedBook;
        use std::fs::File;
        use std::path::PathBuf;

        if state.addition.facts.format == BookFormat::Mp3Folder {
            return Ok(None);
        }
        let Some(checksum) = state.expected_checksum() else { return Ok(None) };
        let identity = self.storage.database().resolve_transferred_content(state.book_id, checksum).await?;
        let Some(name) = self.storage.database().transfer_destination_name(&identity, &import.parent_id).await? else { return Ok(None) };
        let _lease = self.storage.assets().lease_book(&identity).await?;
        let destination = if let Some(batch) = batch.as_deref_mut() {
            batch.prepare_directory(self, import.parent_id).await?;
            self.prepared_import_destination(import.parent_id).await?
        } else {
            self.import_destination(import.parent_id).await?
        };
        let path = destination.join(&name);
        let known_revision = self.storage.database().local_content_revision(identity, import.parent_id, checksum).await?;
        let verified = run_blocking({
            let path = path.clone();
            move || -> Result<Option<(u64, u64)>, BackendError> {
                let metadata = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_file() => metadata,
                    Ok(_) => return Ok(None),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(error.into()),
                };
                let before = fingerprint(&path)?;

                if (known_revision.as_deref() != Some(&before.to_string()) && hash_file(&mut File::open(&path)?)? != checksum) || fingerprint(&path)? != before {
                    return Ok(None);
                }
                Ok(Some((before, metadata.len())))
            }
        })
        .await
        .map_err(BackendError::operation)??;
        let Some((revision, size_bytes)) = verified else { return Ok(None) };
        validate_import_destination(self.storage.database(), &import.parent_id).await?;
        let relative = self.storage.database().native_book_path(import.parent_id, &name).await?.ok_or("destination folder is missing")?;
        if PathBuf::from(&self.root).canonicalize()?.join(relative.as_str().trim_start_matches('/')) != path || fingerprint(&path)? != revision {
            return Err(BackendError::message("destination changed during transfer verification; retry the import"));
        }
        let content = ImportContent { book_id: identity, byte_checksum: Some(checksum), audiobook_content_hash: None, size_bytes, source_identity: None };
        let import = ValidatedImport { parent_id: import.parent_id, file_name: name.clone(), format: state.addition.facts.format };
        let published = PublishedStagedBook { name, relative_path: relative.to_string(), published: true, fingerprint: Some(revision.to_string()) };
        let version = reader_inspection_version(import.format);
        let commit = self.prepare_import_commit(import, content, published, None, version).await?;
        let state = state.clone().retarget(identity)?;
        if let Some(batch) = batch {
            batch.push(commit, state, _lease);
            return Ok(Some(ImportedBook { book_id: identity, created: false }));
        }
        self.commit_transferred_import(commit, state, true).await.map(Some)
    }

    pub(crate) async fn finish_transferred_book(
        &self, mut import: ValidatedImport, staged: StagedBook, mut state: BookTransferState, inspected: bool, #[cfg(not(target_arch = "wasm32"))] mut batch: Option<&mut ReaderImportBatch>,
    ) -> Result<ImportedBook, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        if inspected && batch.is_some() {
            if let Some(imported) = self.reuse_transferred_book(&import, &state, batch.as_deref_mut()).await? {
                return Ok(imported);
            }
        }
        let parent_id = import.parent_id;
        let format = state.addition.facts.format;
        let byte_checksum = staged.byte_checksum();
        state.byte_checksum = Some(byte_checksum);
        let mut content = if format == BookFormat::Mp3Folder {
            let checksum = self.staged_audiobook_content(&staged).await?;
            self.resolve_audiobook_import(checksum, Some(byte_checksum), staged.size_bytes(), None).await?
        } else {
            self.resolve_import_content(byte_checksum, staged.size_bytes(), None).await?
        };
        if state.book_id.is_local() && !state.book_id.is_library_scoped() {
            content.book_id = self.storage.database().resolve_transferred_content(state.book_id, byte_checksum).await?;
        }
        let book_id = content.book_id;
        #[cfg(not(target_arch = "wasm32"))]
        let assets = batch.as_ref().map_or(self.storage.assets(), |batch| &batch.assets);
        #[cfg(target_arch = "wasm32")]
        let assets = self.storage.assets();
        let staged = staged.with_local_identity(assets, book_id).await.map_err(BackendError::operation)?;
        let state = state.retarget(book_id).map_err(BackendError::operation)?;
        if let Some(name) = self.storage.database().transfer_destination_name(&book_id, &parent_id).await.map_err(BackendError::operation)? {
            import.file_name = name;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(batch) = &batch {
            import.file_name = match batch.destination_name(parent_id, book_id) {
                Some(name) => name.to_owned(),
                None if self.storage.database().transfer_destination_name(&book_id, &parent_id).await?.is_none() => batch.available_name(parent_id, &import.file_name, self.storage.database().shared_occupied_file_names(&parent_id).await?),
                None => import.file_name,
            };
        }
        let _lease = self.lease_import_content(&content).await?;
        #[cfg(not(target_arch = "wasm32"))]
        match batch.as_ref() {
            Some(batch) => batch.validate_destination(self, parent_id).await?,
            None => validate_import_destination(self.storage.database(), &parent_id).await?,
        }
        #[cfg(target_arch = "wasm32")]
        validate_import_destination(self.storage.database(), &parent_id).await?;
        #[cfg(not(target_arch = "wasm32"))]
        let published = if let Some(batch) = batch.as_ref() {
            let relative = batch.native_book_path(self, parent_id, &import.file_name).await?;
            assets.publish_import(staged, relative, self.storage.database().shared_occupied_file_names(&parent_id).await?, false).await?
        } else {
            self.publish_import(assets, &import, staged).await?
        };
        #[cfg(target_arch = "wasm32")]
        let published = self.publish_import(assets, &import, staged).await?;
        let inspection_version = book_inspection::inspection_version(format);
        #[cfg(not(target_arch = "wasm32"))]
        let inspection_version = if inspected { reader_inspection_version(format) } else { inspection_version };
        let commit = self.prepare_import_commit(import, content, published, None, inspection_version).await?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(batch) = batch {
            batch.push(commit, state, _lease);
            return Ok(ImportedBook { book_id, created: true });
        }
        self.commit_transferred_import(commit, state, inspected).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn import_transferred_staged_book(&self, parent_id: DirId, file_name: String, staged: library_files::asset_store::StagedBook, state: BookTransferState) -> Result<ImportedBook, BackendError> {
        if state.expected_checksum().is_some_and(|expected| staged.book_id().content_hash() != Some(expected)) {
            return Err(BackendError::message("transferred book failed content verification"));
        }
        let mut import = self.validate_import(&parent_id, &format!("book.{}", state.addition.facts.format.canonical_extension())).await?;
        import.file_name = transfer_file_name(&file_name, state.addition.facts.format)?;
        Box::pin(self.finish_transferred_book(import, staged, state, false)).await
    }
}

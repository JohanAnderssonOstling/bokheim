//! Host resources for copying verified book bytes between library actors.
use super::{LibraryCommand, LibraryHandle, LibrarySession, ReadyCommand};
use book_access::BoxedBookReader;
#[cfg(not(target_arch = "wasm32"))]
use book_access::{FetchFuture, RemoteFileError};
use client_runtime::BackendError;
use library_database::transfer::BookTransferState;
use serde::{Deserialize, Serialize};
use sync_common::BookId;
use sync_common::{DirId, ROOT_DIR_ID};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StorageCopyProgress {
    pub books: Vec<StorageCopiedBook>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StorageCopiedBook {
    pub source: BookId,
    pub directory: DirId,
    pub destination: BookId,
}

impl LibraryHandle {
    /// Host-local orchestration keeps readers inside the backend. The source
    /// receives read requests only, including when the destination fails.
    pub async fn copy_storage_from(&self, source: &LibraryHandle, mut progress: StorageCopyProgress, mut checkpoint: impl FnMut(&StorageCopyProgress) -> Result<(), BackendError>) -> Result<(), BackendError> {
        if self.id == source.id {
            return Err(BackendError::message("choose another library for the storage copy"));
        }
        let plan: library_database::transfer::TransferPlan = source.dispatch(LibraryCommand::Ready(ReadyCommand::StorageCopyPlan)).await?.take()?;
        let mapped = |id: DirId| if id == ROOT_DIR_ID { ROOT_DIR_ID } else { DirId::new_v5(&self.id, id.as_bytes()) };
        for folder in &plan.folders {
            self.dispatch(LibraryCommand::Ready(ReadyCommand::EnsureStorageCopyDirectory { id: mapped(folder.id), parent_id: mapped(folder.parent_id.ok_or("source folder has no parent")?), name: folder.label.clone() }))
                .await?
                .take::<()>()?;
        }
        for book in &plan.books {
            let parent = mapped(book.directory);
            if let Some(copied) = progress.books.iter().find(|copied| copied.source == book.content_hash && copied.directory == book.directory) {
                let present: bool = self.dispatch(LibraryCommand::Ready(ReadyCommand::StorageCopyBookPresent { book: copied.destination, parent_id: parent })).await?.take()?;
                if present {
                    continue;
                }
            }
            let resource = source.export_transfer_book(book.content_hash).await?;
            let copied = self.import_transfer_book(parent, book.file_name.clone(), resource).await?;
            progress.books.retain(|copied| copied.source != book.content_hash || copied.directory != book.directory);
            progress.books.push(StorageCopiedBook { source: book.content_hash, directory: book.directory, destination: copied.content_hash });
            checkpoint(&progress)?;
        }
        Ok(())
    }
}

pub struct TransferResource {
    #[cfg(not(target_arch = "wasm32"))]
    pub reader: BoxedBookReader,
    #[cfg(target_arch = "wasm32")]
    pub(crate) source: BrowserTransferSource,
    pub state: BookTransferState,
}
impl LibrarySession {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn export_transfer_book(&self, hash: BookId) -> Result<TransferResource, BackendError> {
        library_placement::run_file_jobs_for(&self.assets, &self.db, &hash).await.map_err(BackendError::operation)?;
        let mut state = self.db.book_transfer_state(hash)?;
        let paths = self.db.book_paths(&hash)?;
        if !hash.is_external() || self.db.is_book_downloaded(&hash)? {
            if let Some(book) = self.assets.verified_book_reader(hash, paths).await.map_err(BackendError::operation)? {
                if let Some(file) = &state.provider_file {
                    if self.db.provider_file(&hash)?.as_ref() != Some(file) || book.length != file.size_bytes {
                        return Err(BackendError::message("provider book changed while preparing the transfer"));
                    }
                }
                if state.byte_checksum.is_some_and(|expected| book.content_hash.content_hash() != Some(expected)) {
                    return Err(BackendError::message("local transfer bytes do not match the provider revision"));
                }
                state.byte_checksum = book.content_hash.content_hash();
                return Ok(TransferResource { reader: Box::new(book), state });
            }
        }
        // Read the complete original asset, including an MP3 folder's archive.
        let (source, prefix) = self.describe_remote_file(hash).await.map_err(super::remote_access::range_backend_error)?;
        let length = source.length;
        if let Some(file) = &state.provider_file {
            if source.revision.as_deref() != Some(file.revision.as_str()) || source.length != file.size_bytes {
                return Err(BackendError::message("provider book changed while preparing the transfer"));
            }
        }
        let access = self.remote_book_access(&source)?;
        let fetch = move |offset, length| {
            let access = access.clone();
            Box::pin(async move { access.read(offset, length).await }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
        };
        let reader = book_access::remote_file::native_reader(length, prefix, fetch).map_err(BackendError::operation)?;
        Ok(TransferResource { reader, state })
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) enum BrowserTransferSource {
    Local { reader: BoxedBookReader, length: u64 },
    Remote { access: super::remote_access::RemoteBookAccess, length: u64 },
}
impl LibrarySession {
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn export_transfer_book(&self, hash: BookId) -> Result<TransferResource, BackendError> {
        use std::io::{Seek, SeekFrom};
        let state = self.db.book_transfer_state(hash)?;
        let reader = if !hash.is_external() || self.db.is_book_downloaded(&hash)? { self.assets.open_book_reader(&hash, self.db.book_paths(&hash)?).await.map_err(BackendError::operation)? } else { None };
        if state.provider_file.is_some() && self.db.provider_file(&hash)? != state.provider_file {
            return Err(BackendError::message("provider book changed while preparing the copy"));
        }
        let source = if let Some(mut reader) = reader {
            let length = reader.seek(SeekFrom::End(0))?;
            reader.seek(SeekFrom::Start(0))?;
            BrowserTransferSource::Local { reader, length }
        } else {
            let (file, _) = self.describe_remote_file(hash).await.map_err(super::remote_access::range_backend_error)?;
            if state.provider_file.as_ref().is_some_and(|original| file.revision.as_deref() != Some(original.revision.as_str()) || original.size_bytes != file.length) {
                return Err(BackendError::message("provider book changed while preparing the copy"));
            }
            BrowserTransferSource::Remote { access: self.remote_book_access(&file)?, length: file.length }
        };
        let length = match &source {
            BrowserTransferSource::Local { length, .. } | BrowserTransferSource::Remote { length, .. } => *length,
        };
        if state.provider_file.as_ref().is_some_and(|file| file.size_bytes != length) {
            return Err(BackendError::message("provider book changed while preparing the copy"));
        }
        Ok(TransferResource { source, state })
    }

    pub(crate) async fn import_transfer_resource(&self, parent: sync_common::DirId, name: String, resource: TransferResource) -> Result<library_database::transfer::ImportedBook, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(self.import.import_transferred_book(parent, name, resource.reader, resource.state)).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            use library_files::AssetStoreError;
            use std::{
                cell::RefCell,
                io::{Read, Seek, SeekFrom},
                rc::Rc,
            };
            let expected = resource.state.expected_checksum();
            let length = match &resource.source {
                BrowserTransferSource::Local { length, .. } | BrowserTransferSource::Remote { length, .. } => *length,
            };
            let source = Rc::new(RefCell::new(resource.source));
            let staged = self
                .assets
                .stage_transfer_chunks(length, expected, move |offset, count| {
                    let source = source.clone();
                    async move {
                        let access = match &mut *source.borrow_mut() {
                            BrowserTransferSource::Remote { access, .. } => access.clone(),
                            BrowserTransferSource::Local { reader, .. } => {
                                reader.seek(SeekFrom::Start(offset))?;
                                let mut bytes = vec![0; count];
                                reader.read_exact(&mut bytes)?;
                                return Ok(bytes);
                            }
                        };
                        access.read(offset, count).await.map_err(|error| AssetStoreError::operation(super::remote_access::range_backend_error(error)))
                    }
                })
                .await
                .map_err(BackendError::operation)?;
            self.import.import_transferred_staged_book(parent, name, staged, resource.state).await
        }
    }
}

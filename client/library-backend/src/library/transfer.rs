//! Host resources for copying verified book bytes between library actors.
use super::{LibraryCommand, LibraryHandle, LibrarySession, ReadyCommand};
use book_access::BoxedBookReader;
use client_runtime::BackendError;
use library_database::transfer::{BookTransferState, TransferBook, TransferPlan};
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
        let plan: TransferPlan = source.dispatch(LibraryCommand::Ready(ReadyCommand::StorageCopyPlan)).await?.take()?;
        let mapped = |id: DirId| if id == ROOT_DIR_ID { ROOT_DIR_ID } else { DirId::new_v5(&self.id, id.as_bytes()) };
        // Check every missing book's readability and total storage before
        // importing any bytes. Quota and I/O failures during the actual copy
        // still leave the source untouched and its checkpoint resumable.
        let mut required_bytes = 0_u64;
        for book in &plan.books {
            if self.storage_copy_book_present(book, &progress, mapped(book.directory)).await? {
                continue;
            }
            let resource = source.export_transfer_book(book.content_hash).await?;
            required_bytes = required_bytes.checked_add(resource.length).ok_or_else(|| BackendError::message("library size exceeds available storage"))?;
        }
        self.dispatch(LibraryCommand::Ready(ReadyCommand::CheckStorageCopyCapacity { bytes: required_bytes })).await?.take::<()>()?;
        for folder in &plan.folders {
            self.dispatch(LibraryCommand::Ready(ReadyCommand::EnsureStorageCopyDirectory { id: mapped(folder.id), parent_id: mapped(folder.parent_id.ok_or("source folder has no parent")?), name: folder.label.clone() }))
                .await?
                .take::<()>()?;
        }
        for book in &plan.books {
            let parent = mapped(book.directory);
            if self.storage_copy_book_present(book, &progress, parent).await? {
                continue;
            }
            let resource = source.export_transfer_book(book.content_hash).await?;
            let copied = self.import_transfer_book(parent, book.file_name.clone(), resource).await?;
            progress.books.retain(|copied| copied.source != book.content_hash || copied.directory != book.directory);
            progress.books.push(StorageCopiedBook { source: book.content_hash, directory: book.directory, destination: copied.content_hash });
            checkpoint(&progress)?;
        }
        let current: TransferPlan = source.dispatch(LibraryCommand::Ready(ReadyCommand::StorageCopyPlan)).await?.take()?;
        if current != plan {
            return Err(BackendError::message("library changed during the copy; retry to include the latest books"));
        }
        Ok(())
    }

    async fn storage_copy_book_present(&self, book: &TransferBook, progress: &StorageCopyProgress, parent_id: DirId) -> Result<bool, BackendError> {
        let Some(copied) = progress.books.iter().find(|copied| copied.source == book.content_hash && copied.directory == book.directory) else { return Ok(false) };
        self.dispatch(LibraryCommand::Ready(ReadyCommand::StorageCopyBookPresent { book: copied.destination, parent_id })).await?.take()
    }
}

pub struct TransferResource {
    length: u64,
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

        if let Some(book) = self.assets.verified_book_reader_with_checksum(hash, paths, state.byte_checksum).await.map_err(BackendError::operation)? {
            if state.byte_checksum.is_some_and(|expected| book.content_hash.content_hash() != Some(expected)) {
                return Err(BackendError::message("local transfer bytes do not match the recorded checksum"));
            }
            state.byte_checksum = book.content_hash.content_hash();
            let length = book.length;
            return Ok(TransferResource { reader: Box::new(book), state, length });
        }

        // Read the complete original asset, including an MP3 folder's archive.
        let (source, prefix) = self.describe_remote_file(hash).await.map_err(super::remote_access::range_backend_error)?;
        let length = source.length;
        let access = self.remote_book_access(&source)?;
        let reader = book_access::remote_file::native_reader(length, prefix, access.fetch()).map_err(BackendError::operation)?;
        Ok(TransferResource { reader, state, length })
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) enum BrowserTransferSource {
    Local(BoxedBookReader),
    Remote(super::remote_access::RemoteBookAccess),
}
impl LibrarySession {
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn export_transfer_book(&self, hash: BookId) -> Result<TransferResource, BackendError> {
        use std::io::{Seek, SeekFrom};
        let state = self.db.book_transfer_state(hash)?;
        let reader = { self.assets.open_book_reader(&hash, self.db.book_paths(&hash)?).await.map_err(BackendError::operation)? };
        let (source, length) = if let Some(mut reader) = reader {
            let length = reader.seek(SeekFrom::End(0))?;
            reader.seek(SeekFrom::Start(0))?;
            (BrowserTransferSource::Local(reader), length)
        } else {
            let (file, _) = self.describe_remote_file(hash).await.map_err(super::remote_access::range_backend_error)?;
            (BrowserTransferSource::Remote(self.remote_book_access(&file)?), file.length)
        };
        Ok(TransferResource { source, state, length })
    }

    pub(crate) async fn import_transfer_resource(&self, parent: DirId, name: String, resource: TransferResource) -> Result<library_database::transfer::ImportedBook, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(self.import.import_transferred_book(parent, name, resource.reader, resource.state)).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            use library_files::AssetStoreError;
            use std::io::{Read, Seek, SeekFrom};
            let expected = resource.state.expected_checksum();
            let mut source = resource.source;
            let staged = self
                .assets
                .stage_transfer_chunks(resource.length, expected, async |offset, count| match &mut source {
                    BrowserTransferSource::Remote(access) => access.read(offset, count).await.map_err(|error| AssetStoreError::operation(super::remote_access::range_backend_error(error))),
                    BrowserTransferSource::Local(reader) => {
                        reader.seek(SeekFrom::Start(offset))?;
                        let mut bytes = vec![0; count];
                        reader.read_exact(&mut bytes)?;
                        Ok(bytes)
                    }
                })
                .await
                .map_err(BackendError::operation)?;
            self.import.import_transferred_staged_book(parent, name, staged, resource.state).await
        }
    }
}

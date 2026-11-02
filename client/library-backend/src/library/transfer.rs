//! Host resources for copying verified book bytes between library actors.
use super::{LibraryCommand, LibraryHandle, LibrarySession, ReadyCommand};
use book_access::BoxedBookReader;
use client_runtime::BackendError;
use library_database::transfer::{BookTransferState, TransferBook, TransferPlan};
use library_model::{DownloadProgress, DownloadState, LibraryUpdate, TransferOrigin};
use library_runtime::events::LibraryEvent;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
            let resource = source.export_transfer_book(book.book_id).await?;
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
            let resource = source.export_transfer_book(book.book_id).await?;
            let copied = self.import_transfer_book(parent, book.file_name.clone(), resource).await?;
            progress.books.retain(|copied| copied.source != book.book_id || copied.directory != book.directory);
            progress.books.push(StorageCopiedBook { source: book.book_id, directory: book.directory, destination: copied.book_id });
            checkpoint(&progress)?;
        }
        let current: TransferPlan = source.dispatch(LibraryCommand::Ready(ReadyCommand::StorageCopyPlan)).await?.take()?;
        if current != plan {
            return Err(BackendError::message("library changed during the copy; retry to include the latest books"));
        }
        Ok(())
    }

    async fn storage_copy_book_present(&self, book: &TransferBook, progress: &StorageCopyProgress, parent_id: DirId) -> Result<bool, BackendError> {
        let Some(copied) = progress.books.iter().find(|copied| copied.source == book.book_id && copied.directory == book.directory) else { return Ok(false) };
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
    /// Retain every book in this library before releasing its cloud bytes.
    /// Library, book and directory identities and reading state stay intact.
    pub(crate) async fn save_books_locally(&self) -> Result<(), BackendError> {
        let _activity = self.book_storage_activity()?;
        let plan = self.db.storage_copy_plan()?;
        let mut seen = HashSet::new();
        let mut missing = Vec::new();
        let mut required_bytes = 0_u64;
        for book in &plan.books {
            let hash = book.book_id;
            if !seen.insert(hash) || self.local_book_verified(hash).await? {
                continue;
            }
            let resource = Box::pin(self.export_transfer_book(hash)).await?;
            required_bytes = required_bytes.checked_add(resource.length).ok_or_else(|| BackendError::message("library size exceeds available storage"))?;
            missing.push(hash);
        }
        self.assets.check_write_capacity(required_bytes).map_err(BackendError::operation)?;
        self.download_books_locally(&missing).await?;
        // Repair publication may choose a safe new filename instead of
        // replacing mismatching bytes. Names and moves do not change which
        // books must be retained before releasing this library's cloud bytes.
        let current = self.db.storage_copy_plan()?.books.into_iter().map(|book| book.book_id).collect::<HashSet<_>>();
        if current != seen {
            return Err(BackendError::message("library changed while saving books locally; retry to include the latest books"));
        }
        self.events.publish_update(LibraryUpdate::Contents);
        Ok(())
    }

    /// Download only the verified-missing identities that the caller planned.
    pub(crate) async fn download_books_locally(&self, books: &[BookId]) -> Result<(), BackendError> {
        let mut missing = Vec::new();
        for &book in books {
            if !self.local_book_verified(book).await? {
                missing.push(book);
            }
        }
        for hash in missing {
            self.db.sync_request_book_downloads(&[hash], TransferOrigin::UserInitiated)?;
            self.emit(LibraryEvent::DownloadStatusChanged { book_id: hash, state: DownloadState::Downloading(DownloadProgress::ZERO) });
            let result: Result<(), BackendError> = async {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Box::pin(self.sync.asset_executor().redownload_book(hash)).await.map_err(BackendError::operation)?;
                    if !self.local_book_verified(hash).await? {
                        return Err(BackendError::message("a book could not be saved locally; cloud storage has been kept"));
                    }
                }
                #[cfg(target_arch = "wasm32")]
                Box::pin(self.save_browser_book_locally(hash)).await?;
                Ok(())
            }
            .await;
            let state = if result.is_ok() {
                DownloadState::Downloaded
            } else {
                if let Err(error) = self.db.complete_download(&hash) {
                    log::warn!("could not clear failed book download {hash}: {error}");
                }
                DownloadState::NotDownloaded
            };
            self.emit(LibraryEvent::DownloadStatusChanged { book_id: hash, state });
            // Publish each completed book, including partial success before a
            // later failure. Browser cards retain availability until this event.
            self.events.publish_update(LibraryUpdate::Contents);
            result?;
        }
        Ok(())
    }

    pub(crate) async fn local_book_verified(&self, hash: BookId) -> Result<bool, BackendError> {
        let checksum = self.db.book_transfer_state(hash)?.expected_checksum();
        Ok(self.assets.verified_book_reader_with_checksum(hash, self.db.book_paths(&hash)?, checksum).await.map_err(BackendError::operation)?.is_some())
    }

    #[cfg(target_arch = "wasm32")]
    async fn save_browser_book_locally(&self, hash: BookId) -> Result<(), BackendError> {
        use library_files::AssetStoreError;
        use std::io::{Read, Seek, SeekFrom};

        let resource = Box::pin(self.export_transfer_book(hash)).await?;
        let mut source = resource.source;
        let staged = self
            .assets
            .stage_transfer_chunks(resource.length, resource.state.expected_checksum(), async |offset, count| match &mut source {
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
        let checksum = staged.byte_checksum();
        let staged = staged.with_local_identity(&self.assets, hash).await.map_err(BackendError::operation)?;
        let thumbnail_set_exists = self.assets.thumbnail_set_exists(&hash)?;
        // Publication and acknowledgement do not yield: synchronization cannot
        // move or remove the identity between these two operations.
        let publication = staged.publish_pinned().map_err(BackendError::operation)?;
        if let Err(error) = self.db.complete_stored_book_download(hash, checksum, resource.length, thumbnail_set_exists) {
            if let Err(cleanup_error) = publication.discard() {
                return Err(BackendError::message(format!("{error}; could not discard the rejected download: {cleanup_error}")));
            }
            return Err(error.into());
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn export_transfer_book(&self, hash: BookId) -> Result<TransferResource, BackendError> {
        library_placement::run_file_jobs_for(&self.assets, &self.db, &hash).await.map_err(BackendError::operation)?;
        let mut state = self.db.book_transfer_state(hash)?;
        let paths = self.db.book_paths(&hash)?;

        if let Some(book) = self.assets.verified_book_reader_with_checksum(hash, paths, state.byte_checksum).await.map_err(BackendError::operation)? {
            if state.byte_checksum.is_some_and(|expected| book.book_id.content_hash() != Some(expected)) {
                return Err(BackendError::message("local transfer bytes do not match the recorded checksum"));
            }
            state.byte_checksum = book.book_id.content_hash();
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
        let state = self.db.book_transfer_state(hash)?;
        let reader = self.assets.verified_book_reader_with_checksum(hash, self.db.book_paths(&hash)?, state.byte_checksum).await.map_err(BackendError::operation)?;
        let (source, length) = if let Some(reader) = reader {
            let length = reader.length;
            (BrowserTransferSource::Local(Box::new(reader) as BoxedBookReader), length)
        } else {
            let (file, _) = self.describe_remote_file(hash).await.map_err(super::remote_access::range_backend_error)?;
            (BrowserTransferSource::Remote(self.remote_book_access(&file)?), file.length)
        };
        Ok(TransferResource { source, state, length })
    }

    pub(crate) async fn import_transfer_resource(&self, parent: DirId, name: String, resource: TransferResource) -> Result<library_database::transfer::ImportedBook, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(self.import.import_transferred_book(parent, name, resource.reader, resource.length, resource.state)).await
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

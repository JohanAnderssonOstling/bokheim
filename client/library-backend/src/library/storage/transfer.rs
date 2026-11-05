//! Host resources for copying verified book bytes between library services.

use crate::asset_store::AssetStore;
use crate::library::{LibraryHandle, LibraryResources};
use book_access::BoxedBookReader;
use client_protocol::BackendError;
use library_database::transfer::{BookTransferState, ImportedBook, LocalBookMetadata, TransferBook, TransferPlan};
use library_database::DatabaseClient;
use library_model::{DownloadProgress, DownloadState, TransferOrigin};
use library_placement::LibraryState;
use remote_book_access::{describe_remote_file, RemoteBookAccess};
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
    pub async fn copy_storage_from<F: std::future::Future<Output = Result<(), BackendError>>>(
        &self, source: &LibraryHandle, mut progress: StorageCopyProgress, mut checkpoint: impl FnMut(StorageCopyProgress) -> F,
    ) -> Result<(), BackendError> {
        if self.id() == source.id() {
            return Err(BackendError::message("choose another library for the storage copy"));
        }
        let plan: TransferPlan = source.storage_copy_plan().await?;
        let mapped = |id: DirId| if id == ROOT_DIR_ID { ROOT_DIR_ID } else { DirId::new_v5(self.id(), id.as_bytes()) };
        // Import each book directly. Failed copies retain their completed checkpoints.
        for folder in &plan.folders {
            self.ensure_storage_copy_directory(mapped(folder.id), mapped(folder.parent_id.ok_or("source folder has no parent")?), folder.label.clone()).await?;
        }
        for book in &plan.books {
            let parent = mapped(book.directory);
            if self.storage_copy_checkpoint_present(book, &progress, parent).await? {
                continue;
            }
            let resource = source.export_transfer_book(book.book_id).await?;
            let copied = self.import_transfer_book(parent, book.file_name.clone(), resource).await?;
            progress.books.retain(|copied| copied.source != book.book_id || copied.directory != book.directory);
            progress.books.push(StorageCopiedBook { source: book.book_id, directory: book.directory, destination: copied.book_id });
            checkpoint(progress.clone()).await?;
        }
        let current: TransferPlan = source.storage_copy_plan().await?;
        if current != plan {
            return Err(BackendError::message("library changed during the copy; retry to include the latest books"));
        }
        Ok(())
    }

    async fn storage_copy_checkpoint_present(&self, book: &TransferBook, progress: &StorageCopyProgress, parent_id: DirId) -> Result<bool, BackendError> {
        let Some(copied) = progress.books.iter().find(|copied| copied.source == book.book_id && copied.directory == book.directory) else { return Ok(false) };
        self.storage_copy_book_present(copied.destination, parent_id).await
    }
}

pub struct TransferResource {
    length: u64,
    #[cfg(not(target_arch = "wasm32"))]
    pub reader: BoxedBookReader,
    #[cfg(target_arch = "wasm32")]
    pub(crate) source: client_platform_runtime::executor::ServiceHandle<std::rc::Rc<tokio::sync::Mutex<BrowserTransferSource>>>,
    pub state: BookTransferState,
}

pub(super) async fn local_book_verified(database: &DatabaseClient, assets: &AssetStore, hash: BookId) -> Result<bool, BackendError> {
    let LocalBookMetadata { state, paths } = database.local_book_metadata(hash).await?;
    Ok(assets.verified_book_reader_with_checksum(hash, paths, state.expected_checksum()).await.map_err(BackendError::operation)?.is_some())
}

impl LibraryResources {
    /// Retain every book in this library before releasing its cloud bytes.
    /// Library, book and directory identities and reading state stay intact.
    pub(crate) async fn save_books_locally(&self) -> Result<(), BackendError> {
        let plan = self.database.storage_copy_plan().await?;
        let mut seen = HashSet::new();
        let books = plan.books.iter().map(|book| book.book_id).filter(|book| seen.insert(*book)).collect::<Vec<_>>();
        self.download_books_locally(&books).await?;
        self.events.refresh_contents();
        Ok(())
    }

    /// Download only the verified-missing identities that the caller planned.
    pub(crate) async fn download_books_locally(&self, books: &[BookId]) -> Result<(), BackendError> {
        let (database, assets) = {
            let storage = self.state.read().await;
            (storage.database().clone(), storage.assets().clone())
        };
        let mut seen = HashSet::new();
        for &hash in books {
            if !seen.insert(hash) || local_book_verified(&database, &assets, hash).await? {
                continue;
            }
            database.sync_request_book_downloads(&[hash], TransferOrigin::UserInitiated).await?;
            self.events.download_status_changed(hash, DownloadState::Downloading(DownloadProgress::ZERO));
            let result: Result<(), BackendError> = self
                .account
                .while_authorized(async {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        Box::pin(self.transfers.redownload_book(hash)).await.map_err(BackendError::operation)?;
                        if !local_book_verified(&database, &assets, hash).await? {
                            return Err(BackendError::message("a book could not be saved locally; cloud storage has been kept"));
                        }
                    }
                    #[cfg(target_arch = "wasm32")]
                    Box::pin(self.save_browser_book_locally(hash)).await?;
                    Ok(())
                })
                .await
                .map_err(library_services::account::account_error)
                .and_then(|result| result);
            let state = if result.is_ok() {
                DownloadState::Downloaded
            } else {
                if let Err(error) = database.complete_download(&hash).await {
                    log::warn!("could not clear failed book download {hash}: {error}");
                }
                DownloadState::NotDownloaded
            };
            self.events.download_status_changed(hash, state);
            // Publish each completed book, including partial success before a
            // later failure. Browser cards retain availability until this event.
            self.events.refresh_contents();
            result?;
        }
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    async fn save_browser_book_locally(&self, hash: BookId) -> Result<(), BackendError> {
        let mut resource = {
            let storage = self.state.write().await;
            Box::pin(self.export_transfer_book(&storage, hash)).await?
        };
        let assets = self.state.read().await.assets().clone();
        let staged = resource.stage(&assets).await?;
        let checksum = staged.byte_checksum();
        let staged = staged.with_local_identity(&assets, hash).await.map_err(BackendError::operation)?;
        let storage = self.state.write().await;
        let thumbnail_set_exists = storage.assets().thumbnail_set_exists(&hash).await?;
        // Publication and acknowledgement do not yield: synchronization cannot
        // move or remove the identity between these two operations.
        let publication = staged.publish_pinned().await.map_err(BackendError::operation)?;
        if let Err(error) = storage.database().complete_stored_book_download(hash, checksum, resource.length, thumbnail_set_exists).await {
            if let Err(cleanup_error) = publication.discard().await {
                return Err(BackendError::message(format!("{error}; could not discard the rejected download: {cleanup_error}")));
            }
            return Err(error.into());
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn export_transfer_book(&self, storage: &LibraryState, hash: BookId) -> Result<TransferResource, BackendError> {
        library_placement::run_file_jobs_for(storage.assets(), storage.database(), &hash).await.map_err(BackendError::operation)?;
        let LocalBookMetadata { mut state, paths } = storage.database().local_book_metadata(hash).await?;

        if let Some(book) = storage.assets().verified_book_reader_with_checksum(hash, paths, state.byte_checksum).await.map_err(BackendError::operation)? {
            if state.byte_checksum.is_some_and(|expected| book.book_id.content_hash() != Some(expected)) {
                return Err(BackendError::message("local transfer bytes do not match the recorded checksum"));
            }
            state.byte_checksum = book.book_id.content_hash();
            let length = book.length;
            return Ok(TransferResource { reader: Box::new(book), state, length });
        }

        // Read the complete original asset, including an MP3 folder's archive.
        let (source, prefix) = describe_remote_file(self.sync.http_client(), &self.account, self.id, hash).await.map_err(BackendError::operation)?;
        let length = source.length;
        let access = RemoteBookAccess::new(self.sync.http_client().clone(), self.account.clone(), self.id, source);
        let reader = book_access::remote_file::native_reader(length, prefix, access.fetch()).map_err(BackendError::operation)?;
        Ok(TransferResource { reader, state, length })
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) enum BrowserTransferSource {
    Local(BoxedBookReader),
    Remote(RemoteBookAccess),
}
#[cfg(target_arch = "wasm32")]
impl TransferResource {
    async fn stage(&mut self, assets: &AssetStore) -> Result<library_files::asset_store::StagedBook, BackendError> {
        use library_files::AssetStoreError;
        use std::io::{Read, Seek, SeekFrom};
        assets
            .stage_transfer_chunks(self.length, self.state.expected_checksum(), async |offset, count| {
                self.source
                    .call(move |source| async move {
                        match &mut *source.lock().await {
                            BrowserTransferSource::Remote(access) => access.read(offset, count).await.map_err(AssetStoreError::operation),
                            BrowserTransferSource::Local(reader) => {
                                reader.seek(SeekFrom::Start(offset))?;
                                let mut bytes = vec![0; count];
                                reader.read_exact(&mut bytes)?;
                                Ok(bytes)
                            }
                        }
                    })
                    .await
                    .map_err(AssetStoreError::operation)?
            })
            .await
            .map_err(BackendError::operation)
    }
}
impl LibraryResources {
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn export_transfer_book(&self, storage: &LibraryState, hash: BookId) -> Result<TransferResource, BackendError> {
        let LocalBookMetadata { state, paths } = storage.database().local_book_metadata(hash).await?;
        let reader = storage.assets().verified_book_reader_with_checksum(hash, paths, state.byte_checksum).await.map_err(BackendError::operation)?;
        let (source, length) = if let Some(reader) = reader {
            let length = reader.length;
            (BrowserTransferSource::Local(Box::new(reader) as BoxedBookReader), length)
        } else {
            let (file, _) = describe_remote_file(self.sync.http_client(), &self.account, self.id, hash).await.map_err(BackendError::operation)?;
            let length = file.length;
            (BrowserTransferSource::Remote(RemoteBookAccess::new(self.sync.http_client().clone(), self.account.clone(), self.id, file)), length)
        };
        let source = client_platform_runtime::executor::storage_call(move || async move { client_platform_runtime::executor::local_service_on_current_thread(std::rc::Rc::new(tokio::sync::Mutex::new(source)), ()) })
            .await
            .map_err(BackendError::operation)?;
        Ok(TransferResource { source, state, length })
    }

    pub(crate) async fn import_transfer_resource(&self, storage: &LibraryState, parent: DirId, name: String, resource: TransferResource) -> Result<library_database::transfer::ImportedBook, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(self.import(storage).import_transferred_book(parent, name, resource.reader, resource.length, resource.state)).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let mut resource = resource;
            let staged = resource.stage(storage.assets()).await?;
            self.import(storage).import_transferred_staged_book(parent, name, staged, resource.state).await
        }
    }
}

impl LibraryHandle {
    pub(crate) async fn export_transfer_book(&self, book_id: BookId) -> Result<TransferResource, BackendError> {
        self.run_io(async {
            let session = &self.owner.resources;
            let storage = self.storage().await?;
            session.export_transfer_book(&storage, book_id).await
        })
        .await
    }

    pub(crate) async fn import_transfer_book(&self, parent_id: DirId, file_name: String, resource: TransferResource) -> Result<ImportedBook, BackendError> {
        self.run_import(move |session, storage| async move { session.import_transfer_resource(&storage, parent_id, file_name, resource).await }).await
    }
}

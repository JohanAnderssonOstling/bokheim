//! Device-local text reuse before any remote opening or authorization request.

use super::{DocumentFormat, DocumentSource};
use crate::library::{LibraryHandle, downloads::persistent_download_state};
use crate::{BackendError, BookId, BookSource, DownloadState};
use book_access::{AsyncBookSource, SparseBookSource};
use book_content::{Book, ReadingBook, ResourceFetch};
use content_address::ContentHash;
use remote_book_access::RemoteBookAccess;
use std::{io, sync::Arc};
use tokio::sync::Mutex;

/// The library and immutable byte identity owning one prepared text cache.
pub struct TextCache {
    library: LibraryHandle,
    book: BookId,
    checksum: ContentHash,
}

impl LibraryHandle {
    /// Local files retain their normal opening path so local revisions cannot
    /// be hidden by cached text from the immutable remote copy.
    pub async fn text_cache(&self, book: BookId) -> Result<Option<TextCache>, BackendError> {
        self.run_io(async {
            let storage = self.storage().await?;
            let info = storage.database().local_book_info(&book).await?;
            if !matches!(DocumentFormat::try_from(info.format), Ok(DocumentFormat::Text(_))) || matches!(persistent_download_state(storage.database(), storage.assets(), book).await?, DownloadState::Downloaded) {
                return Ok(None);
            }
            let checksum = storage.database().canonical_content_hash(book).await?.or_else(|| book.content_hash());
            Ok(checksum.map(|checksum| TextCache { library: self.clone(), book, checksum }))
        })
        .await
    }

    /// Open only container metadata for lazy resources. EPUB text prefetch is
    /// deliberately absent here; the cached text is already available.
    async fn open_resource_book(&self, book: BookId) -> Result<Book, BackendError> {
        let storage = self.storage().await?;
        let info = storage.database().local_book_info(&book).await?;
        let DocumentFormat::Text(format) = DocumentFormat::try_from(info.format)? else {
            return Err(BackendError::message("cached text requires a text book"));
        };
        if matches!(persistent_download_state(storage.database(), storage.assets(), book).await?, DownloadState::Downloaded) {
            drop(storage);
            let content = self.resolve_book(book).await?.into_document()?;
            return match content.source {
                DocumentSource::Async(source) => Book::open(format, move || source.reader()).await,
                DocumentSource::Reader(reader) => Book::open_reader(format, reader),
            }
            .map_err(BackendError::operation);
        }
        let library = &self.owner.resources;
        let resolved = library.remote_book_source(&storage, book, info.format).await?;
        let source = match resolved.source {
            BookSource::RemoteArchive { source, tail } => {
                let length = source.length;
                let offset = length.checked_sub(tail.len() as u64).ok_or("invalid ZIP tail")?;
                let access = RemoteBookAccess::new(library.sync.http_client().clone(), library.account.clone(), library.id, source);
                #[cfg(not(target_arch = "wasm32"))]
                let source = SparseBookSource::remote(length, offset, tail, access.fetch())?;
                #[cfg(target_arch = "wasm32")]
                let source = {
                    let fetch = access.fetch();
                    SparseBookSource::browser(length, offset, tail, move |offset, length| {
                        let response = fetch(offset, length);
                        Box::pin(async move { response.await.map_err(client_protocol::OperationError::operation) })
                    })?
                };
                AsyncBookSource::from(source)
            }
            BookSource::Remote { source, prefix } => {
                let length = source.length;
                RemoteBookAccess::new(library.sync.http_client().clone(), library.account.clone(), library.id, source).async_source(length, prefix)?
            }
            _ => return Err(BackendError::message("expected a remote text source")),
        };
        Book::open(format, move || source.reader()).await.map_err(BackendError::operation)
    }
}

impl TextCache {
    /// An absent, obsolete or unreadable cache falls back to ordinary opening.
    pub async fn load(&self) -> Option<ReadingBook> {
        let result = self
            .library
            .run_io(async {
                let storage = self.library.storage().await?;
                let fetch = self.resource_fetch();
                storage.assets().read_text_cache(self.checksum, move |reader| ReadingBook::read_cache(reader, fetch)).await.map_err(BackendError::operation)
            })
            .await;
        match result {
            Ok(reading) => reading,
            Err(error) => {
                log::warn!("could not restore prepared text cache: {error}");
                None
            }
        }
    }

    /// Writing is best effort: its temporary file never becomes visible until
    /// all bytes have been written and flushed successfully.
    pub async fn store(&self, reading: Arc<ReadingBook>) {
        let result = self
            .library
            .run_io(async {
                let storage = self.library.storage().await?;
                storage.assets().write_text_cache(self.checksum, move |writer| reading.write_cache(writer)).await.map_err(BackendError::operation)
            })
            .await;
        if let Err(error) = result {
            log::warn!("could not publish prepared text cache: {error}");
        }
    }

    fn resource_fetch(&self) -> ResourceFetch {
        let library = self.library.clone();
        let book = self.book;
        let opened = Arc::new(Mutex::new(None));
        #[cfg(not(target_arch = "wasm32"))]
        {
            Arc::new(move |uri| {
                let library = library.clone();
                let opened = opened.clone();
                Box::pin(read_resource(library, book, opened, uri))
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Fetch and OPFS futures stay on their owning event loop; only the
            // resource URI and reply cross into the image loader's workers.
            let service = client_platform_runtime::executor::local_service_on_current_thread((library, book, opened), ());
            Arc::new(move |uri| {
                let service = service.clone();
                Box::pin(async move { service.call(move |(library, book, opened)| read_resource(library, book, opened, uri)).await.map_err(io::Error::other)? })
            })
        }
    }
}

async fn read_resource(library: LibraryHandle, book: BookId, opened: Arc<Mutex<Option<ResourceFetch>>>, uri: String) -> io::Result<Vec<u8>> {
    library
        .run_io(async {
            let fetch = {
                let mut opened = opened.lock().await;
                if opened.is_none() {
                    *opened = Some(library.open_resource_book(book).await?.into_resource_fetch().await.map_err(BackendError::operation)?);
                }
                opened.as_ref().expect("opened resource fetcher").clone()
            };
            fetch(uri).await.map_err(BackendError::operation)
        })
        .await
        .map_err(io::Error::other)
}

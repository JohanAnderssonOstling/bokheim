//! Account reads independent of the library state lock.

#[cfg(target_arch = "wasm32")]
use super::browser::SOURCES;
use crate::library::{DirectPlayback, LibraryHandle, RemoteFile};
use crate::{BackendError, BookId};
use book_access::remote_file::range_byte_count;
use futures_util::FutureExt;
use remote_book_access::RemoteBookAccess;
use std::{future::Future, panic::AssertUnwindSafe};

impl LibraryHandle {
    /// Run reads in the shared I/O context and convert panics to read errors.
    async fn run_read<T>(&self, read: impl Future<Output = Result<T, BackendError>>) -> Result<T, BackendError> {
        self.run_io(async {
            AssertUnwindSafe(read).catch_unwind().await.unwrap_or_else(|_| Err(BackendError::message("library read panicked")))
        })
        .await
    }

    pub async fn read_file_range(&self, source: RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        self.run_read(async {
            let context = &self.owner.resources;
            RemoteBookAccess::new(context.sync.http_client().clone(), context.account.clone(), context.id, source).read(offset, length).await.map_err(BackendError::operation)
        })
        .await
    }

    pub async fn read_file_ranges(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, BackendError> {
        range_byte_count(&ranges, source.length, 4 * 1024 * 1024).ok_or_else(|| BackendError::message("invalid book ranges"))?;
        self.run_read(async {
            let context = &self.owner.resources;
            RemoteBookAccess::new(context.sync.http_client().clone(), context.account.clone(), context.id, source).read_ranges(ranges).await.map_err(BackendError::operation)
        })
        .await
    }

    /// Renewal authorizes the same remote book without reopening local metadata.
    pub async fn renew_remote_audio(&self, book_id: BookId) -> Result<DirectPlayback, BackendError> {
        self.run_read(async {
            let context = &self.owner.resources;
            remote_book_access::direct_audio_source(context.sync.http_client(), &context.account, context.id, book_id).await.map_err(BackendError::operation)
        })
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn read_book_range(&self, capability: String, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        self.run_read(async {
            let library = *self.id();
            client_platform_runtime::executor::run_blocking(move || {
                let sources = SOURCES.lock().expect("browser book sources");
                let (owner, _) = sources.get(&capability).ok_or("book source closed; reopen the book")?;
                if *owner != library {
                    return Err("book belongs to another library".to_owned());
                }
                client_platform_web::range_reader::read_range(&capability, offset, length)
            })
            .await
            .map_err(BackendError::operation)?
            .map_err(BackendError::operation)
        })
        .await
    }
}

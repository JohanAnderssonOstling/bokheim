use super::LibraryClient;
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand, RemoteFile};
use crate::ThumbnailResolution;
use crate::{BackendError, BookId, DownloadState, ResolvedBook};
use library_model::CardKey;

impl LibraryClient {
    /// Read a bounded remote bundle without exposing its wire byte wrapper.
    pub async fn read_file_bundle(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, BackendError> {
        let bytes: serde_bytes::ByteBuf = self.request(LibraryCommand::Pending(PendingCommand::ReadFileBundle { source, ranges })).await?;
        Ok(bytes.into_vec())
    }

    pub async fn resolve_book_identity(&self, book_id: BookId) -> Result<BookId, BackendError> {
        if book_id.content_hash().is_none() {
            return Ok(book_id);
        }
        self.request(LibraryCommand::Ready(ReadyCommand::ResolveBookIdentity { book_id })).await
    }

    pub async fn book_format(&self, book_id: BookId) -> Result<Option<book_model::BookFormat>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BookFormat { book_id })).await
    }

    pub async fn thumbnail(&self, book_id: BookId, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(LibraryCommand::Pending(PendingCommand::Thumbnail { book_id, resolution })).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }
    pub async fn external_catalog_cover(&self, entry: CardKey, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(LibraryCommand::Pending(PendingCommand::ExternalCatalogCover { entry, resolution })).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }
    pub async fn download_state(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::DownloadState { book_id })).await
    }
    pub async fn download_book(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::DownloadBook { book_id })).await
    }

    /// Drops the downloaded files for one book while keeping its entry,
    /// placements, and reading state. Fails when the server holds no copy.
    pub async fn remove_local_book_copy(&self, book_id: BookId) -> Result<DownloadState, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::RemoveLocalBookCopy { book_id })).await
    }
    pub async fn restore_book(&self, book_id: BookId) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::RestoreBook { book_id })).await
    }
    pub async fn purge_book(&self, book_id: BookId) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::PurgeBook { book_id })).await
    }
    pub async fn move_book_to_trash(&self, book_id: BookId) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::MoveBookToTrash { book_id })).await
    }
    /// Opens the book using the target's reader and source adapter.
    pub async fn resolve_book(&self, book_id: BookId) -> Result<ResolvedBook, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use client_platform_runtime::startup_profile::StartupProfile;
            let mut profile = StartupProfile::new("book_lookup_client");
            let handle = self.transport.handle().await?;
            profile.mark("transport_handle");
            let result = handle.resolve_book(book_id).await;
            profile.mark("actor_roundtrip");
            result
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::library::web_reader::resolve_book(self, book_id).await
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_audiobook_track(&self, book_id: BookId, index: usize) -> Result<ResolvedBook, BackendError> {
        self.transport.handle().await?.resolve_audiobook_track(book_id, index).await
    }
}

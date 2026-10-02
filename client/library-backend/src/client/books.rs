use super::LibraryClient;
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand, SessionCommand};
use crate::ThumbnailResolution;
use crate::{ContentHash, DownloadState, ResolvedBook};

impl LibraryClient {
    pub async fn book_format(&self, content_hash: ContentHash) -> Result<Option<book_model::BookFormat>, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::BookFormat { content_hash }))).await
    }

    pub async fn thumbnail(&self, content_hash: ContentHash, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, String> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(LibraryCommand::Session(SessionCommand::Pending(PendingCommand::Thumbnail { content_hash, resolution }))).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }
    pub async fn download_book(&self, content_hash: ContentHash) -> Result<DownloadState, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::DownloadBook { content_hash }))).await
    }

    /// Drops the downloaded files for one book while keeping its entry,
    /// placements, and reading state. Fails when the server holds no copy.
    pub async fn remove_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::RemoveLocalBookCopy { content_hash }))).await
    }
    pub async fn restore_book(&self, content_hash: ContentHash) -> Result<(), String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::RestoreBook { content_hash }))).await
    }
    pub async fn purge_book(&self, content_hash: ContentHash) -> Result<(), String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::PurgeBook { content_hash }))).await
    }
    pub async fn move_book_to_trash(&self, content_hash: ContentHash) -> Result<(), String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::MoveBookToTrash { content_hash }))).await
    }
    /// Opens the book using the target's reader and source adapter.
    pub async fn resolve_book(&self, content_hash: ContentHash) -> Result<ResolvedBook, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.handle().map_err(|error| error.to_string())?.resolve_book(content_hash).await.map_err(|error| error.to_string())
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::library::web_reader::resolve_book(self, content_hash).await
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_audiobook_track(&self, content_hash: ContentHash, index: usize) -> Result<ResolvedBook, String> {
        self.transport.handle().map_err(|error| error.to_string())?.resolve_audiobook_track(content_hash, index).await.map_err(|error| error.to_string())
    }
}

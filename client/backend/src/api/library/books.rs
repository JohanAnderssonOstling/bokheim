use super::LibraryClient;
use crate::runtime::*;

impl LibraryClient {
    pub async fn book_format(&self, content_hash: crate::ContentHash) -> Result<Option<library_model::BookFormat>, String> {
        self.request(library_requests::BookFormat { content_hash }).await
    }

    pub async fn thumbnail(&self, content_hash: crate::ContentHash, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, String> {
        let bytes: Option<serde_bytes::ByteBuf> = self.request(library_requests::Thumbnail { content_hash, resolution }).await?;
        Ok(bytes.map(serde_bytes::ByteBuf::into_vec))
    }
    pub async fn download_book(&self, content_hash: crate::ContentHash) -> Result<crate::DownloadState, String> {
        self.request(library_requests::DownloadBook { content_hash }).await
    }
    pub async fn download_folder(&self, query: library_model::LibraryBrowseQuery) -> Result<(), String> {
        self.request(library_requests::DownloadFolder { query }).await
    }
    pub async fn download_subject(&self, query: library_model::LibraryBrowseQuery) -> Result<(), String> {
        self.request(library_requests::DownloadSubject { query }).await
    }
    pub async fn restore_book(&self, content_hash: crate::ContentHash) -> Result<(), String> {
        self.request(library_requests::RestoreBook { content_hash }).await
    }
    pub async fn purge_book(&self, content_hash: crate::ContentHash) -> Result<(), String> {
        self.request(library_requests::PurgeBook { content_hash }).await
    }
    pub async fn move_book_to_trash(&self, content_hash: crate::ContentHash) -> Result<(), String> {
        self.request(library_requests::MoveBookToTrash { content_hash }).await
    }
    /// Opens the publication using the target's reader and source adapter.
    pub async fn resolve_book(&self, content_hash: crate::ContentHash) -> Result<crate::ResolvedBook, String> {
        self.app.transport.resolve_book(self, content_hash).await
    }
}

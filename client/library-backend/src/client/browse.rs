use super::LibraryClient;
use crate::library::{LibraryCommand, ReadyCommand};
use crate::LibraryBrowseData;

impl LibraryClient {
    pub async fn book_count(&self) -> Result<usize, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::BookCount)).await
    }
    pub async fn browse(&self, query: library_model::LibraryBrowseQuery) -> Result<LibraryBrowseData, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::Browse { query })).await
    }
    pub async fn book_detail(&self, content_hash: crate::ContentHash) -> Result<library_model::BookDetail, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::BookDetail { content_hash })).await
    }
    pub async fn trash(&self) -> Result<library_model::LibraryTrashView, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::Trash)).await
    }
    /// Queues every book the query reaches, including those in nested folders.
    pub async fn download_browse(&self, query: library_model::LibraryBrowseQuery) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::DownloadBrowse { query })).await
    }
}

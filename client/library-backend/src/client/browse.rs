use super::LibraryClient;
use crate::library::{LibraryCommand, ReadyCommand, SessionCommand};
use crate::LibraryBrowseData;

impl LibraryClient {
    pub async fn book_count(&self) -> Result<usize, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::BookCount))).await
    }
    pub async fn home(&self, limit: i32) -> Result<library_model::LibraryHomeView, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Home { limit }))).await
    }
    pub async fn browse(&self, query: library_model::LibraryBrowseQuery) -> Result<LibraryBrowseData, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Browse { query }))).await
    }
    pub async fn book_detail(&self, content_hash: crate::ContentHash) -> Result<library_model::BookDetail, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::BookDetail { content_hash }))).await
    }
    pub async fn authors(&self, sort: library_model::AuthorSort) -> Result<Vec<library_model::AuthorSummary>, String> {
        self.request(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Authors { sort }))).await
    }
}

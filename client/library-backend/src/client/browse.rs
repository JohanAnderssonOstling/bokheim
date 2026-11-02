use super::LibraryClient;
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand};
use crate::{BackendError, LibraryBrowseData};
use library_model::{BrowseBookPage, BrowseLocation, BrowsePageRequest, LibraryBrowseQuery};

impl LibraryClient {
    pub async fn book_storage(&self, for_download: bool) -> Result<crate::LibraryBookStorage, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::LibraryBookStorage { for_download })).await
    }

    pub async fn download_all_books(&self, confirmed_books: Vec<crate::BookId>, confirmed_bytes: u64) -> Result<(), BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::DownloadAllBooks { confirmed_books, confirmed_bytes })).await
    }

    pub async fn book_count(&self) -> Result<usize, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BookCount)).await
    }
    pub async fn library_language_counts(&self) -> Result<Vec<library_model::LibraryLanguageCount>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::LibraryLanguageCounts)).await
    }
    pub async fn browse(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::Browse { query })).await
    }
    pub async fn browse_page(&self, query: LibraryBrowseQuery, page: BrowsePageRequest) -> Result<LibraryBrowseData, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BrowsePage { query, page })).await
    }
    pub async fn browse_result_page(&self, query: LibraryBrowseQuery, revision: String, page: BrowsePageRequest) -> Result<Option<LibraryBrowseData>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BrowseResultPage { query, revision, page })).await
    }
    pub async fn browse_result_books(&self, query: LibraryBrowseQuery, revision: String, section: BrowseLocation, offset: usize, limit: usize) -> Result<Option<BrowseBookPage>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BrowseResultBooks { query, revision, section, offset, limit })).await
    }
    pub async fn browse_section_books(&self, query: LibraryBrowseQuery, offset: usize, limit: usize) -> Result<BrowseBookPage, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BrowseSectionBooks { query, offset, limit })).await
    }
    pub async fn book_detail(&self, book_id: crate::BookId) -> Result<library_model::BookDetail, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::BookDetail { book_id })).await
    }
    pub async fn external_catalog_detail(&self, entry: library_model::CardKey) -> Result<library_model::BookDetail, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ExternalCatalogDetail { entry })).await
    }
    pub async fn trash(&self) -> Result<library_model::LibraryTrashView, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::Trash)).await
    }
    /// Queues every book the query reaches, including those in nested folders.
    pub async fn download_browse(&self, query: LibraryBrowseQuery) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::DownloadBrowse { query })).await
    }
}

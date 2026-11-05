//! Library browsing, details, and text search.

use crate::library::LibraryHandle;
use crate::{BackendError, BookId, LibraryBrowseData};
use library_model::{BrowseBookPage, BrowseLocation, BrowsePageRequest, LibraryBrowseQuery};

pub(super) mod text_search;

impl LibraryHandle {
    pub async fn book_count(&self) -> Result<usize, BackendError> {
        let database = self.database();
        Ok(database.book_count().await?)
    }

    pub async fn library_language_counts(&self) -> Result<Vec<library_model::LibraryLanguageCount>, BackendError> {
        let database = self.database();
        Ok(database.library_language_counts().await?)
    }

    pub async fn folder_destinations(&self) -> Result<Vec<library_model::LibraryFolderDestination>, BackendError> {
        let database = self.database();
        Ok(database.folder_destinations().await?)
    }

    pub async fn browse(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        let database = self.database();
        Ok(database.browse(&query, false).await?)
    }

    pub async fn browse_result_page(&self, query: LibraryBrowseQuery, revision: String, page: BrowsePageRequest) -> Result<Option<LibraryBrowseData>, BackendError> {
        let database = self.database();
        Ok(database.browse_result_page(&query, false, &revision, page).await?)
    }

    pub async fn browse_result_books(&self, query: LibraryBrowseQuery, revision: String, section: BrowseLocation, offset: usize, limit: usize) -> Result<Option<BrowseBookPage>, BackendError> {
        let database = self.database();
        Ok(database.browse_result_books(&query, false, &revision, section, offset, limit).await?)
    }

    pub async fn browse_page(&self, query: LibraryBrowseQuery, page: BrowsePageRequest) -> Result<LibraryBrowseData, BackendError> {
        let database = self.database();
        Ok(database.browse_page(&query, false, page).await?)
    }

    pub async fn browse_section_books(&self, query: LibraryBrowseQuery, offset: usize, limit: usize) -> Result<BrowseBookPage, BackendError> {
        let database = self.database();
        Ok(database.browse_section_books(&query, false, offset, limit).await?)
    }

    pub async fn book_detail(&self, book_id: BookId) -> Result<library_model::BookDetail, BackendError> {
        let database = self.database();
        Ok(database.browse_book_detail(&book_id).await?)
    }

    pub async fn external_catalog_detail(&self, entry: library_model::CardKey) -> Result<library_model::BookDetail, BackendError> {
        let database = self.database();
        Ok(database.external_catalog_detail(entry).await?)
    }
}

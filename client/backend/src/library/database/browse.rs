//! Native library-handle wrappers for shared browse queries.

use super::*;
use crate::app::BackendError;
use crate::library::{LibraryAuthorsView, LibraryBackend, LibraryFolderView, LibrarySubjectView};
use app_contract::library::{AuthorSort, BookDetail, BookTocEntry, BrowseBookSort, BrowseChipSort, BrowseContents, FolderSort, LibraryFileTypeFilter, LibraryHomeView, LibrarySearchResult};

fn convert_toc_entry(entry: epub_provider::TocEntry) -> BookTocEntry {
    BookTocEntry { title: entry.title, target: entry.link, children: entry.children.into_iter().map(convert_toc_entry).collect() }
}

impl LibraryBackend {
    pub fn book_detail(&self, content_hash: ContentHash) -> Result<BookDetail, BackendError> {
        let mut detail = self.book_detail_summary(content_hash.clone())?;
        detail.toc = self.book_toc(content_hash)?;
        Ok(detail)
    }

    pub fn book_detail_summary(&self, content_hash: ContentHash) -> Result<BookDetail, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::book_detail_summary(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn book_toc(&self, content_hash: ContentHash) -> Result<Vec<BookTocEntry>, BackendError> {
        let toc = if self.is_epub(content_hash.clone())? {
            let resolved = self.resolve_book_for_reader(content_hash)?;
            epub_provider::EpubProvider::try_from_reader(resolved.reader).and_then(|provider| provider.toc()).ok().flatten().unwrap_or_default().into_iter().map(convert_toc_entry).collect()
        } else if self.is_audiobook(content_hash.clone())? {
            self.audiobook_chapters(content_hash)?.into_iter().map(|chapter| BookTocEntry { title: chapter.title, target: app_contract::library::audiobook_toc_target(chapter.start_ms), children: Vec::new() }).collect()
        } else {
            Vec::new()
        };
        Ok(toc)
    }

    pub fn home(&self, limit: i32) -> Result<LibraryHomeView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_home(&conn, limit, downloaded_only).map_err(BackendError::operation)
    }

    pub fn child_directories(&self, parent_id: &DirId) -> Result<Vec<Dir>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut directories = get_child_directories(&conn, &parent_id.to_string()).map_err(BackendError::operation)?;
        FolderSort::Name.sort(&mut directories);
        Ok(directories)
    }

    /// Direct database contents for `folder_id`, or scoped search results.
    ///
    /// Both queries use the same connection. No application sorting,
    /// filtering, or view-model conversion is applied.
    pub fn folder_contents(&self, folder_id: &str, query: &str, file_type: LibraryFileTypeFilter) -> Result<BrowseContents, BackendError> {
        self.folder_contents_sorted(folder_id, query, file_type, BrowseChipSort::Alphabetical, BrowseBookSort::Alphabetical, None)
    }

    pub fn folder_contents_sorted(&self, folder_id: &str, query: &str, file_type: LibraryFileTypeFilter, chip_sort: BrowseChipSort, book_sort: BrowseBookSort, language: Option<&str>) -> Result<BrowseContents, BackendError> {
        let languages = language.into_iter().collect::<Vec<_>>();
        self.folder_contents_filtered_sorted(folder_id, query, &[file_type], chip_sort, book_sort, &languages)
    }

    pub fn folder_contents_filtered_sorted(&self, folder_id: &str, query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, book_sort: BrowseBookSort, languages: &[&str]) -> Result<BrowseContents, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let folder_id = DirId::parse_str(folder_id).map_err(BackendError::operation)?.to_string();
        let normalized_query = library_model::normalize_search_text(query);
        let children = get_folder_list_rows_filtered_sorted(&conn, &folder_id, &normalized_query, file_types, chip_sort, languages).map_err(BackendError::operation)?;
        let books = get_book_cards_in_dir_filtered_sorted_scoped(&conn, &folder_id, false, &normalized_query, file_types, book_sort, languages).map_err(BackendError::operation)?;
        Ok(BrowseContents { children, books })
    }

    /// Direct database contents for one regular subject location, or scoped search results.
    ///
    /// Both queries use the same connection. No application sorting,
    /// filtering, compaction, or view-model conversion is applied.
    pub fn subject_contents(&self, subject_path: &str, query: &str, file_type: LibraryFileTypeFilter) -> Result<BrowseContents, BackendError> {
        self.subject_contents_sorted(subject_path, query, file_type, BrowseChipSort::Alphabetical, BrowseBookSort::Alphabetical, None)
    }

    pub fn subject_contents_sorted(&self, subject_path: &str, query: &str, file_type: LibraryFileTypeFilter, chip_sort: BrowseChipSort, book_sort: BrowseBookSort, language: Option<&str>) -> Result<BrowseContents, BackendError> {
        let languages = language.into_iter().collect::<Vec<_>>();
        self.subject_contents_filtered_sorted(subject_path, query, &[file_type], chip_sort, book_sort, &languages)
    }

    pub fn subject_contents_filtered_sorted(&self, subject_path: &str, query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, book_sort: BrowseBookSort, languages: &[&str]) -> Result<BrowseContents, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let normalized_query = library_model::normalize_search_text(query);
        let children = get_subject_browse_rows_filtered_sorted(&conn, subject_path, &normalized_query, file_types, chip_sort, languages).map_err(BackendError::operation)?;
        let books = get_book_cards_in_subject_filtered_sorted_scoped(&conn, subject_path, false, &normalized_query, file_types, book_sort, languages).map_err(BackendError::operation)?;
        Ok(BrowseContents { children, books })
    }

    pub fn unmapped_subject_texts(&self) -> Result<Vec<String>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::unmapped_subject_texts(&conn).map_err(BackendError::operation)
    }

    pub fn directory_path(&self, directory_id: &DirId) -> Result<Vec<Dir>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_dir_path_components(&conn, &directory_id.to_string()).map(|components| components.into_iter().map(|component| Dir::new(component.id, component.name, 0, 0)).collect()).map_err(BackendError::operation)
    }

    pub fn search(&self, query: &str) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_search(&conn, query, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    pub fn search_folder_filtered(&self, directory_id: &DirId, query: &str, file_type: LibraryFileTypeFilter) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_search_folder(&conn, directory_id, query, file_type, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    pub fn search_subject_filtered(&self, subject_path: Option<&str>, query: &str, file_type: LibraryFileTypeFilter) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_search_subject(&conn, subject_path, query, file_type, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    /// The author index in the reader's chosen order. The sort lives here
    /// rather than in each frontend so that both agree on the tie-breaks —
    /// two authors with the same book count, or the same filing name.
    pub fn authors(&self, selected: Option<AuthorId>, sort: AuthorSort) -> Result<LibraryAuthorsView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_authors(&conn, selected, sort, downloaded_only).map_err(BackendError::operation)
    }

    pub fn author_books(&self, author_id: AuthorId) -> Result<Vec<BookCardRow>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_book_cards_by_author_scoped(&conn, author_id, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    pub fn folder(&self, directory_id: &DirId) -> Result<LibraryFolderView, BackendError> {
        self.folder_filtered(directory_id, LibraryFileTypeFilter::All)
    }

    pub fn folder_filtered(&self, directory_id: &DirId, file_type: LibraryFileTypeFilter) -> Result<LibraryFolderView, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_folder(&conn, directory_id, self.app()?.downloaded_books_only()?, file_type).map_err(BackendError::operation)
    }

    pub fn subjects_filtered(&self, subject_path: Option<&str>, file_type: LibraryFileTypeFilter) -> Result<LibrarySubjectView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        library_sqlite::browse::library_subjects(&conn, subject_path, file_type, downloaded_only).map_err(BackendError::operation)
    }
}

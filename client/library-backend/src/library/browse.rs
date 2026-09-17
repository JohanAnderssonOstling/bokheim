//! Native library-handle wrappers for shared browse queries.

use crate::BackendError;
use crate::library::LibrarySession;
use crate::{ContentHash, DirId};
use library_database::LibraryBrowseData;
use library_model::{BrowseBookSort, BrowseChipSort, BrowseContents, LibraryFileTypeFilter, LibraryFormatCount, LibraryHomeView, LibraryLanguageCount};

impl LibrarySession {
    pub fn book_count(&self) -> Result<usize, BackendError> {
        self.db.browse_book_count().map_err(BackendError::operation)
    }

    pub fn folder_facet_counts(&self, folder_id: &str, query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str], downloaded_only: bool) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), BackendError> {
        self.db.browse_folder_facet_counts(folder_id, query, file_types, languages, downloaded_only).map_err(BackendError::operation)
    }

    pub fn subject_facet_counts(&self, subject_path: &str, query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str], downloaded_only: bool) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), BackendError> {
        self.db.browse_subject_facet_counts(subject_path, query, file_types, languages, downloaded_only).map_err(BackendError::operation)
    }

    /// Everything one book's page states, in one query rather than a request
    /// per panel: the card, its folder trail, its credits, its subjects, its
    /// navigation, and where the reader is inside it.
    pub fn book_detail(&self, content_hash: ContentHash) -> Result<library_model::BookDetail, BackendError> {
        self.db.browse_book_detail(content_hash).map_err(BackendError::operation)
    }

    pub fn home(&self, limit: i32, downloaded_only: bool) -> Result<LibraryHomeView, BackendError> {
        self.db.browse_home(limit, downloaded_only).map_err(BackendError::operation)
    }

    /// The author index: every credited identity with its shelf, already
    /// ordered. One query for the whole page rather than a shelf lookup per
    /// row, so the cost does not scale with the number of authors.
    pub fn authors(&self, sort: library_model::AuthorSort, downloaded_only: bool) -> Result<Vec<library_model::AuthorSummary>, BackendError> {
        self.db.browse_authors(sort, downloaded_only).map(|view| view.authors).map_err(BackendError::operation)
    }

    pub fn folder_contents_filtered_sorted(
        &self, folder_id: &str, query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, book_sort: BrowseBookSort, languages: &[&str], downloaded_only: bool,
    ) -> Result<BrowseContents, BackendError> {
        self.db.browse_folder_contents_filtered_sorted(folder_id, query, file_types, chip_sort, book_sort, languages, downloaded_only).map_err(BackendError::operation)
    }

    pub fn grouped_folder_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<(BrowseContents, Vec<library_model::BrowseChipGroup>), BackendError> {
        self.db.browse_grouped_folder_contents(query, downloaded_only).map_err(BackendError::operation)
    }

    pub fn subject_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<BrowseContents, BackendError> {
        self.db.browse_subject_contents(query, downloaded_only).map_err(BackendError::operation)
    }

    pub fn grouped_subject_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<(BrowseContents, Vec<library_model::BrowseChipGroup>), BackendError> {
        self.db.browse_grouped_subject_contents(query, downloaded_only).map_err(BackendError::operation)
    }

    pub fn directory_path(&self, directory_id: &DirId) -> Result<Vec<library_model::BrowsePathSegment>, BackendError> {
        self.db.browse_directory_path(directory_id).map_err(BackendError::operation)
    }
}

impl LibrarySession {
    pub(crate) fn folder_browse_data(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, String> {
        let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
        let (mut contents, chip_groups) = self.grouped_folder_contents(query, downloaded_only).map_err(|error| error.to_string())?;
        retain_unfinished(&mut contents, query.hide_finished);
        let (language_counts, format_counts) = self.folder_facet_counts(&query.location, &query.search, &query.file_types, &languages, downloaded_only).map_err(|error| error.to_string())?;
        let location = crate::DirId::parse_str(&query.location).map_err(|error| error.to_string())?;
        let path = self.directory_path(&location).map_err(|error| error.to_string())?;
        Ok(LibraryBrowseData { chip_groups, contents, path, language_counts, format_counts })
    }

    pub(crate) fn subject_browse_data(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, String> {
        let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
        let (contents, chip_groups) = self.grouped_subject_contents(query, downloaded_only).map_err(|error| error.to_string())?;
        let (language_counts, format_counts) = self.subject_facet_counts(&query.location, &query.search, &query.file_types, &languages, downloaded_only).map_err(|error| error.to_string())?;
        Ok(LibraryBrowseData { chip_groups, contents, path: Vec::new(), language_counts, format_counts })
    }

    pub(crate) fn queue_browse_downloads(&self, query: library_model::LibraryBrowseQuery, subject: bool) -> Result<(), String> {
        let hashes = collect_browse_downloads(query, |query| {
            let mut contents = if subject {
                self.subject_contents(query, false).map_err(|error| error.to_string())?
            } else {
                let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
                self.folder_contents_filtered_sorted(&query.location, &query.search, &query.file_types, query.chip_sort, query.book_sort, &languages, false).map_err(|error| error.to_string())?
            };
            retain_unfinished(&mut contents, query.hide_finished);
            Ok(contents)
        })?;
        self.request_downloads(&hashes).map_err(|error| error.to_string())
    }
}

/// Facet counts describe the location's full holdings, not its current
/// finished-book filter.
fn retain_unfinished(contents: &mut library_model::BrowseContents, hide_finished: bool) {
    if hide_finished {
        contents.books.retain(|book| book.progress < library_model::FINISHED_PROGRESS_THRESHOLD);
    }
}

fn collect_browse_downloads(mut query: library_model::LibraryBrowseQuery, mut contents: impl FnMut(&library_model::LibraryBrowseQuery) -> Result<library_model::BrowseContents, String>) -> Result<Vec<crate::ContentHash>, String> {
    let mut locations = vec![query.location.clone()];
    let mut visited = std::collections::HashSet::new();
    let mut hashes = std::collections::HashSet::new();
    while let Some(location) = locations.pop() {
        if !visited.insert(location.clone()) {
            continue;
        }
        query.location = location;
        let page = contents(&query)?;
        hashes.extend(page.books.into_iter().filter(|book| !book.downloaded && !book.download_requested).map(|book| book.content_hash));
        locations.extend(page.children.into_iter().filter(|child| child.downloaded_book_count < child.book_count).map(|child| child.id));
    }
    Ok(hashes.into_iter().collect())
}

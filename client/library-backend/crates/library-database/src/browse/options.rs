//! Normalize transport options once, before opening a browse snapshot.
use book_model::BookFormat;
use library_model::{BrowseBookSort, BrowseChipSort, LibraryBrowseQuery, LibraryFileTypeFilter};

pub(super) struct BrowseOptions {
    pub search: String,
    pub file_types: i32,
    pub languages: String,
    pub chip_sort: BrowseChipSort,
    pub book_sort: BrowseBookSort,
    pub downloaded_only: bool,
    pub hide_finished: bool,
    pub include_direct_child_books: bool,
}

impl BrowseOptions {
    pub fn from_query(query: &LibraryBrowseQuery, downloaded_only: bool) -> Self {
        Self {
            search: book_model::normalize_search_text(&query.search),
            file_types: file_types_query_value(&query.file_types),
            languages: languages_query_value(&query.languages),
            chip_sort: query.chip_sort,
            book_sort: query.book_sort,
            downloaded_only,
            hide_finished: query.hide_finished,
            include_direct_child_books: query.include_direct_child_books,
        }
    }
}

/// The mask of stored `BookFormat` codes a file-type filter matches.
pub(crate) fn file_type_query_value(filter: LibraryFileTypeFilter) -> i32 {
    match filter {
        LibraryFileTypeFilter::All => 0,
        LibraryFileTypeFilter::Book => BookFormat::Epub as i32 | BookFormat::Mobi as i32,
        LibraryFileTypeFilter::Pdf => BookFormat::Pdf as i32,
        LibraryFileTypeFilter::Audiobook => BookFormat::M4b as i32 | BookFormat::Mp3Folder as i32,
    }
}

/// The file-type filter a stored `BookFormat` code belongs to. A book whose
/// format no device has observed yet shows as a book.
pub(crate) fn file_type_of(format: Option<i64>) -> LibraryFileTypeFilter {
    match format.and_then(BookFormat::from_code) {
        Some(BookFormat::Pdf) => LibraryFileTypeFilter::Pdf,
        Some(BookFormat::M4b | BookFormat::Mp3Folder) => LibraryFileTypeFilter::Audiobook,
        _ => LibraryFileTypeFilter::Book,
    }
}

pub(crate) fn file_types_query_value(filters: &[LibraryFileTypeFilter]) -> i32 {
    if filters.is_empty() || filters.contains(&LibraryFileTypeFilter::All) {
        return 0;
    }
    filters.iter().fold(0, |mask, filter| mask | file_type_query_value(*filter))
}

/// The lowercase primary subtag browse filters on: `en-US` and `en_GB` are `en`.
pub(crate) fn base_language(tag: &str) -> String {
    tag.trim().split(['-', '_']).next().unwrap_or_default().to_lowercase()
}

pub(crate) fn languages_query_value(languages: &[String]) -> String {
    let mut languages = languages.iter().map(|language| base_language(language)).filter(|language| !language.is_empty()).collect::<Vec<_>>();
    languages.sort();
    languages.dedup();
    if languages.is_empty() { String::new() } else { format!(",{},", languages.join(",")) }
}

pub(crate) const fn book_sort_query_value(sort: BrowseBookSort) -> i32 {
    match sort {
        BrowseBookSort::Alphabetical => 0,
        BrowseBookSort::RecentlyRead => 1,
    }
}

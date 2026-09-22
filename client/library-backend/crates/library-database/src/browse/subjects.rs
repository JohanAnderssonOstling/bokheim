//! Subject browsing: taxonomy.

use crate::browse::{BrowseRow, LibraryFormatCount, LibraryLanguageCount, LibrarySubjectEntry, LibrarySubjectView, SubjectCacheSql, SubjectsSql, book_sort_query_value, read_book_card_row, read_facet_rows, retain_selected_facet_choices};
use crate::{Database, DatabaseError};
use library_model::{BrowseBookSort, BrowseChipSort, LibraryFileTypeFilter, SubjectPath};
use subject_projection::ROOT_SUBJECT_PATH;

mod cache;
mod compaction;
mod navigation;

impl Database {
    pub fn browse_library_subjects(&self, subject_path: Option<&str>, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySubjectView, DatabaseError> {
        let query = library_model::LibraryBrowseQuery {
            location: subject_path.unwrap_or(ROOT_SUBJECT_PATH).to_owned(),
            search: String::new(),
            file_types: vec![file_type],
            languages: vec![],
            chip_sort: BrowseChipSort::Alphabetical,
            book_sort: BrowseBookSort::Alphabetical,
            hide_finished: false,
            include_direct_child_books: false,
        };
        let contents = self.browse_subject_contents(&query, downloaded_only)?;
        let children = contents.children.into_iter().map(|row| LibrarySubjectEntry { id: SubjectPath::new(row.id), name: row.name, book_count: row.book_count, downloaded_book_count: row.downloaded_book_count }).collect();
        Ok(LibrarySubjectView { children, books: contents.books })
    }

    /// Complete subject page from one cache-validated snapshot.
    pub fn browse_subject(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<super::LibraryBrowseData, DatabaseError> {
        let options = super::BrowseOptions::from_query(query, downloaded_only);
        self.with_subject_cache(|| {
            let route = self.subject_route_id(&query.location)?.unwrap_or(-1);
            let contents = self.compact_subject_contents(route, &options)?;
            let (language_counts, format_counts) = self.subject_facets(route, &options)?;
            Ok(super::LibraryBrowseData { contents, chip_groups: Vec::new(), path: Vec::new(), language_counts, format_counts })
        })
    }

    /// Compact using full membership before applying the display filters.
    pub fn browse_subject_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        let options = super::BrowseOptions::from_query(query, downloaded_only);
        self.with_subject_cache(|| {
            let route = self.subject_route_id(&query.location)?.unwrap_or(-1);
            self.compact_subject_contents(route, &options)
        })
    }

    pub fn browse_subject_facet_counts(&self, subject_path: &str, query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str], downloaded_only: bool) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), DatabaseError> {
        let options = super::BrowseOptions::new(query, file_types, languages, downloaded_only);
        self.with_subject_cache(|| {
            let route = self.subject_route_id(subject_path)?.unwrap_or(-1);
            self.subject_facets(route, &options)
        })
    }

    fn subject_facets(&self, route: i64, options: &super::BrowseOptions) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), DatabaseError> {
        let (mut language_counts, mut format_counts) = read_facet_rows(|rows| self.connection.get_subject_facets(route, &options.search, options.file_types, &options.languages, i32::from(options.downloaded_only), rows))?;
        retain_selected_facet_choices(&mut language_counts, &mut format_counts, &options.languages, options.file_types);
        Ok((language_counts, format_counts))
    }
}

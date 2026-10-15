//! Flat browsing of every visible book in the library.

use std::collections::BTreeSet;

use library_model::{BrowseContents, BrowsePageRequest, LibraryBrowseQuery};

use super::subjects::Books;
use super::{BooksSql, BrowseOptions, LibraryBrowseData};
use crate::{Database, DatabaseError};

impl Database {
    pub(super) fn browse_all_books(&self, query: &LibraryBrowseQuery, downloaded_only: bool, page: Option<BrowsePageRequest>) -> Result<LibraryBrowseData, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let selected = self.all_selected_books(&options)?;
            let (books, sections, page) = self.load_section_cards(Books::Library, &selected, Vec::new(), &options, None, page)?;
            let contents = BrowseContents { children: Vec::new(), books };
            let (language_counts, format_counts) = self.library_facets(&options)?;
            Ok(LibraryBrowseData { contents, sections, path: Vec::new(), ancestors: Vec::new(), language_counts, format_counts, feed: None, page })
        })
    }

    pub(super) fn browse_all_books_contents(&self, query: &LibraryBrowseQuery, downloaded_only: bool) -> Result<BrowseContents, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| self.all_books_contents(&options))
    }

    fn all_books_contents(&self, options: &BrowseOptions) -> Result<BrowseContents, DatabaseError> {
        let selected = self.all_selected_books(options)?;
        let books = self.load_cards(&self.ordered_books(&selected, options)?)?;
        Ok(BrowseContents { children: Vec::new(), books })
    }

    fn all_selected_books(&self, options: &BrowseOptions) -> Result<BTreeSet<i64>, DatabaseError> {
        let mut candidates = BTreeSet::new();
        self.connection.all_book_ids(|row| {
            candidates.insert(row.get(0)?);
            Ok(())
        })?;
        let matching = self.matching_books(&candidates, options)?;
        Ok(if options.search.is_empty() { matching.books } else { matching.text_matches })
    }
}

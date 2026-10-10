//! Flat browsing of every visible book in the library.

use std::collections::BTreeSet;

use library_model::{BrowseContents, LibraryBrowseQuery};

use super::{BooksSql, BrowseOptions, LibraryBrowseData};
use crate::{Database, DatabaseError};

impl Database {
    pub(super) fn browse_all_books(&self, query: &LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let contents = self.all_books_contents(&options)?;
            let (language_counts, format_counts) = self.library_facets(&options)?;
            Ok(LibraryBrowseData { contents, sections: Vec::new(), path: Vec::new(), ancestors: Vec::new(), language_counts, format_counts, feed: None })
        })
    }

    pub(super) fn browse_all_books_contents(&self, query: &LibraryBrowseQuery, downloaded_only: bool) -> Result<BrowseContents, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| self.all_books_contents(&options))
    }

    fn all_books_contents(&self, options: &BrowseOptions) -> Result<BrowseContents, DatabaseError> {
        let mut candidates = BTreeSet::new();
        self.connection.all_book_ids(|row| {
            candidates.insert(row.get(0)?);
            Ok(())
        })?;
        let matching = self.matching_books(&candidates, options)?;
        let selected = if options.search.is_empty() { &matching.books } else { &matching.text_matches };
        let books = self.load_cards(&self.ordered_books(selected, options)?)?;
        Ok(BrowseContents { children: Vec::new(), books })
    }
}

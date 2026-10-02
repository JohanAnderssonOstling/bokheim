//! Filtering and ordering shared by browse pages. Each page picks candidate
//! books its own way; these apply the page's filters and book order to them.
use std::collections::BTreeSet;

use super::BooksSql;
use crate::{Database, DatabaseError};

/// The candidate books that pass the page's filters. The search is not one of
/// them: each page decides what a search matches, using `text_matches`.
pub(super) struct MatchingBooks {
    pub books: BTreeSet<i64>,
    pub downloaded: BTreeSet<i64>,
    /// Books whose own title or author contains the search. Empty when there
    /// is no search.
    pub text_matches: BTreeSet<i64>,
}

impl Database {
    pub(super) fn matching_books(&self, candidates: &BTreeSet<i64>, options: &super::BrowseOptions) -> Result<MatchingBooks, DatabaseError> {
        let mut matching = BTreeSet::new();
        let mut downloaded = BTreeSet::new();
        let mut text_matches = BTreeSet::new();
        let ids = serde_json::to_string(candidates).map_err(DatabaseError::operation)?;
        self.connection.matching_books(&ids, i32::from(options.downloaded_only), options.file_types, &options.languages, &options.search, |row| {
            if options.hide_finished && row.get::<_, Option<f32>>(3)?.unwrap_or_default() >= library_model::FINISHED_PROGRESS_THRESHOLD {
                return Ok(());
            }
            let book = row.get(0)?;
            matching.insert(book);
            if row.get::<_, bool>(1)? {
                downloaded.insert(book);
            }
            if !options.search.is_empty() && row.get::<_, bool>(2)? {
                text_matches.insert(book);
            }
            Ok(())
        })?;
        Ok(MatchingBooks { books: matching, downloaded, text_matches })
    }

    /// `books` in the page's book order.
    pub(super) fn ordered_books(&self, books: &BTreeSet<i64>, options: &super::BrowseOptions) -> Result<Vec<i64>, DatabaseError> {
        let mut ordered = Vec::with_capacity(books.len());
        if !books.is_empty() {
            let ids = serde_json::to_string(books).map_err(DatabaseError::operation)?;
            self.connection.ordered_books(
                &ids,
                {
                    use crate::browse::options::*;
                    let sort: BrowseBookSort = options.book_sort;

                    match sort {
                        BrowseBookSort::Alphabetical => 0,
                        BrowseBookSort::RecentlyRead => 1,
                    }
                },
                |row| {
                    ordered.push(row.get(0)?);
                    Ok(())
                },
            )?;
        }
        Ok(ordered)
    }
}

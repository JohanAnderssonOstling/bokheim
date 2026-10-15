//! Filtering and ordering shared by browse pages. Each page picks candidate
//! books its own way; these apply the page's filters and book order to them.
use std::collections::{BTreeMap, BTreeSet};

use super::{
    BooksSql,
    options::{BrowseBookSort, BrowseChipSort},
};
use crate::{Database, DatabaseError};

/// The candidate books that pass the page's filters. The search is not one of
/// them: each page decides what a search matches, using `text_matches`.
pub(super) struct MatchingBooks {
    pub books: BTreeSet<i64>,
    pub downloaded: BTreeSet<i64>,
    /// Books whose own title or author contains the search. Empty when there
    /// is no search.
    pub text_matches: BTreeSet<i64>,
    /// Only known scores for books that pass this page's filters.
    pub popularity: BTreeMap<i64, f64>,
    /// Filtered catalog books from the preferred acquisition source. Empty
    /// for personal-library browsing, where source preference does not apply.
    pub preferred: BTreeSet<i64>,
    /// Catalog acquisition source order; empty for personal-library browsing.
    pub source_priorities: BTreeMap<i64, u8>,
    /// Formats sharing one source popularity observation count once in totals.
    pub popularity_groups: BTreeMap<i64, String>,
}

impl Database {
    pub(super) fn matching_books(&self, candidates: &BTreeSet<i64>, options: &super::BrowseOptions) -> Result<MatchingBooks, DatabaseError> {
        let mut matching = BTreeSet::new();
        let mut downloaded = BTreeSet::new();
        let mut text_matches = BTreeSet::new();
        let mut popularity = BTreeMap::new();
        let ids = serde_json::to_string(candidates).map_err(DatabaseError::operation)?;
        // Book ordering reads the column directly in SQL. Only chip totals
        // need a per-book score map in Rust.
        self.connection.matching_books(&ids, i32::from(options.downloaded_only), options.file_types, &options.languages, &options.search, i32::from(options.chip_sort == BrowseChipSort::Popularity), |row| {
            if options.hide_finished && row.get::<_, Option<f32>>(3)?.unwrap_or_default() >= library_model::FINISHED_PROGRESS_THRESHOLD {
                return Ok(());
            }
            let book = row.get(0)?;
            if let Some(score) = row.get::<_, Option<f64>>(4)? {
                popularity.insert(book, score);
            }
            matching.insert(book);
            if row.get::<_, bool>(1)? {
                downloaded.insert(book);
            }
            if !options.search.is_empty() && row.get::<_, bool>(2)? {
                text_matches.insert(book);
            }
            Ok(())
        })?;
        Ok(MatchingBooks { books: matching, downloaded, text_matches, popularity, preferred: BTreeSet::new(), source_priorities: BTreeMap::new(), popularity_groups: BTreeMap::new() })
    }

    /// `books` in the page's book order.
    pub(super) fn ordered_books(&self, books: &BTreeSet<i64>, options: &super::BrowseOptions) -> Result<Vec<i64>, DatabaseError> {
        let mut ordered = Vec::with_capacity(books.len());
        if !books.is_empty() {
            let ids = serde_json::to_string(books).map_err(DatabaseError::operation)?;
            let sort = match options.book_sort {
                BrowseBookSort::Alphabetical | BrowseBookSort::DefaultOrder => 0,
                BrowseBookSort::RecentlyRead => 1,
                BrowseBookSort::Popularity => 2,
            };
            self.connection.ordered_books(&ids, sort, |row| {
                ordered.push(row.get(0)?);
                Ok(())
            })?;
        }
        Ok(ordered)
    }
}

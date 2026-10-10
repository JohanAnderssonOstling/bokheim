//! Author browsing: every credited name, each holding the books credited to it.

use crate::browse::books::MatchingBooks;
use crate::browse::{parse_uuid_column, AuthorsSql, BrowseOptions, BrowseRow};
use crate::{Database, DatabaseError};
use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
use library_model::{BrowseChipSort, BrowseLocation};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One credited identity and every book credited to it.
struct Author {
    name: String,
    books: BTreeSet<i64>,
}

impl Database {
    pub(super) fn browse_author(&self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<super::LibraryBrowseData, DatabaseError> {
        self.with_author_browse(author, query, downloaded_only, |authors, matching, options| {
            let contents = self.author_contents(author, authors, matching, options)?;
            // The index shows each author as a card of their books. A search
            // lists the matching names and books instead, as the other trees do.
            let sections = if author.is_none() && options.search.is_empty() {
                // Ordered and loaded once for the whole index, then dealt out to
                // each author in that order, rather than a query per author.
                let ordered = self.ordered_books(&matching.books, options)?;
                let position: HashMap<i64, usize> = ordered.iter().enumerate().map(|(index, book)| (*book, index)).collect();
                let cards = self.load_cards(&ordered)?;
                let mut sections = Vec::with_capacity(contents.children.len());
                for parent in &contents.children {
                    let BrowseLocation::Author(Some(id)) = parent.id else { continue };
                    let mut books = authors[&id].books.iter().filter_map(|book| position.get(book).copied()).collect::<Vec<_>>();
                    books.sort_unstable();
                    sections.push(library_model::BrowseSection { parent: parent.clone(), children: Vec::new(), books: books.into_iter().map(|index| cards[index].clone()).collect() });
                }
                sections
            } else {
                Vec::new()
            };
            let (language_counts, format_counts) = self.library_facets(options)?;
            let path = author.and_then(|id| authors.get(&id).map(|found| library_model::BrowsePathSegment { id: BrowseLocation::Author(Some(id)), name: found.name.clone() })).into_iter().collect();
            Ok(super::LibraryBrowseData { contents, sections, path, ancestors: Vec::new(), language_counts, format_counts, feed: None })
        })
    }

    pub(super) fn browse_author_contents(&self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        self.with_author_browse(author, query, downloaded_only, |authors, matching, options| self.author_contents(author, authors, matching, options))
    }

    /// Every author, and the books of the one asked for — or of all of them for
    /// the index — that pass the page's filters.
    fn with_author_browse<T>(
        &self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, read: impl FnOnce(&BTreeMap<AuthorId, Author>, &MatchingBooks, &BrowseOptions) -> Result<T, DatabaseError>,
    ) -> Result<T, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let mut authors = BTreeMap::<AuthorId, Author>::new();
            self.connection.author_books(AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                let id = parse_uuid_column(row.get(0)?, 0)?;
                let name: String = row.get(1)?;
                authors.entry(id).or_insert_with(|| Author { name, books: BTreeSet::new() }).books.insert(row.get(2)?);
                Ok(())
            })?;
            let candidates = match author {
                Some(id) => authors.get(&id).map(|found| found.books.clone()).unwrap_or_default(),
                None => authors.values().flat_map(|found| found.books.iter().copied()).collect(),
            };
            let matching = self.matching_books(&candidates, &options)?;
            read(&authors, &matching, &options)
        })
    }

    /// The index holds authors and, under a search, the books that match it; an
    /// author holds their books and nothing below them.
    fn author_contents(&self, author: Option<AuthorId>, authors: &BTreeMap<AuthorId, Author>, matching: &MatchingBooks, options: &BrowseOptions) -> Result<library_model::BrowseContents, DatabaseError> {
        let searched = !options.search.is_empty();
        let none = BTreeSet::new();
        let (children, selected) = match author {
            Some(_) => (Vec::new(), if searched { &matching.text_matches } else { &matching.books }),
            None => {
                let mut rows = Vec::new();
                for (id, found) in authors {
                    if searched && !book_model::normalize_search_text(&found.name).contains(&options.search) {
                        continue;
                    }
                    let book_count = found.books.intersection(&matching.books).count();
                    if book_count > 0 {
                        rows.push(BrowseRow { id: BrowseLocation::Author(Some(*id)), name: found.name.clone(), path: None, book_count, downloaded_book_count: found.books.intersection(&matching.downloaded).count() });
                    }
                }
                rows.sort_by(|left, right| {
                    let counts = if options.chip_sort == BrowseChipSort::BookCount { right.book_count.cmp(&left.book_count) } else { std::cmp::Ordering::Equal };
                    counts.then_with(|| book_model::normalize_search_text(&left.name).cmp(&book_model::normalize_search_text(&right.name))).then_with(|| left.name.cmp(&right.name)).then_with(|| left.id.cmp(&right.id))
                });
                (rows, if searched { &matching.text_matches } else { &none })
            }
        };
        let books = self.load_cards(&self.ordered_books(selected, options)?)?;
        Ok(library_model::BrowseContents { children, books })
    }
}

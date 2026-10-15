//! Home browsing: the shelves of recent activity — what is being read, what
//! is finished, and what was added lately.

use crate::browse::books::MatchingBooks;
use crate::browse::{BrowseOptions, BrowseRow, HomeSql};
use crate::{Database, DatabaseError};
use library_model::{BrowseBookSort, BrowseLocation, HomeShelf};
use std::collections::{HashMap, HashSet};

/// How many books a shelf holds.
const SHELF_BOOK_COUNT: i32 = 20;

pub(super) struct HomeShelfBooks {
    pub(super) shelf: HomeShelf,
    pub(super) books: Vec<i64>,
}

pub(super) struct HomeShelves {
    pub(super) shelves: Vec<HomeShelfBooks>,
    pub(super) matching: MatchingBooks,
}

impl Database {
    #[cfg(test)]
    pub(super) fn browse_home(&self, shelf: Option<HomeShelf>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<super::LibraryBrowseData, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let HomeShelves { shelves, matching } = self.home_shelves(&options)?;
            let contents = self.home_contents(shelf, &shelves, &matching, &options)?;
            // Home shows each shelf as a card of its books. A search lists the
            // matching books instead, as the other trees do.
            let sections = if shelf.is_none() && options.search.is_empty() {
                let ids = shelves.iter().flat_map(|shelf| shelf.books.iter().copied()).collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
                let cards = ids.iter().copied().zip(self.load_cards(&ids)?).collect::<HashMap<_, _>>();
                contents
                    .children
                    .iter()
                    .filter_map(|parent| {
                        let BrowseLocation::Home(Some(shelf)) = parent.id else { return None };
                        let books = shelves.iter().find(|found| found.shelf == shelf)?.books.iter().map(|book| cards[book].clone()).collect();
                        Some(library_model::BrowseSection { parent: parent.clone(), children: Vec::new(), books, book_count: None })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let (language_counts, format_counts) = self.library_facets(&options)?;
            let path = shelf.map(|shelf| library_model::BrowsePathSegment { id: BrowseLocation::Home(Some(shelf)), name: shelf.label().to_owned() }).into_iter().collect();
            Ok(super::LibraryBrowseData { contents, sections, path, ancestors: Vec::new(), language_counts, format_counts, feed: None, page: None })
        })
    }

    #[cfg(test)]
    pub(super) fn browse_home_contents(&self, shelf: Option<HomeShelf>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let HomeShelves { shelves, matching } = self.home_shelves(&options)?;
            self.home_contents(shelf, &shelves, &matching, &options)
        })
    }

    /// Every shelf with its books in shelf order, keeping only the books that
    /// pass the page's filters. A shelf with nothing left is still listed; it
    /// is the rows that leave it out.
    pub(super) fn home_shelves(&self, options: &BrowseOptions) -> Result<HomeShelves, DatabaseError> {
        let downloaded_only = i32::from(options.downloaded_only);
        // The most recently read books: up to a shelf's worth still in
        // progress, then up to a shelf's worth finished, each with whether it
        // is finished.
        let mut read = Vec::new();
        self.connection.recently_read_books(SHELF_BOOK_COUNT, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
            read.push((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?));
            Ok(())
        })?;
        let mut added = Vec::new();
        self.connection.recently_added_books(SHELF_BOOK_COUNT, downloaded_only, |row| {
            added.push(row.get::<_, i64>(0)?);
            Ok(())
        })?;
        // A book the reader has opened is recently added as well, and saying so
        // twice on one screen wastes the shelf that exists to surface what they
        // have not seen yet.
        let opened = read.iter().map(|(book, _)| *book).collect::<HashSet<_>>();
        added.retain(|book| !opened.contains(book));

        let candidates = read.iter().map(|(book, _)| *book).chain(added.iter().copied()).collect();
        let matching = self.matching_books(&candidates, options)?;
        let passes = |book: &i64| matching.books.contains(book) && (options.search.is_empty() || matching.text_matches.contains(book));
        let shelf = |finished: bool| read.iter().filter(|(book, done)| *done == finished && passes(book)).map(|(book, _)| *book).collect::<Vec<_>>();
        let mut shelves = vec![
            HomeShelfBooks { shelf: HomeShelf::RecentlyRead, books: shelf(false) },
            HomeShelfBooks { shelf: HomeShelf::Finished, books: shelf(true) },
            HomeShelfBooks { shelf: HomeShelf::RecentlyAdded, books: added.into_iter().filter(|book| passes(book)).collect() },
        ];
        if options.book_sort != BrowseBookSort::DefaultOrder {
            for shelf in &mut shelves {
                shelf.books = self.ordered_books(&shelf.books.iter().copied().collect(), options)?;
            }
        }
        let mut rows = shelves.iter().map(|shelf| BrowseRow { id: BrowseLocation::Home(Some(shelf.shelf)), name: shelf.shelf.label().to_owned(), path: None, book_count: shelf.books.len(), downloaded_book_count: 0 }).collect::<Vec<_>>();
        super::popularity::sort_rows(&mut rows, options.chip_sort, |row| {
            shelves.iter().find(|shelf| row.id == BrowseLocation::Home(Some(shelf.shelf))).and_then(|shelf| super::popularity::sum(&shelf.books.iter().copied().collect(), &matching))
        });
        let positions: HashMap<_, _> = rows.iter().enumerate().map(|(index, row)| (row.id, index)).collect();
        shelves.sort_by_key(|shelf| positions[&BrowseLocation::Home(Some(shelf.shelf))]);
        Ok(HomeShelves { shelves, matching })
    }

    /// Home holds its shelves, or under a search the books on them that match
    /// it; a shelf holds its books and nothing below them.
    #[cfg(test)]
    fn home_contents(&self, shelf: Option<HomeShelf>, shelves: &[HomeShelfBooks], matching: &MatchingBooks, options: &BrowseOptions) -> Result<library_model::BrowseContents, DatabaseError> {
        let (children, mut books) = match shelf {
            Some(shelf) => (Vec::new(), shelves.iter().find(|found| found.shelf == shelf).map(|shelf| shelf.books.clone()).unwrap_or_default()),
            None if options.search.is_empty() => {
                let rows = shelves
                    .iter()
                    .filter(|shelf| !shelf.books.is_empty())
                    .map(|shelf| BrowseRow {
                        id: BrowseLocation::Home(Some(shelf.shelf)),
                        name: shelf.shelf.label().to_owned(),
                        path: None,
                        book_count: shelf.books.len(),
                        downloaded_book_count: shelf.books.iter().filter(|book| matching.downloaded.contains(book)).count(),
                    })
                    .collect();
                (rows, Vec::new())
            }
            None => {
                let mut seen = HashSet::new();
                (Vec::new(), shelves.iter().flat_map(|shelf| shelf.books.iter().copied()).filter(|book| seen.insert(*book)).collect())
            }
        };
        if options.book_sort != BrowseBookSort::DefaultOrder {
            books = self.ordered_books(&books.iter().copied().collect(), options)?;
        }
        Ok(library_model::BrowseContents { children, books: self.load_cards(&books)? })
    }
}

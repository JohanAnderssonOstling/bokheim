//! Author browsing: every credited name, each holding the books credited to it.

use crate::browse::books::MatchingBooks;
use crate::browse::sections::SectionSelection;
use crate::browse::subjects::Books;
use crate::browse::{AuthorsSql, BrowseOptions, BrowseRow, parse_uuid_column};
use crate::{Database, DatabaseError};
use book_model::{AUTHOR_MARC_RELATOR_CODE, AuthorId};
use library_model::{BrowseContents, BrowseHeadingState, BrowseLocation, BrowsePageRequest};
use std::collections::{BTreeMap, BTreeSet};

/// One credited identity and every book credited to it.
pub(super) struct Author {
    pub(super) name: String,
    pub(super) books: BTreeSet<i64>,
}

/// The author index's rows: every author with a matching book, or under a
/// search every name that matches, ordered by the chosen sort and then by
/// name. `location` is where a row leads.
pub(super) fn index_rows(authors: &BTreeMap<AuthorId, Author>, matching: &MatchingBooks, options: &BrowseOptions, location: fn(AuthorId) -> BrowseLocation) -> Vec<BrowseRow> {
    let searched = !options.search.is_empty();
    let mut rows = Vec::new();
    for (id, found) in authors {
        if searched && !book_model::normalize_search_text(&found.name).contains(&options.search) {
            continue;
        }
        let book_count = found.books.intersection(&matching.books).count();
        if book_count > 0 {
            rows.push(BrowseRow { id: location(*id), name: found.name.clone(), path: None, book_count, downloaded_book_count: found.books.intersection(&matching.downloaded).count() });
        }
    }
    super::popularity::sort_rows(&mut rows, options.chip_sort, |row| match row.id {
        BrowseLocation::Author(Some(id)) | BrowseLocation::CatalogAuthor(Some(id)) => super::popularity::sum(&authors[&id].books, matching),
        _ => None,
    });
    if options.chip_sort == library_model::BrowseChipSort::Popularity && !matching.source_priorities.is_empty() {
        rows.sort_by_key(|row| match row.id {
            BrowseLocation::CatalogAuthor(Some(id)) => super::popularity::source_priority(&authors[&id].books, matching),
            _ => None,
        });
    }
    if !matching.preferred.is_empty() {
        rows.sort_by_key(|row| match row.id {
            BrowseLocation::CatalogAuthor(Some(id)) => authors[&id].books.is_disjoint(&matching.preferred),
            _ => true,
        });
    }
    rows
}

/// Only the requested author names cross the transport. The digest covers
/// the complete ordered list so windows from different revisions cannot mix.
pub(super) fn page_author_rows(rows: &mut Vec<BrowseRow>, page: Option<BrowsePageRequest>, cards: bool) -> Option<BrowseHeadingState> {
    let page = page?;
    let total = rows.len();
    let mut revision = blake3::Hasher::new();
    for row in rows.iter() {
        if let BrowseLocation::Author(Some(id)) | BrowseLocation::CatalogAuthor(Some(id)) = row.id {
            revision.update(id.as_bytes());
        }
        revision.update(&(row.name.len() as u64).to_le_bytes());
        revision.update(row.name.as_bytes());
        revision.update(&(row.book_count as u64).to_le_bytes());
        revision.update(&(row.downloaded_book_count as u64).to_le_bytes());
    }
    *rows = rows.drain(..).skip(page.section_offset).take(page.section_limit).collect();
    Some(BrowseHeadingState { total, revision: revision.finalize().to_hex().to_string(), cards })
}

impl Database {
    pub(super) fn browse_author(
        &self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, section_limit: Option<usize>, page: Option<BrowsePageRequest>,
    ) -> Result<super::LibraryBrowseData, DatabaseError> {
        self.with_author_browse(author, query, downloaded_only, |authors, matching, options| {
            let searched = !options.search.is_empty();
            let mut children = if author.is_none() { index_rows(authors, matching, options, |id| BrowseLocation::Author(Some(id))) } else { Vec::new() };
            let headings = if author.is_none() { page_author_rows(&mut children, page, !searched) } else { None };
            let direct = if author.is_some() || searched { if searched { matching.text_matches.clone() } else { matching.books.clone() } } else { BTreeSet::new() };
            let selections = if author.is_none() && !searched {
                children
                    .iter()
                    .filter_map(|parent| {
                        let BrowseLocation::Author(Some(id)) = parent.id else { return None };
                        Some(SectionSelection { parent: parent.clone(), children: Vec::new(), books: authors[&id].books.intersection(&matching.books).copied().collect() })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let card_page = if headings.is_some() { page.map(|page| BrowsePageRequest { section_offset: 0, ..page }) } else { page };
            let (books, sections, mut state) = self.load_section_cards(Books::Library, &direct, selections, options, section_limit, card_page)?;
            if let (Some(state), Some(request)) = (&mut state, page) {
                state.section_offset = request.section_offset;
                state.headings = headings;
            }
            let page = state;
            let contents = BrowseContents { children, books };
            let (language_counts, format_counts) = self.library_facets(options)?;
            let path = author.and_then(|id| authors.get(&id).map(|found| library_model::BrowsePathSegment { id: BrowseLocation::Author(Some(id)), name: found.name.clone() })).into_iter().collect();
            Ok(super::LibraryBrowseData { contents, sections, path, ancestors: Vec::new(), language_counts, format_counts, feed: None, page })
        })
    }

    pub(super) fn browse_author_contents(&self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        self.with_author_browse(author, query, downloaded_only, |authors, matching, options| self.author_contents(author, authors, matching, options))
    }

    pub(super) fn browse_author_book_page(&self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, offset: usize, limit: usize) -> Result<library_model::BrowseBookPage, DatabaseError> {
        self.with_author_browse(author, query, downloaded_only, |_, matching, options| {
            let selected = if options.search.is_empty() { &matching.books } else { &matching.text_matches };
            self.load_source_book_page(Books::Library, selected, options, offset, limit)
        })
    }

    /// Every author, and the books of the one asked for — or of all of them for
    /// the index — that pass the page's filters.
    fn with_author_browse<T>(
        &self, author: Option<AuthorId>, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, read: impl FnOnce(&BTreeMap<AuthorId, Author>, &MatchingBooks, &BrowseOptions) -> Result<T, DatabaseError>,
    ) -> Result<T, DatabaseError> {
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let authors = self.library_browse_authors()?;
            let candidates = match author {
                Some(id) => authors.get(&id).map(|found| found.books.clone()).unwrap_or_default(),
                None => authors.values().flat_map(|found| found.books.iter().copied()).collect(),
            };
            let matching = self.matching_books(&candidates, &options)?;
            read(&authors, &matching, &options)
        })
    }

    pub(super) fn library_browse_authors(&self) -> Result<BTreeMap<AuthorId, Author>, DatabaseError> {
        let mut authors = BTreeMap::<AuthorId, Author>::new();
        self.connection.author_books(AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
            let id = parse_uuid_column(row.get(0)?, 0)?;
            let name: String = row.get(1)?;
            authors.entry(id).or_insert_with(|| Author { name, books: BTreeSet::new() }).books.insert(row.get(2)?);
            Ok(())
        })?;
        Ok(authors)
    }

    /// The index holds authors and, under a search, the books that match it; an
    /// author holds their books and nothing below them.
    fn author_contents(&self, author: Option<AuthorId>, authors: &BTreeMap<AuthorId, Author>, matching: &MatchingBooks, options: &BrowseOptions) -> Result<library_model::BrowseContents, DatabaseError> {
        let searched = !options.search.is_empty();
        let none = BTreeSet::new();
        let (children, selected) = match author {
            Some(_) => (Vec::new(), if searched { &matching.text_matches } else { &matching.books }),
            None => (index_rows(authors, matching, options, |id| BrowseLocation::Author(Some(id))), if searched { &matching.text_matches } else { &none }),
        };
        let books = self.load_cards(&self.ordered_books(selected, options)?)?;
        Ok(library_model::BrowseContents { children, books })
    }
}

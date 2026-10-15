//! One owner for a filtered browse selection, its totals and its ordering.
//! Navigation and source records remain separate; only page cards are hydrated.
use super::books::MatchingBooks;
use super::profile::Stage;
use super::sections::SectionSelection;
use super::subjects::Books;
use super::{BrowseOptions, LibraryBrowseData};
use crate::{Database, DatabaseError};
use library_model::{
    BROWSE_SECTION_BOOK_LIMIT, BookCardRow, BrowseBookPage, BrowseContents, BrowseHeadingState, BrowseLocation, BrowsePageRequest, BrowsePageState, BrowsePathSegment, BrowseRow, BrowseSection, LibraryBrowseQuery,
    MAX_BROWSE_SECTION_BOOK_LIMIT,
};
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap};
use std::mem::size_of;
use std::sync::{Arc, Mutex};
use sync_common::DirId;
mod cache;
mod key;
mod sources;
pub(crate) use cache::BrowseResults;
use key::{Dependencies, ResultKey};

#[derive(Clone, Copy)]
struct Item {
    book: i64,
    directory: Option<DirId>,
}

impl From<i64> for Item {
    fn from(book: i64) -> Self {
        Self { book, directory: None }
    }
}

impl Item {
    fn card(&self, cards: &HashMap<i64, BookCardRow>) -> BookCardRow {
        let mut card = cards[&self.book].clone();
        card.source_directory = self.directory;
        card
    }
}

type Orders = HashMap<BrowseLocation, Arc<Vec<Item>>>;

struct Selection {
    metadata: LibraryBrowseData,
    sections: Vec<SectionSelection>,
    books: SelectedBooks,
}

enum SelectedBooks {
    Membership { eligible: BTreeSet<i64>, direct: BTreeSet<i64> },
    Ordered { direct: Vec<Item>, section_orders: Orders },
}

struct BrowseResult {
    location: BrowseLocation,
    revision: String,
    options: BrowseOptions,
    metadata: LibraryBrowseData,
    direct: Vec<Item>,
    sections: Vec<SectionSelection>,
    positions: HashMap<i64, usize>,
    section_orders: Mutex<Orders>,
}

enum Projection {
    Initial(BrowsePageRequest),
    Continuation(BrowsePageRequest),
    Complete { section_books: Option<usize>, sections: bool },
}

impl BrowseResult {
    fn prepare(db: &Database, location: BrowseLocation, options: BrowseOptions, catalog: Option<&Connection>, revision: String) -> Result<Self, DatabaseError> {
        let selection = db.prepare_browse_selection(location, &options, catalog)?;
        let (direct, positions, section_orders) = match selection.books {
            SelectedBooks::Ordered { direct, section_orders } => (direct, HashMap::new(), section_orders),
            SelectedBooks::Membership { eligible, direct } => {
                let _stage = Stage::new("eligible book ordering");
                let ordered = match catalog {
                    Some(catalog) => super::catalog::ordered_books_in(catalog, &eligible, &options)?,
                    None => db.ordered_books(&eligible, &options)?,
                };
                if ordered.len() != eligible.len() {
                    return Err(DatabaseError::message("browse data changed while preparing ordering"));
                }
                let positions = ordered.iter().enumerate().map(|(position, &id)| (id, position)).collect();
                let direct = ordered.into_iter().filter(|id| direct.contains(id)).map(Item::from).collect();
                (direct, positions, Orders::new())
            }
        };
        Ok(Self { location, revision, options, metadata: selection.metadata, direct, sections: selection.sections, positions, section_orders: Mutex::new(section_orders) })
    }

    fn author_index(&self) -> bool {
        matches!(self.location, BrowseLocation::Author(None) | BrowseLocation::CatalogAuthor(None))
    }

    fn section_order(&self, db: &Database, catalog: Option<&Connection>, location: BrowseLocation) -> Result<Option<Arc<Vec<Item>>>, DatabaseError> {
        if self.author_index() {
            if !self.options.search.is_empty() {
                return Ok(None);
            }
            if !self.metadata.contents.children.iter().any(|row| row.id == location) {
                return Ok(None);
            }
            self.prepare_author_orders(db, catalog, &[location])?;
        }
        let mut orders = self.section_orders.lock().map_err(|_| DatabaseError::message("browse ordering lock is poisoned"))?;
        if let Some(order) = orders.get(&location) {
            return Ok(Some(Arc::clone(order)));
        }
        let Some(section) = self.sections.iter().find(|section| section.parent.id == location) else { return Ok(None) };
        let mut order = section.books.iter().copied().collect::<Vec<_>>();
        order.sort_unstable_by_key(|id| self.positions[id]);
        let order = Arc::new(order.into_iter().map(Item::from).collect());
        orders.insert(location, Arc::clone(&order));
        Ok(Some(order))
    }

    fn prepare_author_orders(&self, db: &Database, catalog: Option<&Connection>, locations: &[BrowseLocation]) -> Result<(), DatabaseError> {
        let mut orders = self.section_orders.lock().map_err(|_| DatabaseError::message("browse ordering lock is poisoned"))?;
        let missing = locations
            .iter()
            .filter(|location| !orders.contains_key(location))
            .filter_map(|location| match location {
                BrowseLocation::Author(Some(id)) | BrowseLocation::CatalogAuthor(Some(id)) => Some(*id),
                _ => None,
            })
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        let _stage = Stage::new("requested author ordering");
        for (id, books) in db.author_query(catalog, &self.options)?.orders(&missing)? {
            let location = if catalog.is_some() { BrowseLocation::CatalogAuthor(Some(id)) } else { BrowseLocation::Author(Some(id)) };
            let expected = self.metadata.contents.children.iter().find(|row| row.id == location).map(|row| row.book_count);
            if expected != Some(books.len()) {
                return Err(DatabaseError::message("author membership changed while preparing ordering"));
            }
            orders.insert(location, Arc::new(books.into_iter().map(Item::from).collect()));
        }
        Ok(())
    }

    fn estimated_bytes(&self) -> usize {
        let row_bytes = |row: &BrowseRow| size_of::<BrowseRow>() + row.name.len() + row.path.as_ref().map_or(0, String::len);
        size_of::<Self>()
            + self.revision.len()
            + 2 * (self.options.search.len() + self.options.languages.len())
            + self.section_orders.lock().map(|orders| orders.capacity() * 64 + orders.values().map(|ids| ids.capacity() * size_of::<Item>() + size_of::<Vec<Item>>()).sum::<usize>()).unwrap_or_default()
            + self.positions.capacity() * 48
            + self.direct.capacity() * size_of::<Item>()
            + self.metadata.contents.children.iter().map(row_bytes).sum::<usize>()
            + self.metadata.ancestors.iter().flat_map(|(_, rows)| rows).map(row_bytes).sum::<usize>()
            + self.metadata.path.iter().map(|segment| size_of::<BrowsePathSegment>() + segment.name.len()).sum::<usize>()
            + self.metadata.language_counts.iter().map(|count| size_of::<library_model::LibraryLanguageCount>() + count.language.len()).sum::<usize>()
            + self.metadata.format_counts.len() * size_of::<library_model::LibraryFormatCount>()
            + self.sections.iter().map(|section| size_of::<SectionSelection>() + row_bytes(&section.parent) + section.children.iter().map(row_bytes).sum::<usize>() + section.books.len() * 64).sum::<usize>()
    }

    fn project(&self, db: &Database, catalog: Option<&Connection>, projection: Projection) -> Result<LibraryBrowseData, DatabaseError> {
        match projection {
            Projection::Initial(request) => {
                let mut response = self.project_page(db, catalog, request)?;
                if !self.author_index() {
                    response.contents.children = self.metadata.contents.children.clone();
                }
                self.include_metadata(&mut response);
                Ok(response)
            }
            Projection::Continuation(request) => self.project_page(db, catalog, request),
            Projection::Complete { section_books, sections } => {
                let request = BrowsePageRequest { section_offset: 0, section_limit: if sections { usize::MAX } else { 0 }, book_offset: 0, book_limit: usize::MAX };
                let mut response = self.load_window(db, catalog, request, section_books)?;
                response.contents.children = self.metadata.contents.children.clone();
                self.include_metadata(&mut response);
                Ok(response)
            }
        }
    }

    fn include_metadata(&self, response: &mut LibraryBrowseData) {
        response.path = self.metadata.path.clone();
        response.ancestors = self.metadata.ancestors.clone();
        response.language_counts = self.metadata.language_counts.clone();
        response.format_counts = self.metadata.format_counts.clone();
    }

    fn project_page(&self, db: &Database, catalog: Option<&Connection>, request: BrowsePageRequest) -> Result<LibraryBrowseData, DatabaseError> {
        let request = BrowsePageRequest { section_limit: request.section_limit.min(64), book_limit: request.book_limit.min(MAX_BROWSE_SECTION_BOOK_LIMIT), ..request };
        let mut response = self.load_window(db, catalog, request, Some(BROWSE_SECTION_BOOK_LIMIT))?;
        let headings = if self.author_index() {
            response.contents.children = self.metadata.contents.children.iter().skip(request.section_offset).take(request.section_limit).cloned().collect();
            Some(BrowseHeadingState { total: self.metadata.contents.children.len(), revision: self.revision.clone(), cards: self.options.search.is_empty() })
        } else {
            None
        };
        let section_ids = if self.author_index() { response.sections.iter().map(|section| section.parent.id).collect() } else { self.sections.iter().map(|section| section.parent.id).collect() };
        response.page = Some(BrowsePageState { section_ids, section_offset: request.section_offset, book_offset: request.book_offset, book_total: self.direct.len(), headings, result_revision: Some(self.revision.clone()) });
        Ok(response)
    }

    /// Both projection modes batch-hydrate the same selected card identities.
    fn load_window(&self, db: &Database, catalog: Option<&Connection>, request: BrowsePageRequest, section_books: Option<usize>) -> Result<LibraryBrowseData, DatabaseError> {
        let sections = if self.author_index() && self.options.search.is_empty() {
            let parents = self.metadata.contents.children.iter().skip(request.section_offset).take(request.section_limit).collect::<Vec<_>>();
            self.prepare_author_orders(db, catalog, &parents.iter().map(|parent| parent.id).collect::<Vec<_>>())?;
            parents.into_iter().map(|parent| (parent, &[][..])).collect::<Vec<_>>()
        } else {
            self.sections.iter().skip(request.section_offset).take(request.section_limit).map(|section| (&section.parent, section.children.as_slice())).collect::<Vec<_>>()
        };
        let direct = self.direct.iter().copied().skip(request.book_offset).take(request.book_limit).collect::<Vec<_>>();
        let mut loaded = direct.iter().map(|item| item.book).collect::<BTreeSet<_>>();
        let mut plans = Vec::with_capacity(sections.len());
        for (parent, children) in sections {
            let order = self.section_order(db, catalog, parent.id)?.ok_or_else(|| DatabaseError::message("selected section is missing"))?;
            let ids = order.iter().copied().take(section_books.unwrap_or(usize::MAX)).collect::<Vec<_>>();
            loaded.extend(ids.iter().map(|item| item.book));
            let section = BrowseSection { parent: parent.clone(), children: children.to_vec(), books: Vec::new(), book_count: section_books.map(|_| order.len()) };
            plans.push((section, ids));
        }
        let cards = {
            let _stage = Stage::new("page card hydration");
            db.result_cards(catalog, &loaded, &self.options)?
        };
        Ok(LibraryBrowseData {
            contents: BrowseContents { children: Vec::new(), books: direct.iter().map(|item| item.card(&cards)).collect() },
            sections: plans
                .into_iter()
                .map(|(mut section, ids)| {
                    section.books = ids.iter().map(|item| item.card(&cards)).collect();
                    section
                })
                .collect(),
            ..LibraryBrowseData::default()
        })
    }

    fn books(&self, db: &Database, catalog: Option<&Connection>, location: BrowseLocation, offset: usize, limit: usize) -> Result<BrowseBookPage, DatabaseError> {
        let section;
        let order = if location == self.location {
            &self.direct
        } else {
            section = self.section_order(db, catalog, location)?.ok_or_else(|| DatabaseError::message("section does not belong to this browse result"))?;
            &section
        };
        let ids = order.iter().copied().skip(offset).take(limit.clamp(1, MAX_BROWSE_SECTION_BOOK_LIMIT)).collect::<Vec<_>>();
        let cards = db.result_cards(catalog, &ids.iter().map(|item| item.book).collect(), &self.options)?;
        Ok(BrowseBookPage { offset, total: order.len(), books: ids.iter().map(|item| item.card(&cards)).collect() })
    }
}

impl Database {
    pub(super) fn browse_result_initial(&self, query: &LibraryBrowseQuery, downloaded_only: bool, page: BrowsePageRequest) -> Result<LibraryBrowseData, DatabaseError> {
        self.with_browse_result(query, downloaded_only, None, |result, catalog| result.project(self, catalog, Projection::Initial(page)))?.ok_or_else(|| DatabaseError::message("initial browse result is unavailable"))
    }

    pub(super) fn browse_result_complete(&self, query: &LibraryBrowseQuery, downloaded_only: bool, section_books: Option<usize>, sections: bool) -> Result<LibraryBrowseData, DatabaseError> {
        self.with_browse_result(query, downloaded_only, None, |result, catalog| result.project(self, catalog, Projection::Complete { section_books, sections }))?.ok_or_else(|| DatabaseError::message("complete browse result is unavailable"))
    }

    pub(super) fn browse_result_legacy_books(&self, query: &LibraryBrowseQuery, downloaded_only: bool, offset: usize, limit: usize) -> Result<BrowseBookPage, DatabaseError> {
        self.with_browse_result(query, downloaded_only, None, |result, catalog| {
            if matches!(query.location, BrowseLocation::Author(None) | BrowseLocation::CatalogAuthor(None)) {
                let ordered = self.author_query(catalog, &result.options)?.ordered(None)?;
                let selected = ordered.iter().copied().skip(offset).take(limit.clamp(1, MAX_BROWSE_SECTION_BOOK_LIMIT)).collect::<Vec<_>>();
                let cards = self.result_cards(catalog, &selected.iter().copied().collect(), &result.options)?;
                Ok(BrowseBookPage { offset, total: ordered.len(), books: selected.iter().map(|id| cards[id].clone()).collect() })
            } else {
                result.books(self, catalog, query.location, offset, limit)
            }
        })?
        .ok_or_else(|| DatabaseError::message("browse book result is unavailable"))
    }

    /// None means the caller must replace its installed result before paging.
    pub fn browse_result_page(&self, query: &LibraryBrowseQuery, downloaded_only: bool, revision: &str, page: BrowsePageRequest) -> Result<Option<LibraryBrowseData>, DatabaseError> {
        self.with_browse_result(query, downloaded_only, Some(revision), |result, catalog| result.project(self, catalog, Projection::Continuation(page)))
    }

    pub fn browse_result_books(&self, query: &LibraryBrowseQuery, downloaded_only: bool, revision: &str, section: BrowseLocation, offset: usize, limit: usize) -> Result<Option<BrowseBookPage>, DatabaseError> {
        self.with_browse_result(query, downloaded_only, Some(revision), |result, catalog| result.books(self, catalog, section, offset, limit))
    }

    fn with_browse_result<T>(&self, query: &LibraryBrowseQuery, downloaded_only: bool, expected: Option<&str>, read: impl FnOnce(&BrowseResult, Option<&Connection>) -> Result<T, DatabaseError>) -> Result<Option<T>, DatabaseError> {
        let dependencies = Dependencies::for_location(query.location)?;
        let operation = |catalog: Option<&Connection>, generation| {
            self.with_browse_snapshot(|| {
                let key = dependencies.read_key(self, query, downloaded_only, catalog, generation)?;
                let revision = self.browse_results.borrow().revision(&key)?;
                if expected.is_some_and(|expected| expected != revision) {
                    return Ok(None);
                }
                let result = self.prepared_browse_result(key, catalog, revision)?;
                let response = read(&result, catalog).map(Some);
                self.browse_results.borrow_mut().trim();
                response
            })
        };
        if dependencies.catalog { crate::catalog::read_snapshot(|catalog, generation| operation(Some(catalog), generation)) } else { operation(None, 0) }
    }

    fn prepared_browse_result(&self, key: ResultKey, catalog: Option<&Connection>, revision: String) -> Result<Arc<BrowseResult>, DatabaseError> {
        let cached = self.browse_results.borrow_mut().get(&key);
        if let Some(result) = cached {
            return Ok(result);
        }
        let result = Arc::new(BrowseResult::prepare(self, key.location, key.options.clone(), catalog, revision)?);
        self.browse_results.borrow_mut().insert(key, Arc::clone(&result));
        Ok(result)
    }

    fn result_matching(&self, catalog: Option<&Connection>, candidates: &BTreeSet<i64>, options: &BrowseOptions) -> Result<MatchingBooks, DatabaseError> {
        match catalog {
            Some(catalog) => super::catalog::matching_in(catalog, candidates, &self.library_book_hashes()?, options),
            None => self.matching_books(candidates, options),
        }
    }

    fn result_cards(&self, catalog: Option<&Connection>, selected: &BTreeSet<i64>, options: &BrowseOptions) -> Result<HashMap<i64, BookCardRow>, DatabaseError> {
        let cards: HashMap<_, _> = match catalog {
            Some(catalog) => super::catalog::card_rows_in(catalog, selected, options)?.into_iter().collect(),
            None => {
                let ids = selected.iter().copied().collect::<Vec<_>>();
                ids.iter().copied().zip(self.load_cards(&ids)?).collect()
            }
        };
        if cards.len() != selected.len() {
            return Err(DatabaseError::message("browse data changed while hydrating cards"));
        }
        Ok(cards)
    }
}

#[cfg(test)]
mod tests;

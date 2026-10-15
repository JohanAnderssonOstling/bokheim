//! Library browse pages, hierarchy projections, home, and authors.

use crate::DatabaseError;
use crate::SqlParam;
use content_address::ContentHash;
#[cfg(test)]
mod all_books;
mod author_query;
mod books;
mod cards;
mod catalog;
pub(crate) mod facets;
pub(crate) mod options;
mod popularity;
mod profile;
pub(crate) mod result;
pub(crate) mod revision;
pub mod rows;
mod sections;
mod snapshot;
use options::BrowseOptions;
use rows::{invalid_integer_column, invalid_text_column, parse_uuid_column};
#[cfg(test)]
pub(crate) mod authors;
pub(crate) mod folders;
pub(crate) mod home;
pub(crate) mod library;
pub(crate) mod subjects;
pub(crate) mod text;

use include_sqlite_sql::include_sql;

use library_model::{BrowsePageRequest, BrowsePageState, BrowseRow};

/// Complete library-owned browse projection returned to any UI transport.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct LibraryBrowseData {
    #[serde(default)]
    pub sections: Vec<library_model::BrowseSection>,
    pub contents: library_model::BrowseContents,
    pub path: Vec<library_model::BrowsePathSegment>,
    /// The children of the root and of every location on `path` above this
    /// one, with this page's filters but not its search, for drawing the way
    /// here. Empty for kinds that are not trees.
    #[serde(default)]
    pub ancestors: Vec<(library_model::BrowseLocation, Vec<BrowseRow>)>,
    pub language_counts: Vec<library_model::LibraryLanguageCount>,
    pub format_counts: Vec<library_model::LibraryFormatCount>,
    /// Set only for a page of an OPDS feed, which the library never holds.
    #[serde(default)]
    pub feed: Option<library_model::FeedPage>,
    #[serde(default)]
    pub page: Option<BrowsePageState>,
}

impl crate::Database {
    /// Complete projection and paged UI reads share the same selection semantics.
    pub fn browse(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, DatabaseError> {
        self.browse_result_complete(query, downloaded_only, None, true)
    }

    pub fn browse_sections(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, book_limit: usize) -> Result<LibraryBrowseData, DatabaseError> {
        self.browse_result_complete(query, downloaded_only, Some(book_limit.clamp(1, library_model::MAX_BROWSE_SECTION_BOOK_LIMIT)), true)
    }

    /// Complete browse page, including facets and breadcrumbs, from one snapshot.
    #[cfg(test)]
    pub(crate) fn legacy_browse(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, DatabaseError> {
        match query.location {
            library_model::BrowseLocation::Books => self.browse_all_books(query, downloaded_only, None),
            library_model::BrowseLocation::Folder(directory) => self.browse_folder(directory, query, downloaded_only),
            library_model::BrowseLocation::Subject(route) => self.browse_subject(subjects::Books::Library, route, query, downloaded_only, None, None),
            library_model::BrowseLocation::Catalog(route) => self.browse_subject(subjects::Books::Catalog, route, query, downloaded_only, None, None),
            library_model::BrowseLocation::CatalogAuthor(author) => self.browse_catalog_author(author, query, downloaded_only, None, None),
            library_model::BrowseLocation::Author(author) => self.browse_author(author, query, downloaded_only, None, None),
            library_model::BrowseLocation::Home(shelf) => self.browse_home(shelf, query, downloaded_only),
            library_model::BrowseLocation::Opds { .. } => Err(DatabaseError::message("an OPDS feed is browsed over the network, not in the library")),
            library_model::BrowseLocation::Store(_) => Err(DatabaseError::message("a store opens in its own window and is not browsed")),
        }
    }

    /// Initial covers for every subject or author section are loaded together.
    /// Recursive operations continue to use the complete `browse_contents`.
    #[cfg(test)]
    pub(crate) fn legacy_browse_sections(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, book_limit: usize) -> Result<LibraryBrowseData, DatabaseError> {
        let limit = Some(book_limit.clamp(1, library_model::MAX_BROWSE_SECTION_BOOK_LIMIT));
        match query.location {
            library_model::BrowseLocation::Subject(route) => self.browse_subject(subjects::Books::Library, route, query, downloaded_only, limit, None),
            library_model::BrowseLocation::Catalog(route) => self.browse_subject(subjects::Books::Catalog, route, query, downloaded_only, limit, None),
            library_model::BrowseLocation::Author(author) => self.browse_author(author, query, downloaded_only, limit, None),
            library_model::BrowseLocation::CatalogAuthor(author) => self.browse_catalog_author(author, query, downloaded_only, limit, None),
            _ => self.legacy_browse(query, downloaded_only),
        }
    }

    /// Loads only requested section panels and direct covers. Names and totals
    /// remain complete for layout, navigation, and restoring distant pages.
    pub fn browse_page(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, page: BrowsePageRequest) -> Result<LibraryBrowseData, DatabaseError> {
        match query.location {
            library_model::BrowseLocation::Books
            | library_model::BrowseLocation::Subject(_)
            | library_model::BrowseLocation::Catalog(_)
            | library_model::BrowseLocation::Author(_)
            | library_model::BrowseLocation::CatalogAuthor(_)
            | library_model::BrowseLocation::Folder(_)
            | library_model::BrowseLocation::Home(_) => self.browse_result_initial(query, downloaded_only, page),
            _ => self.browse(query, downloaded_only),
        }
    }

    pub fn browse_section_books(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, offset: usize, limit: usize) -> Result<library_model::BrowseBookPage, DatabaseError> {
        self.browse_result_legacy_books(query, downloaded_only, offset, limit)
    }

    /// Complete direct identities and navigation for recursive operations.
    pub fn browse_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        self.browse_result_complete(query, downloaded_only, None, false).map(|result| result.contents)
    }
}

include_sql!("src/browse/sql/folders.sql");
include_sql!("src/browse/sql/subjects.sql");
include_sql!("src/browse/sql/home.sql");
include_sql!("src/browse/sql/authors.sql");
include_sql!("src/browse/sql/detail.sql");
include_sql!("src/browse/sql/counts.sql");
include_sql!("src/browse/sql/cards.sql");
include_sql!("src/browse/sql/books.sql");

impl crate::Database {
    /// Books with a live placement, as the carousels display them.
    pub fn book_count(&self) -> Result<usize, DatabaseError> {
        let mut count = 0_i64;
        self.connection.count_all_books(|row| {
            count = row.get(0)?;
            Ok(())
        })?;
        usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))
    }
}

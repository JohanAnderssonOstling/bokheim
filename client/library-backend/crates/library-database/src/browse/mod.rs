//! Library browse pages, hierarchy projections, home, and authors.

use crate::DatabaseError;
use crate::SqlParam;
use content_address::BookId;
mod author_query;
mod books;
mod cards;
mod catalog;
pub(crate) mod facets;
pub(crate) mod options;
mod ordering;
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
    #[serde(default)]
    pub page: Option<BrowsePageState>,
}

impl crate::Database {
    /// Complete projection and paged UI reads share the same selection semantics.
    pub fn browse(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, DatabaseError> {
        self.browse_result_complete(query, downloaded_only, true)
    }

    /// Loads only requested section panels and direct covers. Names and totals
    /// remain complete for layout, navigation, and restoring distant pages.
    pub fn browse_page(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, page: BrowsePageRequest) -> Result<LibraryBrowseData, DatabaseError> {
        self.browse_result_initial(query, downloaded_only, page)
    }

    pub fn browse_section_books(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool, offset: usize, limit: usize) -> Result<library_model::BrowseBookPage, DatabaseError> {
        self.browse_result_section_books(query, downloaded_only, offset, limit)
    }

    /// Complete direct identities and navigation for recursive operations.
    pub fn browse_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        self.browse_result_complete(query, downloaded_only, false).map(|result| result.contents)
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
        let count = self.connection.count_all_books(|row| row.get::<_, i64>(0))?;
        usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))
    }
}

// Complete browse requests execute inside one database owner.
database_methods! {
    fn browse(query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> LibraryBrowseData;
    arguments (library_model::LibraryBrowseQuery, bool) = (query.clone(), downloaded_only);
    execute database, arguments => database.browse(&arguments.0, arguments.1);

    fn browse_page(query: &library_model::LibraryBrowseQuery, downloaded_only: bool, page: BrowsePageRequest) -> LibraryBrowseData;
    arguments (library_model::LibraryBrowseQuery, bool, BrowsePageRequest) = (query.clone(), downloaded_only, page);
    execute database, arguments => database.browse_page(&arguments.0, arguments.1, arguments.2);

    fn browse_section_books(query: &library_model::LibraryBrowseQuery, downloaded_only: bool, offset: usize, limit: usize) -> library_model::BrowseBookPage;
    arguments (library_model::LibraryBrowseQuery, bool, usize, usize) = (query.clone(), downloaded_only, offset, limit);
    execute database, arguments => database.browse_section_books(&arguments.0, arguments.1, arguments.2, arguments.3);

    fn browse_contents(query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> library_model::BrowseContents;
    arguments (library_model::LibraryBrowseQuery, bool) = (query.clone(), downloaded_only);
    execute database, arguments => database.browse_contents(&arguments.0, arguments.1);

    fn book_count() -> usize;
    arguments () = ();
    execute database, arguments => database.book_count();
}

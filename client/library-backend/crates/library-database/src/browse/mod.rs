//! Library browse pages, hierarchy projections, home, and authors.

use crate::DatabaseError;
use crate::SqlParam;
use content_address::ContentHash;
mod all_books;
mod books;
mod cards;
mod catalog;
mod facets;
pub(crate) mod options;
pub mod rows;
mod snapshot;
use options::BrowseOptions;
use rows::{invalid_integer_column, invalid_text_column, parse_uuid_column};
pub(crate) mod authors;
pub(crate) mod folders;
pub(crate) mod home;
pub(crate) mod library;
pub(crate) mod subjects;
pub(crate) mod text;

use include_sqlite_sql::include_sql;

use library_model::BrowseRow;

/// Complete library-owned browse projection returned to any UI transport.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
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
}

impl crate::Database {
    /// Complete browse page, including facets and breadcrumbs, from one snapshot.
    pub fn browse(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, DatabaseError> {
        match query.location {
            library_model::BrowseLocation::Books => self.browse_all_books(query, downloaded_only),
            library_model::BrowseLocation::Folder(directory) => self.browse_folder(directory, query, downloaded_only),
            library_model::BrowseLocation::Subject(route) => self.browse_subject(subjects::Books::Library, route, query, downloaded_only),
            library_model::BrowseLocation::Catalog(route) => self.browse_subject(subjects::Books::Catalog, route, query, downloaded_only),
            library_model::BrowseLocation::Author(author) => self.browse_author(author, query, downloaded_only),
            library_model::BrowseLocation::Home(shelf) => self.browse_home(shelf, query, downloaded_only),
            library_model::BrowseLocation::Opds { .. } => Err(DatabaseError::message("an OPDS feed is browsed over the network, not in the library")),
        }
    }

    /// Ungrouped contents for recursive operations such as requesting downloads.
    pub fn browse_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        match query.location {
            library_model::BrowseLocation::Books => self.browse_all_books_contents(query, downloaded_only),
            library_model::BrowseLocation::Folder(directory) => self.browse_folder_contents(directory, query, downloaded_only),
            library_model::BrowseLocation::Subject(route) => self.browse_subject_contents(subjects::Books::Library, route, query, downloaded_only),
            library_model::BrowseLocation::Catalog(route) => self.browse_subject_contents(subjects::Books::Catalog, route, query, downloaded_only),
            library_model::BrowseLocation::Author(author) => self.browse_author_contents(author, query, downloaded_only),
            library_model::BrowseLocation::Home(shelf) => self.browse_home_contents(shelf, query, downloaded_only),
            library_model::BrowseLocation::Opds { .. } => Err(DatabaseError::message("an OPDS feed is browsed over the network, not in the library")),
        }
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

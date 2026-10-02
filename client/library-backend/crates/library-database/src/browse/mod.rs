//! Library browse pages, hierarchy projections, home, and authors.

use crate::DatabaseError;
mod books;
mod cards;
mod facets;
pub(crate) mod options;
pub mod rows;
mod snapshot;
use options::BrowseOptions;
use rows::{invalid_integer_column, invalid_text_column, parse_uuid_column};
pub(crate) mod folders;
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
    pub language_counts: Vec<library_model::LibraryLanguageCount>,
    pub format_counts: Vec<library_model::LibraryFormatCount>,
}

impl crate::Database {
    /// Complete browse page, including facets and breadcrumbs, from one snapshot.
    pub fn browse(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<LibraryBrowseData, DatabaseError> {
        match query.location {
            library_model::BrowseLocation::Folder(directory) => self.browse_folder(directory, query, downloaded_only),
            library_model::BrowseLocation::Subject(route) => self.browse_subject(route, query, downloaded_only),
        }
    }

    /// Ungrouped contents for recursive operations such as requesting downloads.
    pub fn browse_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        match query.location {
            library_model::BrowseLocation::Folder(directory) => self.browse_folder_contents(directory, query, downloaded_only),
            library_model::BrowseLocation::Subject(route) => self.browse_subject_contents(route, query, downloaded_only),
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

#[cfg(test)]
mod tests;

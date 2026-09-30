//! Library browse pages, hierarchy projections, home, and authors.

mod books;
mod cards;
mod facets;
pub(crate) mod options;
mod rows;
mod snapshot;
use options::{BrowseOptions, book_sort_query_value};
use rows::{invalid_integer_column, invalid_text_column, parse_uuid_column, read_book_card_row};
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

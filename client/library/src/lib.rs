//! Client library features and their persistence, shared by native and browser runtimes.
// `include_sql!` generates query methods with one argument per SQL parameter.
#![allow(clippy::too_many_arguments)]
#[macro_use]
mod query_methods;
use library_model::{AuthorSummary, BookCardRow, SubjectPath};
use sync_common::DirId;

#[cfg(test)]
mod contributor_encoding_tests;
pub mod descriptions;
pub mod file_work;
mod metadata_batch;

/// Compatibility re-export: filename utilities live in `library-replica`.
pub use library_replica::filenames;
mod local_paths;
pub use local_paths::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};

#[derive(Clone, serde::Serialize)]
pub struct LibraryAuthorsView {
    pub authors: Vec<AuthorSummary>,
    pub books: Option<Vec<BookCardRow>>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryHierarchyEntry<Id> {
    pub id: Id,
    pub name: String,
    pub book_count: usize,
    pub downloaded_book_count: usize,
}

#[derive(Clone, serde::Serialize)]
pub struct LibraryHierarchyView<Id> {
    pub children: Vec<LibraryHierarchyEntry<Id>>,
    pub books: Vec<BookCardRow>,
}

pub type LibraryFolderEntry = LibraryHierarchyEntry<DirId>;
pub type LibrarySubjectEntry = LibraryHierarchyEntry<SubjectPath>;
pub type LibraryFolderView = LibraryHierarchyView<DirId>;
pub type LibrarySubjectView = LibraryHierarchyView<SubjectPath>;

pub const LIBRARY_SCHEMA_VERSION: i64 = 57;

pub mod external_metadata;
pub mod import_inspection;
pub mod marks;
pub mod remote_reducer;
pub mod runtime_contracts;
pub mod subject_pipeline;
pub mod thumbnail_jobs;
pub mod toc;

/// Read models used by library browsing pages.
pub mod browse;


/// Local and remote asset state and durable download intent.
pub mod assets;

/// Durable reader progress for EPUB, PDF, and audiobook formats.
pub mod reading;

pub mod scanner;
/// Synchronization outbox, scan generations, tombstones, and replica state.
pub mod sync_state;
pub mod sync_status;

/// Typed book-card read models.
pub mod books;

/// Metadata registers, relational projections, and author identities.
pub mod authors;
pub mod metadata;
pub mod pdf_metadata;
pub mod prepared_enrichment;

/// Directory and physical book placement persistence.
pub mod placements;

pub mod audible_jobs;
/// Metadata derived specifically from audiobook files.
pub mod audiobooks;

#[cfg(test)]
mod tests;

pub mod schema;
mod storage_values;

pub mod operations;

pub mod upload_pipeline;

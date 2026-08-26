//! API and shared resources scoped to one concrete library.

mod covers;
pub(crate) mod database;
mod enrichment;
mod handle;
mod management;
mod model;
mod open;
mod operations;
#[cfg(feature = "pdf")]
pub(crate) mod pdf_annotations;

use library_sqlite::filenames;

#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use covers::{generate_thumbnail_for_sync, thumbnail_needs_repair};
pub use enrichment::{MissingSubjectsBook, SubjectEnrichmentOutcome, SubjectEnrichmentStatus};
pub use handle::{LibraryAuthorsView, LibraryBackend, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use model::generate_content_hash_blake3;
pub use model::ResolvedBook;

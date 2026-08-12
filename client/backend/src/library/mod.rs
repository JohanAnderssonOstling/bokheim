//! API and shared resources scoped to one concrete library.

mod covers;
pub(crate) mod database;
mod enrichment;
pub(crate) mod filenames;
mod handle;
pub mod links;
mod management;
pub mod marks;
mod model;
mod open;
mod operations;

#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use covers::generate_thumbnail_for_sync;
pub use enrichment::{MissingSubjectsBook, SubjectEnrichmentOutcome, SubjectEnrichmentStatus};
pub use handle::{
    LibraryAuthorsView, LibraryBackend, LibraryFolderDestination, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibraryHomeView, LibrarySubjectEntry, LibrarySubjectView, LibraryTrashView, SubjectPath,
    TrashedBook, TrashedFolder,
};
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use model::generate_content_hash_blake3;
pub use model::{AuthorSort, AuthorSummary, BookCardRow, BookImportDestination, Dir, LibrarySearchResult, ReaderFormat, ResolvedBook};

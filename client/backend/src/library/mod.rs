//! API and shared resources scoped to one concrete library.

pub mod browse_rows;
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

pub use app_contract::library::{audiobook_toc_position, audiobook_toc_target, AudiobookChapter, BookDetail, BookDetailAuthor, BookTocEntry, LibraryFileTypeFilter};
pub use browse_rows::{BrowseContents, BrowseRow};
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use covers::{generate_thumbnail_for_sync, thumbnail_needs_repair};
pub use enrichment::{EditionIdentityReview, EditionIdentityReviewCandidate, EditionIdentityReviewStatus, MissingSubjectsBook, SubjectEnrichmentOutcome, SubjectEnrichmentStatus};
pub use handle::{
    LibraryAuthorsView, LibraryBackend, LibraryFolderDestination, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibraryHomeView, LibrarySubjectEntry, LibrarySubjectView, LibraryTrashView, SubjectPath,
    TrashedBook, TrashedFolder,
};
pub use library_model::ROOT_SUBJECT_PATH;
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use model::generate_content_hash_blake3;
pub use model::{AuthorSort, AuthorSummary, BookCardRow, BookImportDestination, Dir, LibrarySearchResult, ReaderFormat, ResolvedBook};

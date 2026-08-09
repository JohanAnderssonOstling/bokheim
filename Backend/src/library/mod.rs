//! API and shared resources scoped to one concrete library.

mod covers;
pub(crate) mod database;
pub(crate) mod filenames;
mod handle;
pub mod links;
pub mod marks;
mod model;
mod open;
mod operations;

#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use covers::generate_thumbnail_for_sync;
pub use handle::{LibraryAuthorsView, LibraryBackend, LibraryFolderEntry, LibraryFolderView, LibraryHomeView};
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub(crate) use model::generate_content_hash_blake3;
pub use model::{AuthorFolderBookSummaries, AuthorFolderBooks, AuthorSort, AuthorSummary, Book, BookCardRow, BookImportDestination, BookSummary, Dir, DirContent, DirSummaryContent, ReaderFormat, ResolvedBook};

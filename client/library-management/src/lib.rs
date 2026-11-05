//! Profile-wide account, registration, catalog, and storage coordination.
//! Concrete library sessions and their workers belong to `library-backend`.
use library_backend::{executor, library, LibraryHandle};
use library_backend::{BackendError, BookId, LibraryClient, LibraryId, LibrarySyncState, LibrarySyncStatus};

pub mod account;
mod catalog;
pub mod management;
pub use catalog::start as start_catalog;

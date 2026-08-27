//! API and shared resources scoped to one concrete library.

pub(crate) mod database;
mod download;
mod enrichment;
mod handle;
mod io;
mod management;
mod model;
#[cfg(not(target_arch = "wasm32"))]
mod native_import;
mod open;
mod operations;

use library_sqlite::filenames;

pub use download::{BookDownloadStatus, DownloadProgress, DownloadProgressError, DownloadState, FolderDownloadStatus};
pub use handle::{LibraryAuthorsView, LibraryBackend, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};
pub use io::{BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReaderFuture};
pub use model::ResolvedBook;

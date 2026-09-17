//! API and shared resources scoped to one concrete library.

pub mod discovery;
pub(crate) use library_runtime::events;

mod annotations;
mod browse;
pub mod commands;
mod directory;
mod handle;
mod io;
mod jobs;
mod management;
mod reading;
mod resources;
pub use book_access::remote_file::{AudioStreamControl, RemoteFile};
mod audible_enrichment;
#[cfg(target_arch = "wasm32")]
#[path = "../browser_audiobook.rs"]
pub mod browser_audiobook;
#[cfg(target_arch = "wasm32")]
#[path = "../browser_book.rs"]
pub mod browser_book;
#[cfg(not(target_arch = "wasm32"))]
mod directory_import;
mod enrichment;
mod operations;
mod preparation;
mod book;
pub(crate) mod remote_access;
mod transfer_status;

use library_replica::filenames;

#[cfg(target_arch = "wasm32")]
pub use browser_audiobook::{BrowserAudiobook, PlaybackLease};
pub use library_database::LibraryBrowseData;
pub use client_library::runtime_contracts::{DirectPlayback, PlaybackLocation, PlaybackMetadata, PlaybackSource};
pub use client_library::sync_status::{LibrarySyncState, LibrarySyncStatus};
pub use commands::LibraryCommand;
pub use handle::{LibraryAuthorsView, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};
pub use io::PreparedDirectoryImport;
pub use io::{BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, ImportSource};
pub use library_model::{BookDownloadStatus, DownloadProgress, DownloadProgressError, DownloadState};
pub use book::{AudiobookBook, DocumentBook, ResolvedBook};
#[cfg(not(target_arch = "wasm32"))]

pub use directory::LibraryDirectory;
pub use handle::{LibraryHandle, LibrarySession};
pub(crate) use library_runtime::session::LibraryActivity;
pub use resources::LibraryRuntimeSpec;


#[cfg(not(target_arch = "wasm32"))]
mod import_native;
#[cfg(target_arch = "wasm32")]
mod import_web;

//! API and shared resources scoped to one concrete library.

pub(crate) mod discovery;
pub(crate) use library_runtime::events;

mod annotations;
mod browse;
pub(crate) mod commands;
mod directory;
mod handle;
mod io;
mod jobs;
mod management;
mod reading;
mod resources;
pub(crate) use publication_access::remote_file;
pub use remote_file::RemoteFile;
mod audible_enrichment;
#[cfg(target_arch = "wasm32")]
#[path = "../browser_audiobook.rs"]
pub(crate) mod browser_audiobook;
#[cfg(target_arch = "wasm32")]
#[path = "../browser_publication.rs"]
pub(crate) mod browser_publication;
#[cfg(not(target_arch = "wasm32"))]
mod directory_import;
mod enrichment;
mod operations;
mod preparation;
mod publication;
pub(crate) mod remote_access;
mod transfer_status;

use client_library::filenames;

#[cfg(target_arch = "wasm32")]
pub use browser_audiobook::{BrowserAudiobook, PlaybackLease};
pub use client_library::browse::LibraryBrowseData;
pub use client_library::runtime_contracts::{DirectPlayback, PlaybackLocation, PlaybackMetadata, PlaybackSource};
pub use client_library::sync_status::{LibrarySyncState, LibrarySyncStatus};
pub use commands::LibraryCommand;
pub use handle::{LibraryAuthorsView, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};
pub(crate) use io::PreparedDirectoryImport;
pub use io::{BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, ImportSource};
pub use library_model::{BookDownloadStatus, DownloadProgress, DownloadProgressError, DownloadState};
pub use publication::{AudiobookBook, DocumentBook, ResolvedBook};
#[cfg(not(target_arch = "wasm32"))]
pub use remote_file::AudioStreamControl;

pub(crate) use directory::LibraryDirectory;
pub(crate) use handle::{LibraryHandle, LibrarySession};
pub(crate) use library_runtime::session::LibraryActivity;
pub(crate) use resources::LibraryRuntimeSpec;

pub(crate) use publication_access::pdf_bundle;

#[cfg(not(target_arch = "wasm32"))]
mod import_native;
#[cfg(target_arch = "wasm32")]
mod import_web;

//! API and shared resources scoped to one concrete library.

pub mod discovery;
pub(crate) use library_runtime::events;

mod browse;
#[cfg(not(target_arch = "wasm32"))]
mod browser;
pub use browse::LibraryBookStorage;
pub mod commands;
mod directory;
mod notifications;
mod open;
pub(crate) mod session;
pub mod subscriptions;
#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
mod watcher;
mod workers;
pub use open::LibraryOpenConfig;
pub mod handle;
mod io;
pub(crate) mod jobs;
mod pending;
mod reading;
mod resources;
pub(crate) mod transfer;
#[cfg(not(target_arch = "wasm32"))]
pub use book_access::remote_file::AudioStreamControl;
pub use book_access::remote_file::RemoteFile;
pub use transfer::StorageCopyProgress;
pub(crate) mod book;
#[cfg(target_arch = "wasm32")]
#[path = "../browser_audiobook.rs"]
pub mod browser_audiobook;
mod directory_import;
pub(crate) mod operations;
mod playback;
pub(crate) mod preparation;
pub(crate) mod remote_access;

pub use book::{AudiobookBook, DocumentBook, DocumentFormat, DocumentSource, PdfBook, ResolvedBook};
#[cfg(target_arch = "wasm32")]
pub use browser_audiobook::{BrowserAudiobook, PlaybackLease};
pub use commands::{LibraryCommand, PendingCommand, ReadyCommand};
pub use directory::LibraryDirectory;
pub use handle::{LibraryHandle, LibrarySession};
pub use io::{BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, ImportSource};
pub use library_database::sync_status::{LibrarySyncState, LibrarySyncStatus};
pub use library_database::LibraryBrowseData;
pub use library_model::{BookDownloadStatus, DownloadProgress, DownloadProgressError, DownloadState};
pub use notifications::LibraryNotifications;
pub use playback::{DirectPlayback, PlaybackLocation, PlaybackSource};
pub use resources::LibraryRuntimeSpec;

#[cfg(target_arch = "wasm32")]
pub use book::web_reader;

#[cfg(not(target_arch = "wasm32"))]
mod annotation_export;
#[cfg(not(target_arch = "wasm32"))]
mod automatic_annotation_export;
#[cfg(not(target_arch = "wasm32"))]
mod reader_import;

#[cfg(not(target_arch = "wasm32"))]
mod job_registry;

mod text_search;

#[cfg(not(target_arch = "wasm32"))]
mod opds;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use opds::OpdsLibrary;

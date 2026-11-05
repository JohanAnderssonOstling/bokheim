//! API and shared resources scoped to one concrete library.
//!
//! Ownership, services, and worker coordination live here. Concrete operations
//! live in the book, browse, downloads, import, storage, and sync modules.

pub(crate) use library_services::events;

// Shared infrastructure for every library operation.
mod activity;
mod event_processing;
pub(crate) mod jobs;
pub mod library_handle;
mod library_resources;
mod lifecycle;
mod notifications;
pub mod subscriptions;
mod workers;

// Concrete library features.
pub(crate) mod book;
mod browse;
mod downloads;
mod import;
mod storage;
mod sync;

#[cfg(not(target_arch = "wasm32"))]
pub use book_access::remote_file::AudioStreamControl;
pub use book_access::remote_file::RemoteFile;
pub use downloads::LibraryBookStorage;
pub use storage::StorageCopyProgress;
#[cfg(target_arch = "wasm32")]
#[path = "../browser_audiobook.rs"]
pub mod browser_audiobook;

pub use crate::book::{PlaybackLocation, PlaybackSource};
pub use book::{AudiobookBook, DocumentBook, DocumentFormat, DocumentSource, PdfBook, ResolvedBook};
pub use book_access::{BookReader, BoxedBookReader};
#[cfg(target_arch = "wasm32")]
pub use browser_audiobook::{BrowserAudiobook, PlaybackLease};
pub use library_database::sync_status::{LibrarySyncState, LibrarySyncStatus};
pub use library_database::LibraryBrowseData;
pub use library_handle::LibraryHandle;
pub use library_import::{DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, ImportSource};
pub use library_model::{BookDownloadStatus, DownloadProgress, DownloadProgressError, DownloadState};
pub use library_resources::LibraryOpenOptions;
pub(crate) use library_resources::LibraryResources;
#[cfg(not(target_arch = "wasm32"))]
pub use lifecycle::RootMoveOperation;
pub use notifications::LibraryNotifications;
pub use remote_book_access::DirectPlayback;
#[cfg(target_arch = "wasm32")]
pub use subscriptions::BookSubscription;

#[cfg(target_arch = "wasm32")]
pub use book::web_reader;

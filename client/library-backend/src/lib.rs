//! Resource-bearing backend for one concrete library.
//!
//! The application crate owns device registry, account policy, and startup.
//! This crate owns database-backed library sessions, synchronization, imports,
//! book access, and their workers.

extern crate core;

pub use client_platform_runtime::executor;

pub mod asset_store;
mod asset_workflow;
pub mod import_journal;
pub mod import_status;
pub mod import_workflow;
pub mod library;
pub mod book;
pub mod staged_import;
pub mod sync;
pub mod worker_cpu;
pub(crate) mod thumbnail_jobs;
#[cfg(test)]
mod test_database;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
pub(crate) mod scanner;

pub use import_status::{ImportFailure, ImportFailureKind, ImportProgress, ImportState};
pub use library::{BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData, LibrarySyncState, LibrarySyncStatus};
pub use library_replica::{BlobKey, BlobKind, ScanProgress, TransferJobKind};
pub use sync::transfer::{LibraryTransfers, TransferOrigin, TransferState, TransferStatus};
pub use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};
pub use library_runtime::BackendError;
pub use account_client::ServerUrl;
pub use library_database::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
/// The database belongs to a concrete library session, not the filesystem adapter.
pub(crate) const APP_HIDDEN_DIR: &str = ".bokheim";
pub(crate) const APP_HIDDEN_LIBRARY_DB_PATH: &str = ".bokheim/library.db";
pub use library_runtime::wire::ThumbnailResolution;

pub(crate) use library_runtime::interest as notification_interest;

/// Book-level enrichment progress. Categories are mutually exclusive.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnrichmentStatus {
    pub running: bool,
    pub total: u64,
    pub enriched: u64,
    pub pending: u64,
    pub no_match: u64,
    pub failed: u64,
}

pub use library::{LibraryCommand, LibraryHandle, LibrarySession};
pub use book::{BookSource, ResolvedBookData};

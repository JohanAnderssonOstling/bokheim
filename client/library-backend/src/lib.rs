//! Resource-bearing backend for one concrete library.
//!
//! The application crate owns device registry, account policy, and startup.
//! This crate owns database-backed library sessions, synchronization, imports,
//! book access, and their workers.

extern crate core;

pub use client_platform_runtime::executor;

pub mod asset_store;
mod asset_workflow;
pub mod book;
pub mod client;
pub use client::LibraryClient;
pub use book_model::{AgentId, AnnotationAnchor, AnnotationStyle, AuthorId, BookFormat, EpubCfi, ReadProgress, ReaderAnnotation};
pub use library::{AudiobookBook, DocumentBook, ResolvedBook};
#[cfg(target_arch = "wasm32")]
pub mod browser_import;
pub mod library;
pub mod integration;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
pub(crate) mod scanner;
pub mod sync;
#[cfg(test)]
mod test_database;
pub mod worker_cpu;

pub use account_client::ServerUrl;
pub use import_pipeline::{ImportFailure, ImportFailureKind, ImportProgress, ImportState};
pub use library::{
    BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData,
    LibrarySyncState, LibrarySyncStatus,
};
pub use library_database::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
pub use library_model::{LibraryBrowseQuery, LibraryHomeView, LibraryUpdate};
pub use library_replica::{BlobKey, BlobKind, FolderDestinationPolicy, LibraryOperation, LibraryOperationKind, LibraryOperationState, ScanProgress, TransferJobKind};
pub use client_runtime::BackendError;
pub use sync::transfer::{LibraryTransfers, TransferOrigin, TransferState, TransferStatus};
pub use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};
/// The database belongs to a concrete library session, not the filesystem adapter.
pub(crate) const APP_HIDDEN_DIR: &str = ".bokheim";
pub(crate) const APP_HIDDEN_LIBRARY_DB_PATH: &str = ".bokheim/library.db";
mod thumbnail_resolution;
pub use thumbnail_resolution::ThumbnailResolution;

pub(crate) use library_runtime::interest as notification_interest;

pub use book::{BookSource, ResolvedBookData};
pub use library::{LibraryCommand, LibraryHandle, LibrarySession};

/// Inspection cache version for a format. Matches the extractor contract:
/// EPUB and PDF evidence versions with the shared extractor, others are 1.
/// Shared by explicit import and the filesystem scanner, which both cache
/// results keyed on this value.
pub(crate) fn inspection_version(format: book_model::BookFormat) -> i64 {
    match format {
        book_model::BookFormat::Epub | book_model::BookFormat::Pdf => i64::from(book_inspection::evidence::INSPECTION_VERSION),
        _ => 1,
    }
}

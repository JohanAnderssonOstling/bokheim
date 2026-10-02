//! Resource-bearing backend for one concrete library.
//!
//! The application crate owns device registry, account policy, and startup.
//! This crate owns database-backed library sessions, synchronization, imports,
//! book access, and their workers.

extern crate core;

pub use client_platform_runtime::executor;

pub mod asset_store;
pub mod asset_workflow;
pub mod book;
pub mod client;
pub use book_model::{AgentId, AnnotationAnchor, AnnotationStyle, AuthorId, BookFormat, EpubCfi, ReadProgress, ReaderAnnotation};
pub use client::LibraryClient;
pub use library::{AudiobookBook, DocumentBook, ResolvedBook};
pub mod integration;
pub mod library;
pub mod sync;
#[cfg(test)]
mod test_database;

pub use account_client::ServerUrl;
pub use client_runtime::BackendError;
pub use import_pipeline::{ImportFailure, ImportFailureKind, ImportProgress, ImportState};
pub use library::{
    BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData,
    LibrarySyncState, LibrarySyncStatus,
};

pub use library_model::{LibraryBrowseQuery, LibraryHomeView, LibraryUpdate};
pub use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};
/// The database belongs to a concrete library session, not the filesystem adapter.
pub(crate) const APP_HIDDEN_DIR: &str = ".bokheim";
pub(crate) const APP_HIDDEN_LIBRARY_DB_PATH: &str = ".bokheim/library.db";
mod thumbnail_resolution;
pub use thumbnail_resolution::ThumbnailResolution;

pub(crate) use library_runtime::interest as notification_interest;

pub use book::{BookSource, ResolvedBookData};
pub use library::{ImportBookSource, LibraryCommand, LibraryHandle, LibrarySession, PendingCommand, ReadyCommand, RuntimeCommand, SessionCommand, StagedImportMetadata};

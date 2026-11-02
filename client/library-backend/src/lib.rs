//! Resource-bearing backend for one concrete library.
//!
//! The application crate owns device registry, account policy, and startup.
//! This crate owns database-backed library sessions, synchronization, imports,
//! book access, and their workers.

extern crate core;

pub use client_platform_runtime::executor;

pub mod asset_store;
pub mod book;
pub mod client;
pub use book_model::{AgentId, Annotation, AnnotationAnchor, AnnotationStyle, AuthorId, BookFormat, EpubCfi, ReadProgress};
pub use client::LibraryClient;
pub use library::{AudiobookBook, DocumentBook, DocumentSource, ResolvedBook};
pub mod integration;
pub mod library;

pub use account_client::Url;
pub use client_runtime::BackendError;
pub use library::{
    BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData,
    LibrarySyncState, LibrarySyncStatus,
};
pub use library_import::{ImportFailure, ImportFailureKind};
#[cfg(not(target_arch = "wasm32"))]
pub use library_import::readest::{ImportMode as ReadestImportMode, ReadestBookCounts, ReadestGroup, ReadestImportOptions, ReadestImportProgress, ReadestImportReport, ReadestLibrarySummary};
#[cfg(not(target_arch = "wasm32"))]
pub use library_import::reader_import::{ImportMode as ReaderImportMode, ReaderBookCounts, ReaderGroup, ReaderImportOptions, ReaderImportProgress, ReaderImportReport, ReaderImportSource, ReaderLibrarySummary};

pub use library::subscriptions::LibraryUpdates;
pub use library_model::{LibraryBrowseQuery, LibraryUpdate};
pub use sync_common::{BookId, DirId, LibraryId, ROOT_DIR_ID};
mod thumbnail_resolution;
pub use library_catalog::evict_cover_caches as evict_catalog_cover_caches;
pub use library_catalog::{COVER_CACHE_MAX_BYTES as CATALOG_COVER_CACHE_MAX_BYTES, configure_cover_cache as configure_catalog_cover_cache};
pub use thumbnail_resolution::ThumbnailResolution;

pub(crate) use library_runtime::interest as notification_interest;

pub use book::{BookSource, ResolvedBookData};
pub use library::{LibraryCommand, LibraryHandle, LibrarySession, PendingCommand, ReadyCommand};

pub use library_annotation_export::library_destination as annotation_export_destination;

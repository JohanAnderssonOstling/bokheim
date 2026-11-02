//! Resource-bearing backend for one concrete library.
//!
//! Hosts construct shared account and library services.
//! This crate owns account lifecycle, library registration, opening, database-backed sessions,
//! synchronization, imports, book access, and their workers.

extern crate core;

pub use client_platform_runtime::executor;

pub mod account;
pub mod asset_store;
pub mod book;
mod caches;
pub use caches::evict_cover_caches;
mod catalog;
pub mod catalog_sources;
pub use catalog::start as start_catalog;
pub mod client;
pub use book_model::{AgentId, Annotation, AnnotationAnchor, AnnotationStyle, AuthorId, BookFormat, EpubCfi, ReadProgress};
pub use client::LibraryClient;
pub use library::LibraryBookStorage;
pub use library::{AudiobookBook, DocumentBook, DocumentFormat, DocumentSource, PdfBook, ResolvedBook};
pub mod integration;
pub mod library;
pub mod management;

pub use account_client::Url;
pub use client_runtime::BackendError;
pub use library::{
    BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData,
    LibrarySyncState, LibrarySyncStatus,
};
#[cfg(not(target_arch = "wasm32"))]
pub use library_import::reader_import::{ImportMode as ReaderImportMode, ReaderBookCounts, ReaderGroup, ReaderImportOptions, ReaderImportProgress, ReaderImportReport, ReaderImportSource, ReaderLibrarySummary};
#[cfg(not(target_arch = "wasm32"))]
pub use library_import::readest::{ImportMode as ReadestImportMode, ReadestBookCounts, ReadestGroup, ReadestImportOptions, ReadestImportProgress, ReadestImportReport, ReadestLibrarySummary};
pub use library_import::{ImportFailure, ImportFailureKind};

pub use library::subscriptions::LibraryUpdates;
pub use library_model::{LibraryBrowseQuery, LibraryUpdate};
pub use sync_common::{BookId, DirId, LibraryId, ROOT_DIR_ID};
mod thumbnail_resolution;
pub use library_catalog::evict_cover_caches as evict_catalog_cover_caches;
pub use thumbnail_resolution::ThumbnailResolution;

pub(crate) use library_runtime::interest as notification_interest;

pub use book::{BookSource, ResolvedBookData};
pub use library::{LibraryCommand, LibraryHandle, LibrarySession, PendingCommand, ReadyCommand};

pub use library_annotation_export::library_destination as annotation_export_destination;

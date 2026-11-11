//! Resource-bearing backend for one concrete library.
//!
//! Hosts supply account capabilities and opening policy. This crate owns
//! library resources, synchronization, imports, book access, and workers.

pub use client_platform_runtime::executor;

pub mod asset_store;
pub mod book;
pub mod client;
pub use book_model::{AgentId, Annotation, AnnotationAnchor, AnnotationStyle, AuthorId, BookFormat, EpubCfi, ReadProgress};
pub use client::LibraryClient;
pub use library::LibraryBookStorage;
pub use library::{AudiobookBook, DocumentBook, DocumentFormat, DocumentSource, PdfBook, ResolvedBook, TextCache};
pub mod library;

pub use account_client::Url;
pub use client_protocol::BackendError;
pub use library::{
    BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData,
    LibrarySyncState, LibrarySyncStatus,
};
#[cfg(not(target_arch = "wasm32"))]
pub use library_import::reader_import::{ImportMode as ReaderImportMode, ReaderBookCounts, ReaderGroup, ReaderImportOptions, ReaderImportReport, ReaderImportSource, ReaderLibrarySummary};
#[cfg(not(target_arch = "wasm32"))]
pub use library_import::readest::{ImportMode as ReadestImportMode, ReadestBookCounts, ReadestGroup, ReadestImportOptions, ReadestImportReport, ReadestLibrarySummary};
pub use library_import::{ImportFailure, ImportFailureKind};

pub use library::subscriptions::LibraryUpdates;
pub use library_model::{LibraryBrowseQuery, LibraryUpdate};
pub use sync_common::{BookId, DirId, LibraryId, ROOT_DIR_ID};
mod thumbnail_resolution;
pub use thumbnail_resolution::ThumbnailResolution;

pub use book::{BookSource, ResolvedBookData};
pub use library::LibraryHandle;

pub use library_annotation_export::library_destination as annotation_export_destination;

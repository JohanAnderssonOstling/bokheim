//! Portable library values and validation rules.
//!
//! This crate deliberately has no filesystem, synchronization, networking, UI,
//! or runtime dependencies.
//!
//! Its only re-export from another crate is `BookTocEntry`, defined once in
//! `book-model` and passed through here so existing paths keep working.
//! A crate that needs any other part of a book's vocabulary depends
//! on `book-model`, a taxonomy's on `subject-projection`: passing those
//! through here made every dependant look like it needed a library when
//! it needed a book, and hid which layer a type actually belongs to.

mod placement;
pub use placement::{BookPlacement, BookUploadIntent, CloudQuotaBook, PublishedStagedBook, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};

mod transfer;
pub use transfer::{BlobKey, BlobKind, TransferJobKind, TransferOrigin};

mod download;
mod playback;
mod view;

pub use download::{BookDownloadStatus, BrowserDownloadPhase, BrowserDownloadUpdate, CatalogImportProgress, DownloadProgress, DownloadProgressError, DownloadState};
pub use playback::PlaybackMetadata;
pub use view::*;

pub use book_model::BookTocEntry;

mod annotation_export;
pub use annotation_export::*;
mod recurring_annotation;
pub use recurring_annotation::RecurringAnnotation;
mod reader_link;
pub use reader_link::ReaderLink;
mod notebook;
mod text_search;
pub use text_search::{BookTextMatch, BookTextMatchPage, BookTextSearchBook, BookTextSearchPage, TextSearchRemoval};
pub use notebook::{NotebookAnnotationPage, NotebookBook, NotebookPage, NotebookQuery, NotebookRequest, NotebookSort, NOTEBOOK_ANNOTATION_PAGE_SIZE};

mod pdf_annotation;
pub use pdf_annotation::stored_pdf_annotation;

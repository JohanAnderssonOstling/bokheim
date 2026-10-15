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
pub use placement::{BookPlacement, BookUploadIntent, PublishedStagedBook, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};

mod transfer;
pub use transfer::{BlobKey, BlobKind, TransferJobKind, TransferOrigin};

mod download;
mod playback;
mod torrent;
pub use torrent::{TorrentInput, TorrentStatus};
mod view;

pub use download::{BookDownloadStatus, BrowserDownloadPhase, BrowserDownloadUpdate, CatalogImportProgress, DownloadProgress, DownloadProgressError, DownloadState};
pub use playback::PlaybackMetadata;
pub use view::*;

pub use book_model::BookTocEntry;

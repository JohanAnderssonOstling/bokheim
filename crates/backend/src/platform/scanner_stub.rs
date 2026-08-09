use crate::app::BackendContext;
use crate::ContentHash;
use crate::LibraryRoot;
use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::Serialize;

pub use crate::DownloadState;

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded { content_hash: ContentHash, path: String },
    BookRemoved { content_hash: ContentHash, path: String },
    DirectoryAdded { path: String },
    DirectoryRemoved { path: String },
    ThumbnailGenerated { content_hash: ContentHash },
    DownloadStatusChanged { content_hash: ContentHash, state: DownloadState },
    FolderDownloadStatusChanged { dir_id: crate::DirId, downloaded_books: usize, total_books: usize },
    TransferStatusChanged,
    ScanComplete,
}

pub struct LibraryScanner;

impl LibraryScanner {
    pub fn new(_context: BackendContext, _library_path: String, _db: Pool<SqliteConnectionManager>, _event_tx: Sender<LibraryEvent>) -> std::io::Result<Self> {
        Ok(Self)
    }

    pub(crate) fn from_root(_context: BackendContext, _library_root: LibraryRoot, _db: Pool<SqliteConnectionManager>, _event_tx: Sender<LibraryEvent>) -> std::io::Result<Self> {
        Ok(Self)
    }
}

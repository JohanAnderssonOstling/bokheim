use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum DownloadState {
    NotDownloaded,
    Downloading(f32),
    Downloaded,
}

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded { book_id: String, path: String },
    BookRemoved { book_id: String, path: String },
    DirectoryAdded { path: String },
    DirectoryRemoved { path: String },
    ThumbnailGenerated { book_id: String },
    DownloadStatusChanged { book_id: String, state: DownloadState },
    BookOpened { book_id: String },
    ScanComplete,
}

pub struct LibraryScanner;

impl LibraryScanner {
    pub fn new(_library_path: String, _db: Pool<SqliteConnectionManager>, _event_tx: Sender<LibraryEvent>) -> Self {
        Self
    }
}

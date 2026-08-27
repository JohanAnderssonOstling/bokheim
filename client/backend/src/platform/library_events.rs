//! Internal library event channel shared by native scanning, browser imports,
//! synchronization, and the application backend.
//!
//! Native filesystem discovery and explicit browser imports produce the same
//! events. Delivery is asynchronous on every target so application logic does
//! not need threads on native or polling loops in the browser.

use crate::ContentHash;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded { content_hash: ContentHash, path: String },
    BookRemoved { content_hash: ContentHash, path: String },
    DirectoryAdded { path: String },
    DirectoryRemoved { path: String },
    ThumbnailGenerated { content_hash: ContentHash },
    ThumbnailBatchProcessed { content_hashes: Vec<ContentHash>, finished: bool },
    SubjectEnrichmentBatchProcessed { processed: usize, updated: usize, finished: bool },
    DownloadStatusChanged { content_hash: ContentHash, state: crate::DownloadState },
    FolderDownloadStatusChanged { dir_id: crate::DirId, downloaded_books: usize, total_books: usize },
    TransferStatusChanged,
    LibraryListChanged,
    LibraryChanged,
    ScanStarted,
    ScanComplete,
}

pub(crate) type LibraryEventSender = async_channel::Sender<LibraryEvent>;

#[derive(Clone, Debug)]
pub(crate) struct LibraryEventReceiver(async_channel::Receiver<LibraryEvent>);

pub(crate) fn library_event_channel() -> (LibraryEventSender, LibraryEventReceiver) {
    let (sender, receiver) = async_channel::unbounded();
    (sender, LibraryEventReceiver(receiver))
}

impl LibraryEventReceiver {
    pub(crate) async fn recv(&self) -> Result<LibraryEvent, async_channel::RecvError> {
        self.0.recv().await
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(&self, timeout: std::time::Duration) -> Result<LibraryEvent, async_channel::TryRecvError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.0.try_recv() {
                Ok(event) => return Ok(event),
                Err(async_channel::TryRecvError::Closed) => return Err(async_channel::TryRecvError::Closed),
                Err(async_channel::TryRecvError::Empty) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(1)),
                Err(error) => return Err(error),
            }
        }
    }
}

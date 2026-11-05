//! Library presentation updates, independent of storage access and worker scheduling.

use async_channel::{Receiver, Sender};
use library_model::{BookDownloadStatus, BookFileActivity, DownloadState, LibraryUpdate};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sync_common::BookId;

#[derive(Clone)]
pub struct LibraryUpdatePublisher(Arc<Mutex<UpdateState>>);

struct UpdateState {
    closed: bool,
    subscribers: Vec<Sender<LibraryUpdate>>,
    downloads: HashMap<BookId, DownloadState>,
    book_file_activity: BookFileActivity,
}

impl Default for LibraryUpdatePublisher {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(UpdateState { closed: false, subscribers: Vec::new(), downloads: HashMap::new(), book_file_activity: BookFileActivity::Idle })))
    }
}

impl LibraryUpdatePublisher {
    /// Snapshot replay and registration share the publication lock, so a new
    /// subscriber cannot miss a progress update between those two steps.
    pub fn subscribe(&self) -> Receiver<LibraryUpdate> {
        let (sender, receiver) = async_channel::unbounded();
        let mut state = self.0.lock().expect("library update state");
        if !state.closed {
            if state.book_file_activity != BookFileActivity::Idle {
                let _ = sender.try_send(LibraryUpdate::BookFileActivity(state.book_file_activity));
            }
            for (&book_id, download) in &state.downloads {
                if matches!(download, DownloadState::Queued | DownloadState::Downloading(_)) {
                    let _ = sender.try_send(LibraryUpdate::Download(BookDownloadStatus::new(book_id, download.clone())));
                }
            }
            state.subscribers.push(sender);
        }
        receiver
    }

    pub fn publish(&self, update: LibraryUpdate) {
        let mut state = self.0.lock().expect("library update state");
        if state.closed {
            return;
        }
        match &update {
            LibraryUpdate::Download(download) => {
                let previous = state.downloads.get(&download.book_id);
                if previous == Some(&download.state) || previous.is_some_and(|previous| matches!((previous, &download.state), (DownloadState::Downloaded, DownloadState::Queued | DownloadState::Downloading(_)))) {
                    return;
                }
                state.downloads.insert(download.book_id, download.state.clone());
            }
            LibraryUpdate::BookFileActivity(activity) => {
                if state.book_file_activity == *activity {
                    return;
                }
                state.book_file_activity = *activity;
            }
            _ => {}
        }
        state.send(update);
    }

    pub fn download_state(&self, book_id: &BookId) -> Option<DownloadState> {
        self.0.lock().expect("library update state").downloads.get(book_id).cloned()
    }

    pub fn reset_download(&self, book_id: &BookId) {
        let mut state = self.0.lock().expect("library update state");
        if matches!(state.downloads.get(book_id), Some(DownloadState::Downloaded)) {
            state.downloads.remove(book_id);
        }
    }

    pub(crate) fn close(&self) {
        let mut state = self.0.lock().expect("library update state");
        state.closed = true;
        for sender in state.subscribers.drain(..) {
            sender.close();
        }
    }

    /// Workers were stopped for a folder change; their durable requests may
    /// resume, but their interrupted progress must no longer appear active.
    pub fn reset_transfer_progress(&self) {
        let mut state = self.0.lock().expect("library update state");
        let interrupted: Vec<_> = state.downloads.iter().filter_map(|(&book_id, download)| {
            matches!(download, DownloadState::Queued | DownloadState::Downloading(_)).then_some(book_id)
        }).collect();
        for book_id in interrupted {
            state.downloads.remove(&book_id);
            state.send(LibraryUpdate::Download(BookDownloadStatus::new(book_id, DownloadState::NotDownloaded)));
        }
    }
}

impl UpdateState {
    fn send(&mut self, update: LibraryUpdate) {
        self.subscribers.retain(|sender| sender.try_send(update.clone()).is_ok());
    }
}

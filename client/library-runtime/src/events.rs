//! Event delivery owned by one concrete library session.

use std::sync::{Arc, Mutex};

use library_model::{BookDownloadStatus, DownloadState, LibraryUpdate};
use sync_common::ContentHash;

#[derive(Clone, Debug)]
pub enum LibraryEvent {
    ThumbnailGenerated { content_hash: ContentHash },
    DownloadStatusChanged { content_hash: ContentHash, state: DownloadState },
    TransferStatusChanged,
    StorageChanged,
    ContentsChanged,
    ImportStarted,
    ImportComplete,
    ScanStarted,
    ScanComplete,
}

pub type LibraryEventSender = async_channel::Sender<LibraryEvent>;
pub type LibraryEventReceiver = async_channel::Receiver<LibraryEvent>;

pub fn library_event_channel() -> (LibraryEventSender, LibraryEventReceiver) {
    async_channel::unbounded()
}

/// UI subscriptions live with the session, never in global application state.
#[derive(Clone, Default)]
pub struct LibraryEventSubscriptions {
    subscribers: Arc<Mutex<LibraryEventSubscribers>>,
}

#[derive(Default)]
struct LibraryEventSubscribers {
    updates: Vec<async_channel::Sender<LibraryUpdate>>,
    downloads: Vec<(ContentHash, async_channel::Sender<DownloadState>)>,
}

impl LibraryEventSubscriptions {
    pub fn subscribe_updates(&self) -> async_channel::Receiver<LibraryUpdate> {
        let (sender, receiver) = async_channel::unbounded();
        self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).updates.push(sender);
        receiver
    }

    pub fn subscribe_download(&self, content_hash: ContentHash) -> async_channel::Receiver<DownloadState> {
        let (sender, receiver) = async_channel::unbounded();
        let mut subscribers = self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        subscribers.downloads.retain(|(_, sender)| !sender.is_closed());
        subscribers.downloads.push((content_hash, sender));
        receiver
    }

    pub fn publish(&self, event: LibraryEvent, busy: bool) {
        let mut subscribers = self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let update = match event {
            LibraryEvent::ThumbnailGenerated { content_hash } => LibraryUpdate::Covers(vec![content_hash]),
            LibraryEvent::DownloadStatusChanged { content_hash, state } => {
                subscribers.downloads.retain(|(subscribed_hash, sender)| *subscribed_hash != content_hash || sender.try_send(state.clone()).is_ok());
                LibraryUpdate::Download(BookDownloadStatus::new(content_hash, state))
            }
            LibraryEvent::TransferStatusChanged | LibraryEvent::StorageChanged => return,
            LibraryEvent::ScanStarted | LibraryEvent::ImportStarted => LibraryUpdate::Scanning(busy),
            LibraryEvent::ContentsChanged => LibraryUpdate::Contents,
            LibraryEvent::ScanComplete | LibraryEvent::ImportComplete => {
                send_update(&mut subscribers.updates, LibraryUpdate::Scanning(busy));
                LibraryUpdate::Contents
            }
        };
        send_update(&mut subscribers.updates, update);
    }

    pub fn publish_update(&self, update: LibraryUpdate) {
        send_update(&mut self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).updates, update);
    }
}

fn send_update(subscribers: &mut Vec<async_channel::Sender<LibraryUpdate>>, update: LibraryUpdate) {
    subscribers.retain(|sender| sender.try_send(update.clone()).is_ok());
}

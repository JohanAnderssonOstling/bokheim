//! Event delivery owned by one open library.

use async_channel::{Receiver, Sender};

use crate::reading_positions::ReadingPositionPolicy;
use crate::scheduler::SyncScheduler;
use crate::update_publisher::LibraryUpdatePublisher;
use library_model::{BookDownloadStatus, DownloadState, LibraryUpdate};
use sync_common::BookId;
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub enum LibraryEvent {
    /// Selection is reported with the policy revision observed by the scan.
    ReadingPositionsSelected(Vec<(BookId, u64)>),
    /// Independent work has written its files, or failed before doing so.
    /// The live library consumes this notification; retired libraries ignore it.
    ThumbnailFinished { book_id: BookId, result: Result<Option<bool>, Option<client_protocol::Failure>> },
    /// Committed content changes wake dependent workers and refresh views.
    ContentsChanged,
    /// Reconcile search removals before publishing a contents refresh.
    RefreshContents,
    /// Recalculate activity from durable download requests after a state transition.
    RefreshFileActivity,
}

/// Workers publish presentation updates and wake schedulers directly. Only
/// events requiring owner state or storage access enter the event queue.
#[derive(Clone)]
pub struct LibraryEventRouter {
    pub updates: LibraryUpdatePublisher,
    pub text_search: watch::Sender<()>,
    pub thumbnails: watch::Sender<()>,
    sender: Sender<LibraryEvent>,
    notifications: watch::Sender<()>,
    pub reading_positions: watch::Receiver<ReadingPositionPolicy>,
    pub sync: SyncScheduler,
    pub assets: SyncScheduler,
    pub files: SyncScheduler,
    pub enrichment: SyncScheduler,
}

impl LibraryEventRouter {
    pub fn new() -> (Self, LibraryEventState) {
        let (sender, receiver) = async_channel::unbounded();
        let (position_updates, reading_positions) = watch::channel(ReadingPositionPolicy::default());
        let updates = LibraryUpdatePublisher::default();
        (
            Self {
                updates: updates.clone(),
                text_search: watch::channel(()).0,
                thumbnails: watch::channel(()).0,
                notifications: watch::channel(()).0,
                sender,
                reading_positions,
                sync: SyncScheduler::default(),
                assets: SyncScheduler::default(),
                files: SyncScheduler::default(),
                enrichment: SyncScheduler::default(),
            },
            LibraryEventState { receiver, position_updates, updates, closed: false },
        )
    }

    /// Wake synchronization after a commit. The owner coalesces content updates
    /// while it holds the library state lock.
    pub fn notify_contents_changed(&self) {
        self.sync.wake();
        self.emit(LibraryEvent::ContentsChanged);
    }

    pub fn emit(&self, event: LibraryEvent) {
        if self.sender.try_send(event).is_ok() {
            self.notifications.send_replace(());
        }
    }

    /// Wakes the owner without removing events before it has the library gate.
    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.notifications.subscribe()
    }

    /// Contents refreshes need storage access to reconcile search removals.
    pub fn refresh_contents(&self) {
        self.emit(LibraryEvent::RefreshContents);
    }

    pub fn cover_changed(&self, book_id: BookId) {
        if self.sender.is_closed() {
            return;
        }
        self.assets.wake();
        self.updates.publish(LibraryUpdate::Covers(vec![book_id]));
    }

    pub fn download_status_changed(&self, book_id: BookId, state: DownloadState) {
        if self.sender.is_closed() {
            return;
        }
        if state == DownloadState::Downloaded {
            self.thumbnails.send_replace(());
            self.assets.wake();
            self.files.wake();
        }
        self.text_search.send_replace(());
        let refresh_activity = !matches!(state, DownloadState::Downloading(_));
        self.updates.publish(LibraryUpdate::Download(BookDownloadStatus::new(book_id, state)));
        // Progress is presentation-only. Queue activity reconciliation only
        // when the set of durable download requests may have changed.
        if refresh_activity {
            self.emit(LibraryEvent::RefreshFileActivity);
        }
    }

    fn contents_changed(&self) {
        self.thumbnails.send_replace(());
        self.text_search.send_replace(());
        self.assets.wake();
        self.files.wake();
    }
}

/// Reading-position policy and queued completion events belong to the library owner.
/// Presentation state and subscriptions live in the separate update publisher.
pub struct LibraryEventState {
    position_updates: watch::Sender<ReadingPositionPolicy>,
    receiver: Receiver<LibraryEvent>,
    closed: bool,
    pub updates: LibraryUpdatePublisher,
}

impl LibraryEventState {
    pub fn reading_position_changed(&mut self, hash: BookId, router: &LibraryEventRouter) {
        self.position_updates.send_modify(|policy| policy.changed(hash, web_time::Instant::now()));
        // Every edit schedules its quiet deadline. An earlier maximum-wait
        // wake is retained by the scheduler; unrelated passes re-arm it.
        router.sync.wake_after(std::time::Duration::from_secs(5));
    }

    pub fn receiver(&self) -> Receiver<LibraryEvent> {
        self.receiver.clone()
    }

    /// Apply queued coordination events without retaining a borrow across an await.
    pub fn drain(&mut self, router: &LibraryEventRouter) {
        while let Ok(event) = self.receiver.try_recv() {
            self.apply(event, router);
        }
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.receiver.close();
        while self.receiver.try_recv().is_ok() {}
        self.updates.close();
    }

    pub fn apply(&mut self, event: LibraryEvent, router: &LibraryEventRouter) {
        if self.closed {
            return;
        }
        if let LibraryEvent::ReadingPositionsSelected(positions) = event {
            self.position_updates.send_modify(|policy| policy.selected(&positions));
            return;
        }
        let update = match event {
            LibraryEvent::ContentsChanged => {
                router.contents_changed();
                LibraryUpdate::Contents
            }
            LibraryEvent::RefreshContents => LibraryUpdate::Contents,
            LibraryEvent::ReadingPositionsSelected(_) | LibraryEvent::ThumbnailFinished { .. } | LibraryEvent::RefreshFileActivity => return,
        };
        self.updates.publish(update);
    }
}

impl Drop for LibraryEventState {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::Schedule;
    use library_model::DownloadProgress;

    fn fixture() -> (LibraryEventRouter, LibraryEventState, watch::Receiver<Schedule>) {
        let (router, state) = LibraryEventRouter::new();
        let requests = router.sync.subscribe();
        (router, state, requests)
    }

    #[test]
    fn sync_status_reaches_all_subscribers_without_scheduling_another_pass() {
        let (router, mut state) = LibraryEventRouter::new();
        let sync_requests = router.sync.subscribe();
        let asset_requests = router.assets.subscribe();
        let file_requests = router.files.subscribe();
        let enrichment_requests = router.enrichment.subscribe();
        let first = state.updates.subscribe();
        let second = state.updates.subscribe();

        router.updates.publish(LibraryUpdate::SyncStatusChanged);
        state.drain(&router);

        assert_eq!(first.try_recv().unwrap(), LibraryUpdate::SyncStatusChanged);
        assert_eq!(second.try_recv().unwrap(), LibraryUpdate::SyncStatusChanged);
        assert!(first.try_recv().is_err());
        assert!(second.try_recv().is_err());
        for requests in [sync_requests, asset_requests, file_requests, enrichment_requests] {
            assert!(!requests.has_changed().unwrap());
        }
    }

    #[test]
    fn download_completion_wakes_durable_workers_and_keeps_the_presentation_identity() {
        let (router, mut state) = LibraryEventRouter::new();
        let sync_requests = router.sync.subscribe();
        let asset_requests = router.assets.subscribe();
        let file_requests = router.files.subscribe();
        let enrichment_requests = router.enrichment.subscribe();
        let updates = state.updates.subscribe();
        let hash = BookId::from_bytes([7; 32]);
        for _ in 0..2 {
            router.download_status_changed(hash, DownloadState::Downloaded);
        }
        state.drain(&router);
        for mut requests in [asset_requests, file_requests] {
            assert!(requests.has_changed().unwrap());
            requests.borrow_and_update();
            assert!(!requests.has_changed().unwrap());
        }
        assert!(!sync_requests.has_changed().unwrap());
        assert!(!enrichment_requests.has_changed().unwrap());
        assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::Download(BookDownloadStatus::new(hash, DownloadState::Downloaded)));
        assert!(updates.try_recv().is_err(), "duplicate completion only wakes workers");
    }

    #[test]
    fn queued_progress_is_owned_without_subscribers_and_cannot_regress_completion() {
        let (router, mut state, _) = fixture();
        let hash = BookId::from_bytes([7; 32]);
        let downloading = DownloadState::Downloading(DownloadProgress::ZERO);
        router.download_status_changed(hash, downloading.clone());
        state.drain(&router);
        assert_eq!(state.updates.download_state(&hash), Some(downloading.clone()));

        let updates = state.updates.subscribe();
        router.download_status_changed(hash, DownloadState::Downloaded);
        router.download_status_changed(hash, downloading);
        state.drain(&router);
        assert_eq!(state.updates.download_state(&hash), Some(DownloadState::Downloaded));
        assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::Download(BookDownloadStatus::new(hash, DownloadState::Downloaded)));
        assert!(updates.try_recv().is_err());

        state.updates.reset_download(&hash);
        router.download_status_changed(hash, DownloadState::Queued);
        state.drain(&router);
        assert_eq!(state.updates.download_state(&hash), Some(DownloadState::Queued));
    }

    #[test]
    fn sync_selection_uses_its_snapshot_revision_and_preserves_a_newer_edit() {
        let (router, mut state, _) = fixture();
        let hash = BookId::from_bytes([7; 32]);
        state.reading_position_changed(hash, &router);
        let scan_policy = router.reading_positions.borrow().clone();
        let selected = scan_policy.revisions([hash]);
        state.reading_position_changed(hash, &router);
        router.emit(LibraryEvent::ReadingPositionsSelected(selected));
        state.drain(&router);
        let current = router.reading_positions.borrow().clone();
        assert_ne!(scan_policy.revisions([hash]), current.revisions([hash]));
        assert!(current.waiting(web_time::Instant::now()).contains(&hash));
        router.emit(LibraryEvent::ReadingPositionsSelected(current.revisions([hash])));
        state.drain(&router);
        assert!(router.reading_positions.borrow().revisions([hash]).is_empty());
    }

    #[test]
    fn close_discards_pending_events_and_rejects_stale_emitters() {
        let (router, mut state, mut sync) = fixture();
        let updates = state.updates.subscribe();
        router.refresh_contents();
        state.close();
        state.drain(&router);
        router.refresh_contents();
        state.drain(&router);
        assert!(updates.is_closed());
        assert!(state.updates.subscribe().is_closed());
        assert!(router.sender.is_closed());
        assert!(!sync.has_changed().unwrap());
    }
}

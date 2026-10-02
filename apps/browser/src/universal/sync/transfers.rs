//! Current local file work and network transfers for each library.
//!
//! This is the queue state the account page once listed as "batch jobs" under
//! the libraries. A queue of its own answered "what is this device doing"; the
//! question people actually arrive with is "what is *this library* doing", so
//! the state now lives here as a per-library summary and is drawn on the
//! library's own row.
//!
//! Activity subscriptions stay in an entity of their own because progress changes
//! several times a second, and the settings controls around the library list
//! must not be rebuilt at that rate.

#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;
use std::{sync::Arc, time::Duration};
use web_time::Instant;

#[path = "activity_display.rs"]
mod display;
use display::{ActivityDisplay, ActivityKind};

#[cfg(not(target_arch = "wasm32"))]
use app::AppClient;
use gpui::{Context, Task};
use library_model::{LibraryActivitySnapshot, TransferJobKind, TransferState};
use sync_common::LibraryId;

/// Whether the current activity is running or waiting.
/// The page turns it into a colour; the summary itself carries no styling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivityState {
    /// Work is under way now.
    Running,
    /// Work is queued or waiting to be retried.
    Waiting,
}

/// One line about a library, and — whenever the job can count — how far it has
/// got.
///
/// `count` and `progress` are the same quantity twice, in the two forms a
/// reader wants: the exact one and the shape of it. They are kept out of `text`
/// so that the lines under a library form columns instead of four sentences
/// with a number somewhere in the middle of each. A job that cannot count sets
/// neither, and the empty track that results is itself the report.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LibraryActivitySummary {
    kind: ActivityKind,
    urgent: bool,
    pub(super) state: ActivityState,
    pub(super) text: String,
    pub(super) count: Option<String>,
    pub(super) progress: Option<f32>,
}

/// A job's progress in both forms, from what it has done and what it has to do.
///
/// One place, so a summary cannot report a fraction that disagrees with its own
/// numbers, and so that a total of zero — which every job passes through before
/// it knows its size — reports nothing rather than dividing by it.
fn counted(completed: impl Into<u64>, total: impl Into<u64>) -> (Option<String>, Option<f32>) {
    let (completed, total) = (completed.into(), total.into());
    if total == 0 {
        return (None, None);
    }
    (Some(format!("{completed} / {total}")), Some((completed as f32 / total as f32).clamp(0.0, 1.0)))
}

/// The transfer API is snapshot-based, so refresh while a library has work
/// that can change without another UI action. Idle libraries do not poll.
fn has_live_activity(group: &LibraryActivitySnapshot) -> bool {
    group.scan_pending
        || group.pending_thumbnails > 0
        || group.pending_uploads > 0
        || group.pending_covers > 0
        || group.pending_changes > 0
        || group.file_work_pending
        || group.scanner_running
        || group.thumbnail_work_running
        || group.metadata_sync_running
        || group.preparing_transfers
        || group.transfers.iter().any(|transfer| transfer.state != TransferState::Completed)
}

pub(super) struct LibraryActivity {
    #[cfg(not(target_arch = "wasm32"))]
    backend: Rc<AppClient>,
    #[cfg(target_arch = "wasm32")]
    events: async_channel::Sender<super::SyncEvent>,
    groups: Arc<Vec<LibraryActivitySnapshot>>,
    display: ActivityDisplay,
    display_timer: Option<Task<()>>,
    refresh_scheduled: bool,
    refresh_active: bool,
    refresh_pending: bool,
}

impl LibraryActivity {
    pub(super) fn new(#[cfg(not(target_arch = "wasm32"))] backend: Rc<AppClient>, #[cfg(target_arch = "wasm32")] events: Option<async_channel::Sender<super::SyncEvent>>, _: &mut Context<Self>) -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            backend,
            #[cfg(target_arch = "wasm32")]
            events: events.expect("web sync event inbox"),
            groups: Arc::new(Vec::new()),
            display: ActivityDisplay::default(),
            display_timer: None,
            refresh_scheduled: false,
            refresh_active: false,
            refresh_pending: false,
        }
    }

    /// Every current local activity; idle libraries have no activity lines.
    pub(super) fn summaries(&self, library_id: &LibraryId) -> Vec<LibraryActivitySummary> {
        self.display.rows(*library_id, false)
    }

    pub(super) fn thumbnail_download_coverage(&self, library_id: &LibraryId) -> Option<(u64, u64)> {
        self.groups.iter().find(|group| &group.library_id == library_id).map(|group| group.thumbnail_download_coverage)
    }

    pub(super) fn set_groups(&mut self, groups: Vec<LibraryActivitySnapshot>, cx: &mut Context<Self>) {
        self.set_shared_groups(Arc::new(groups), cx);
    }

    fn set_shared_groups(&mut self, groups: Arc<Vec<LibraryActivitySnapshot>>, cx: &mut Context<Self>) {
        self.groups = groups;
        let rows = self.groups.iter().flat_map(|group| summarize(group).into_iter().chain(transfer_summaries(group)).map(|row| (group.library_id, row))).collect();
        self.display.update(rows, Instant::now());
        self.advance_display(cx);
        if self.groups.iter().any(has_live_activity) {
            self.schedule_refresh_after(Duration::from_secs(1), cx);
        }
    }

    pub(super) fn server_summaries(&self, library_id: &LibraryId) -> Vec<LibraryActivitySummary> {
        self.display.rows(*library_id, true)
    }

    /// A library was created or changed outside the transfer snapshot's normal
    /// active-work polling loop. Fetch it now; if it contains work,
    /// `set_groups` starts the bounded polling loop again.
    pub(super) fn refresh_now(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    fn advance_display(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let changed = self.display.advance(now);
        self.display_timer = self.display.next_deadline().map(|deadline| {
            let timer = cx.background_executor().timer(deadline.saturating_duration_since(now));
            #[cfg(target_arch = "wasm32")]
            let events = self.events.clone();
            cx.spawn(async move |activity, cx| {
                timer.await;
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = events.try_send(super::SyncEvent::TransferDisplayTick);
                    let _ = (activity, cx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    super::after_browser_callback(cx).await;
                    let _ = activity.update(cx, |activity, cx| activity.advance_display(cx));
                }
            })
        });
        if changed {
            cx.notify();
        }
    }

    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        self.schedule_refresh_after(Duration::from_millis(200), cx);
    }

    fn schedule_refresh_after(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if self.refresh_scheduled {
            return;
        }
        self.refresh_scheduled = true;
        let timer = cx.background_executor().timer(delay);
        #[cfg(target_arch = "wasm32")]
        let events = self.events.clone();
        cx.spawn(async move |activity, cx| {
            timer.await;
            #[cfg(target_arch = "wasm32")]
            {
                let _ = events.try_send(super::SyncEvent::TransferRefreshDue);
                let _ = (activity, cx);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                super::after_browser_callback(cx).await;
                let _ = activity.update(cx, |activity, cx| {
                    activity.refresh_scheduled = false;
                    activity.refresh(cx);
                });
            }
        })
        .detach();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_active {
            self.refresh_pending = true;
            return;
        }
        self.refresh_active = true;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = self.events.try_send(super::SyncEvent::TransferRefreshRequested);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let backend = self.backend.clone();
            let operation = async move { backend.transfers().await };
            cx.spawn(async move |activity, cx| {
                let result = operation.await;
                super::after_browser_callback(cx).await;
                let _ = activity.update(cx, |activity, cx| {
                    activity.refresh_active = false;
                    match result {
                        Ok(groups) => {
                            activity.set_groups(groups, cx);
                        }
                        Err(error) => {
                            log::error!("failed to refresh library activity: {error}");
                            activity.set_groups(Vec::new(), cx);
                        }
                    }
                    if std::mem::take(&mut activity.refresh_pending) {
                        activity.schedule_refresh(cx);
                    }
                });
            })
            .detach();
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn refresh_due(&mut self, cx: &mut Context<Self>) {
        self.refresh_scheduled = false;
        self.refresh(cx);
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn display_tick(&mut self, cx: &mut Context<Self>) {
        self.advance_display(cx);
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn apply_result(&mut self, result: Result<(), String>, groups: Arc<Vec<LibraryActivitySnapshot>>, cx: &mut Context<Self>) {
        self.refresh_active = false;
        match result {
            Ok(()) => self.set_shared_groups(groups, cx),
            Err(error) => {
                log::error!("failed to refresh library activity: {error}");
                self.set_groups(Vec::new(), cx);
            }
        }
        if std::mem::take(&mut self.refresh_pending) {
            self.schedule_refresh(cx);
        }
    }
}

/// Local activities remain independent when file work, scans and thumbnails overlap.
fn summarize(group: &LibraryActivitySnapshot) -> Vec<LibraryActivitySummary> {
    let mut summaries = Vec::new();
    if group.storage_quota_blocked {
        summaries.push(LibraryActivitySummary {
            kind: ActivityKind::StorageQuota,
            urgent: true,
            state: ActivityState::Waiting,
            text: if group.uploaded_books > 0 {
                "Partially uploaded · some books need more server storage. Uploaded books are kept; remaining books retry automatically."
            } else {
                "Waiting for server storage · books remain on this device and retry automatically."
            }
            .to_owned(),
            count: None,
            progress: None,
        });
    }
    if group.file_work_pending {
        summaries.push(LibraryActivitySummary {
            kind: ActivityKind::Files,
            urgent: group.file_work_error.is_some(),
            state: ActivityState::Waiting,
            text: if group.file_work_error.is_some() { "Library files will be retried automatically".to_owned() } else { "Updating library files".to_owned() },
            count: None,
            progress: None,
        });
    }
    if let Some(progress) = group.scan_progress.filter(|_| group.scanner_running) {
        let completed = progress.succeeded + progress.failed;
        let (count, fraction) = counted(completed, progress.total);
        summaries.push(LibraryActivitySummary {
            kind: ActivityKind::Scan,
            urgent: false,
            state: ActivityState::Running,
            text: if progress.total == 0 { "Scanning library · checking local files".to_owned() } else { format!("Scanning library · {} added · {} failed", progress.succeeded, progress.failed) },
            count,
            progress: fraction,
        });
    } else if group.scanner_running {
        summaries.push(LibraryActivitySummary { kind: ActivityKind::Scan, urgent: false, state: ActivityState::Running, text: "Scanning library · checking local files".to_owned(), count: None, progress: None });
    } else if group.scan_pending {
        summaries.push(LibraryActivitySummary {
            kind: ActivityKind::Scan,
            urgent: group.scan_failed,
            state: ActivityState::Waiting,
            text: if group.scan_failed { "Scanning library · waiting to retry" } else { "Scanning library · queued" }.to_owned(),
            count: None,
            progress: None,
        });
    }
    if group.thumbnail_work_running || group.pending_thumbnails > 0 {
        let mut text = "Generating thumbnails".to_owned();
        let batch = group.thumbnail_progress.filter(|progress| progress.total > 0);
        if group.thumbnail_waiting {
            text.push_str(if group.scanner_running && batch.is_none_or(|progress| progress.succeeded >= progress.total) { " · waiting for scan" } else { " · waiting to retry" });
        } else if !group.thumbnail_work_running {
            text.push_str(" · queued");
        }
        let (count, progress) = batch.map(|batch| counted(batch.succeeded + batch.failed, batch.total)).unwrap_or((Some(format!("{} pending", group.pending_thumbnails)).filter(|_| group.pending_thumbnails > 0), None));
        summaries.push(LibraryActivitySummary {
            kind: ActivityKind::Thumbnails,
            urgent: false,
            state: if group.thumbnail_waiting || !group.thumbnail_work_running { ActivityState::Waiting } else { ActivityState::Running },
            text,
            count,
            progress,
        });
    }
    summaries
}

/// Show specific metadata, preparation, and transfer work without an
/// uncounted umbrella row between batches.
pub(super) fn transfer_summaries(group: &LibraryActivitySnapshot) -> Vec<LibraryActivitySummary> {
    let mut summaries = Vec::new();
    if group.metadata_sync_running || group.pending_changes > 0 {
        summaries.push(LibraryActivitySummary {
            kind: ActivityKind::Metadata,
            urgent: group.sync_configured && !group.signed_in && group.pending_changes > 0,
            state: if group.metadata_sync_running { ActivityState::Running } else { ActivityState::Waiting },
            text: if group.sync_configured && !group.signed_in {
                "Syncing library changes · sign in to resume"
            } else if group.metadata_sync_running {
                "Syncing library changes"
            } else {
                "Syncing library changes · queued"
            }
            .to_owned(),
            count: (group.pending_changes > 0).then(|| format!("{} pending", group.pending_changes)),
            progress: None,
        });
    }
    if group.preparing_transfers {
        summaries.push(LibraryActivitySummary { kind: ActivityKind::Preparation, urgent: false, state: ActivityState::Running, text: "Preparing file transfers".to_owned(), count: None, progress: None });
    }
    for (kind, pending) in [(TransferJobKind::UploadBook, group.pending_uploads), (TransferJobKind::UploadThumbnail, group.pending_covers)] {
        if pending > 0 && !group.transfers.iter().any(|transfer| transfer.kind == kind && transfer.state != TransferState::Completed) {
            let (count, progress) = if kind == TransferJobKind::UploadBook { counted(group.uploaded_books, group.total_books) } else { (None, None) };
            summaries.push(LibraryActivitySummary {
                kind: ActivityKind::Transfer(kind),
                urgent: !group.signed_in,
                state: ActivityState::Waiting,
                text: format!("{} · {}", kind.batch_label(), if group.signed_in { "queued" } else { "sign in to resume" }),
                count: count.or_else(|| Some(format!("{pending} pending"))),
                progress,
            });
        }
    }
    summaries.extend(group.transfers.iter().filter_map(|transfer| {
        // A queued or retrying batch knows exactly as much about itself as a
        // running one. It used to report none of it, so a transfer that stalled
        // lost the progress it had already made — the moment the number matters
        // most.
        let (count, progress) = counted(transfer.completed_items, transfer.total_items);
        let (state, text, count, progress) = match transfer.state {
            TransferState::Running => (ActivityState::Running, transfer.kind.batch_label().to_owned(), count, progress),
            TransferState::Queued if transfer.kind == TransferJobKind::UploadThumbnail && transfer.completed_items == transfer.total_items && group.thumbnail_work_running => {
                (ActivityState::Waiting, format!("{} · waiting for thumbnails", transfer.kind.batch_label()), count, progress)
            }
            TransferState::Queued => (ActivityState::Waiting, format!("{} · queued", transfer.kind.batch_label()), count, progress),
            TransferState::Retrying => (ActivityState::Waiting, format!("{} · retrying, attempt {}", transfer.kind.batch_label(), transfer.attempts), count, progress),
            // A failure reports where it stopped rather than how far it got:
            // the bar would otherwise read as work still under way.
            TransferState::Failed => (ActivityState::Waiting, format!("{} · failed: {}", transfer.kind.batch_label(), transfer.last_error.as_deref().unwrap_or("transfer failed")), count, None),
            TransferState::Completed => return None,
        };
        Some(LibraryActivitySummary { kind: ActivityKind::Transfer(transfer.kind), urgent: matches!(transfer.state, TransferState::Retrying | TransferState::Failed), state, text, count, progress })
    }));
    summaries
}

#[cfg(test)]
mod tests {
    use super::*;
    use app::ContentHash;
    use library_model::{TransferJobKind, TransferOrigin, TransferStatus};

    #[test]
    fn storage_warning_preserves_other_work_and_clears_with_quota_status() {
        let mut library = group(true, Vec::new(), Vec::new());
        library.storage_quota_blocked = true;
        library.uploaded_books = 3;
        let rows = summarize(&library);
        assert!(rows.iter().any(|row| row.kind == ActivityKind::StorageQuota && row.urgent && row.text.starts_with("Partially uploaded")));
        assert!(rows.iter().any(|row| row.kind == ActivityKind::Scan));
        library.uploaded_books = 0;
        assert!(summarize(&library)[0].text.starts_with("Waiting for server storage"));
        library.storage_quota_blocked = false;
        assert!(summarize(&library).iter().all(|row| row.kind != ActivityKind::StorageQuota));
    }

    #[test]
    fn uploads_remain_visible_during_scanning() {
        let mut scanning = group(true, vec![transfer(TransferJobKind::UploadBook, TransferState::Running, 2, 5, "1"), transfer(TransferJobKind::UploadThumbnail, TransferState::Running, 3, 8, "1")], vec![]);
        scanning.scan_progress = Some(library_model::ScanProgress { total: 10, succeeded: 3, failed: 0 });
        let uploads = transfer_summaries(&scanning);
        assert_eq!(uploads.len(), 2);
        assert_eq!(uploads[0].text, "Uploading books");
        assert_eq!(uploads[0].count.as_deref(), Some("2 / 5"));
        assert_eq!(uploads[0].progress, Some(0.4));
        assert_eq!(uploads[1].text, "Uploading thumbnails");
        assert_eq!(uploads[1].count.as_deref(), Some("3 / 8"));
        assert_eq!(uploads[1].progress, Some(3. / 8.));
        assert!(summarize(&scanning).remove(0).text.starts_with("Scanning library"));
        scanning.scanner_running = false;
        scanning.scan_progress = None;
        assert!(summarize(&scanning).is_empty(), "network activity must not appear under device operations");
        assert!(transfer_summaries(&group(false, vec![], vec![])).is_empty());
    }

    #[test]
    fn scanning_shows_only_specific_server_work() {
        let mut library = group(true, vec![], vec![]);
        let stable = transfer_summaries(&library);
        assert!(stable.is_empty());
        library.metadata_sync_running = true;
        library.preparing_transfers = true;
        library.transfers = vec![transfer(TransferJobKind::UploadBook, TransferState::Running, 0, 1, "1")];
        let uploading = transfer_summaries(&library);
        assert_eq!(uploading.len(), 3);
        assert_eq!(uploading[2].text, "Uploading books");
        assert_eq!(uploading[2].count.as_deref(), Some("0 / 1"));
        library.transfers.clear();
        library.metadata_sync_running = false;
        library.preparing_transfers = false;
        assert_eq!(transfer_summaries(&library), stable);
        library.transfers = vec![transfer(TransferJobKind::UploadBook, TransferState::Retrying, 0, 1, "2")];
        let retry = transfer_summaries(&library);
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].state, ActivityState::Waiting);
        assert!(retry[0].text.contains("retrying"));
        assert_eq!(retry[0].count.as_deref(), Some("0 / 1"));
        library.transfers[0].state = TransferState::Failed;
        assert!(transfer_summaries(&library)[0].text.contains("failed"));
        library.scanner_running = false;
        library.transfers[0].state = TransferState::Running;
        let running = transfer_summaries(&library).remove(0);
        assert_eq!(running.text, "Uploading books");
        assert_eq!(running.count.as_deref(), Some("0 / 1"));
        library.transfers.clear();
        assert!(transfer_summaries(&library).is_empty());
    }

    #[test]
    fn pending_work_remains_visible_without_workers_or_operations() {
        let mut library = group(false, vec![], vec![]);
        library.sync_configured = true;
        library.total_books = 10;
        library.uploaded_books = 3;
        library.pending_uploads = 7;
        library.pending_changes = 2;
        library.pending_thumbnails = 4;
        assert!(has_live_activity(&library));
        assert_eq!(summarize(&library)[0].count.as_deref(), Some("4 pending"));
        let rows = transfer_summaries(&library);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].count.as_deref(), Some("3 / 10"));
        assert_eq!(rows[1].progress, Some(0.3));
        library.signed_in = false;
        assert!(transfer_summaries(&library).iter().all(|row| row.text.contains("sign in to resume")));
        library.pending_uploads = 0;
        library.pending_changes = 0;
        library.pending_thumbnails = 0;
        assert!(!has_live_activity(&library));
        assert!(summarize(&library).is_empty());
        assert!(transfer_summaries(&library).is_empty());
    }

    #[test]
    fn queued_scan_recovery_and_active_transfer_progress_clear_independently() {
        let mut library = group(false, vec![transfer(TransferJobKind::UploadBook, TransferState::Running, 3, 8, "1")], vec![]);
        library.scan_pending = true;
        library.scan_failed = true;
        library.pending_uploads = 5;
        let scan = summarize(&library).remove(0);
        assert!(scan.urgent);
        assert_eq!(scan.state, ActivityState::Waiting);
        assert!(scan.text.contains("waiting to retry"));
        let rows = transfer_summaries(&library);
        assert_eq!(rows.len(), 1, "queue progress replaces the pending-work fallback");
        assert_eq!(rows[0].count.as_deref(), Some("3 / 8"));
        library.scan_pending = false;
        assert!(summarize(&library).is_empty());
        assert_eq!(transfer_summaries(&library), rows);
    }

    fn transfer(kind: TransferJobKind, state: TransferState, completed: u32, total: u32, updated_at: &str) -> TransferStatus {
        TransferStatus {
            id: format!("{}-{updated_at}", kind.storage()),
            kind,
            content_hash: ContentHash::new(&"a".repeat(64)),
            state,
            origin: TransferOrigin::Background,
            attempts: 1,
            completed_items: completed,
            total_items: total,
            failed_content_hash: None,
            file_name: None,
            last_error: None,
            next_retry_at: None,
            created_at: updated_at.to_owned(),
            updated_at: updated_at.to_owned(),
        }
    }

    fn group(scanner_running: bool, transfers: Vec<TransferStatus>, history: Vec<TransferStatus>) -> LibraryActivitySnapshot {
        LibraryActivitySnapshot {
            storage_quota_blocked: false,
            sync_configured: false,
            signed_in: true,
            pending_uploads: 0,
            pending_covers: 0,
            pending_changes: 0,
            pending_thumbnails: 0,
            scan_pending: false,
            scan_failed: false,
            library_id: LibraryId::new_v4(),
            library_name: "Shelf".to_owned(),
            file_work_pending: false,
            file_work_error: None,
            total_books: 0,
            uploaded_books: 0,
            scan_progress: None,
            thumbnail_work_running: false,
            thumbnail_progress: None,
            thumbnail_waiting: false,
            thumbnail_download_coverage: (0, 0),
            metadata_sync_running: false,
            preparing_transfers: false,
            scanner_running,
            transfers,
            history,
        }
    }

    #[test]
    fn pending_file_work_is_visible_without_idle_errors() {
        let mut files = group(false, vec![], vec![]);
        files.file_work_pending = true;
        let status = summarize(&files).remove(0);
        assert_eq!(status.state, ActivityState::Waiting);
        assert_eq!(status.text, "Updating library files");
        files.file_work_error = Some("folder is unavailable".into());
        let status = summarize(&files).remove(0);
        assert_eq!(status.state, ActivityState::Waiting);
        assert_eq!(status.text, "Library files will be retried automatically");
        assert!(status.urgent);
        files.file_work_pending = false;
        assert!(summarize(&files).is_empty());
    }

    #[test]
    fn overlapping_local_activities_remain_visible_and_clear_independently() {
        let mut library = group(true, vec![], vec![]);
        library.file_work_pending = true;
        library.thumbnail_work_running = true;
        library.scan_progress = Some(library_model::ScanProgress { total: 500, succeeded: 120, failed: 0 });
        let summaries = summarize(&library);
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].text, "Updating library files");
        assert_eq!(summaries[1].text, "Scanning library · 120 added · 0 failed");
        assert_eq!(summaries[1].count.as_deref(), Some("120 / 500"));
        assert_eq!(summaries[1].progress, Some(0.24));
        assert_eq!(summaries[2].text, "Generating thumbnails");
        library.file_work_pending = false;
        assert_eq!(summarize(&library), summaries[1..]);
        library.scan_progress = None;
        library.scanner_running = false;
        assert_eq!(summarize(&library), summaries[2..]);
        library.thumbnail_work_running = false;
        assert!(summarize(&library).is_empty());
    }

    #[test]
    fn thumbnail_batch_keeps_progress_visible_while_waiting() {
        let mut library = group(false, vec![], vec![]);
        library.thumbnail_work_running = true;
        library.thumbnail_progress = Some(library_model::ScanProgress { total: 10, succeeded: 3, failed: 1 });
        let generating = summarize(&library).remove(0);
        assert_eq!(generating.text, "Generating thumbnails");
        assert_eq!(generating.count.as_deref(), Some("4 / 10"));
        library.thumbnail_waiting = true;
        let status = summarize(&library).remove(0);
        assert_eq!(status.text, "Generating thumbnails · waiting to retry");
        assert_eq!(status.count.as_deref(), Some("4 / 10"));
        assert_eq!(status.state, ActivityState::Waiting);
        assert_eq!(status.progress, Some(0.4));
        library.thumbnail_work_running = false;
        assert!(summarize(&library).is_empty());
    }

    #[test]
    fn a_quiet_library_reports_nothing() {
        assert!(summarize(&group(false, Vec::new(), Vec::new())).is_empty());
    }

    #[test]
    fn sync_phases_are_visible_only_while_active() {
        let mut library = group(false, vec![], vec![]);
        library.metadata_sync_running = true;
        library.preparing_transfers = true;
        let summaries = transfer_summaries(&library);
        assert_eq!(summaries.iter().map(|summary| summary.text.as_str()).collect::<Vec<_>>(), ["Syncing library changes", "Preparing file transfers"]);
        library.metadata_sync_running = false;
        library.preparing_transfers = false;
        assert!(transfer_summaries(&library).is_empty());
    }

    #[test]
    fn thumbnail_upload_batch_waits_for_generation_between_uploads() {
        let mut library = group(false, vec![transfer(TransferJobKind::UploadThumbnail, TransferState::Queued, 4, 4, "1")], vec![]);
        library.thumbnail_work_running = true;
        let summaries = transfer_summaries(&library);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].state, ActivityState::Waiting);
        assert_eq!(summaries[0].text, "Uploading thumbnails · waiting for thumbnails");
        assert_eq!(summaries[0].count.as_deref(), Some("4 / 4"));
    }

    #[test]
    fn thumbnail_uploads_and_failures_are_visible_but_completed_work_is_hidden() {
        let transfers = vec![
            transfer(TransferJobKind::UploadBook, TransferState::Running, 12, 40, "2"),
            transfer(TransferJobKind::UploadThumbnail, TransferState::Running, 3, 8, "2"),
            transfer(TransferJobKind::DownloadBook, TransferState::Completed, 3, 3, "1"),
            transfer(TransferJobKind::UploadBook, TransferState::Failed, 0, 1, "1"),
        ];
        let summaries = transfer_summaries(&group(false, transfers, Vec::new()));
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].text, "Uploading books");
        assert_eq!(summaries[0].count.as_deref(), Some("12 / 40"));
        assert_eq!(summaries[0].progress, Some(0.3));
        assert_eq!(summaries[1].text, "Uploading thumbnails");
        assert_eq!(summaries[1].count.as_deref(), Some("3 / 8"));
        // A failed batch reports where it stopped, without a bar that would
        // read as work still under way.
        assert!(summaries[2].text.starts_with("Uploading books · failed"));
        assert_eq!(summaries[2].count.as_deref(), Some("0 / 1"));
        assert_eq!(summaries[2].progress, None);
    }

    #[test]
    fn folder_progress_counts_failures_and_supported_books() {
        let mut library = group(true, vec![], vec![]);
        library.scan_progress = Some(library_model::ScanProgress { total: 20, succeeded: 7, failed: 3 });
        let summary = summarize(&library).remove(0);
        assert_eq!(summary.text, "Scanning library · 7 added · 3 failed");
        assert_eq!(summary.count.as_deref(), Some("10 / 20"));
        assert_eq!(summary.progress, Some(0.5));
        library.scan_progress = Some(library_model::ScanProgress::default());
        assert_eq!(summarize(&library).remove(0).progress, None);
    }

    #[test]
    fn a_scan_is_reported_when_nothing_is_transferring() {
        let summary = summarize(&group(true, Vec::new(), Vec::new())).remove(0);

        assert_eq!(summary.state, ActivityState::Running);
        assert!(summary.text.starts_with("Scanning library"));
    }
}

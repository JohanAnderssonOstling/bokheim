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

use std::{rc::Rc, sync::Arc, time::Instant};

#[path = "activity_display.rs"]
mod display;
use display::{ActivityDisplay, ActivityKind};

use bokheim_backend::{AppClient, LibraryTransfers, TransferJobKind, TransferState};
use gpui::{Context, Task};
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

pub(super) struct LibraryActivity {
    backend: Rc<AppClient>,
    groups: Arc<Vec<LibraryTransfers>>,
    display: ActivityDisplay,
    display_timer: Option<Task<()>>,
    refresh_scheduled: bool,
    refresh_active: bool,
    refresh_pending: bool,
    _updates: Task<()>,
}

impl LibraryActivity {
    pub(super) fn new(backend: Rc<AppClient>, cx: &mut Context<Self>) -> Self {
        let updates_backend = backend.clone();
        let updates = cx.spawn(async move |activity, cx| {
            let changes = match updates_backend.transfer_changes().await {
                Ok(changes) => changes,
                Err(error) => {
                    log::error!("failed to subscribe to transfer updates: {error}");
                    return;
                }
            };
            // Subscribe before reading the initial snapshot so no transition is lost.
            let _ = activity.update(cx, |activity, cx| activity.refresh(cx));
            while changes.recv().await.is_ok() {
                let _ = activity.update(cx, |activity, cx| activity.schedule_refresh(cx));
            }
        });
        Self { backend, groups: Arc::new(Vec::new()), display: ActivityDisplay::default(), display_timer: None, refresh_scheduled: false, refresh_active: false, refresh_pending: false, _updates: updates }
    }

    /// Every current local activity; idle libraries have no activity lines.
    pub(super) fn summaries(&self, library_id: &LibraryId) -> Vec<LibraryActivitySummary> {
        self.display.rows(*library_id, false)
    }

    pub(super) fn set_groups(&mut self, groups: Vec<LibraryTransfers>, cx: &mut Context<Self>) {
        self.groups = Arc::new(groups);
        let rows = self.groups.iter().flat_map(|group| summarize(group).into_iter().chain(transfer_summaries(group)).map(|row| (group.library_id, row))).collect();
        self.display.update(rows, Instant::now());
        self.advance_display(cx);
    }

    pub(super) fn server_summaries(&self, library_id: &LibraryId) -> Vec<LibraryActivitySummary> {
        self.display.rows(*library_id, true)
    }

    fn advance_display(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let changed = self.display.advance(now);
        self.display_timer = self.display.next_deadline().map(|deadline| {
            let timer = cx.background_executor().timer(deadline.saturating_duration_since(now));
            cx.spawn(async move |activity, cx| {
                timer.await;
                let _ = activity.update(cx, |activity, cx| activity.advance_display(cx));
            })
        });
        if changed {
            cx.notify();
        }
    }

    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_scheduled {
            return;
        }
        self.refresh_scheduled = true;
        let timer = cx.background_executor().timer(std::time::Duration::from_millis(200));
        cx.spawn(async move |activity, cx| {
            timer.await;
            let _ = activity.update(cx, |activity, cx| {
                activity.refresh_scheduled = false;
                activity.refresh(cx);
            });
        })
        .detach();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_active {
            self.refresh_pending = true;
            return;
        }
        self.refresh_active = true;
        let backend = self.backend.clone();
        let operation = async move { backend.transfers().await };
        cx.spawn(async move |activity, cx| {
            let result = operation.await;
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

/// Local activities remain independent when file work, scans and thumbnails overlap.
fn summarize(group: &LibraryTransfers) -> Vec<LibraryActivitySummary> {
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
            text: group.file_work_error.as_ref().map(|error| format!("Updating library files · {error}")).unwrap_or_else(|| "Updating library files".to_owned()),
            count: None,
            progress: None,
        });
    }
    if let Some(operation) = &group.operation {
        use bokheim_backend::{LibraryOperationKind as Kind, LibraryOperationState as State};
        if operation.state != State::Completed {
            let name = match operation.kind {
                Kind::Import => "Importing library",
                Kind::Sync => "Syncing library",
            };
            let mut text = name.to_owned();
            if operation.scanning {
                text.push_str(" · discovering files");
            }
            if operation.pending_thumbnails > 0 {
                text.push_str(&format!(" · {} thumbnails pending", operation.pending_thumbnails));
            }
            if operation.pending_uploads > 0 {
                text.push_str(&format!(" · {} uploads pending", operation.pending_uploads));
            }
            if operation.pending_changes > 0 {
                text.push_str(&format!(" · {} changes pending", operation.pending_changes));
            }
            if operation.failures > 0 {
                text.push_str(&format!(" · {} failed", operation.failures));
            }
            if let Some(reason) = &operation.waiting_reason {
                text.push_str(&format!(" · {reason}"));
            }
            // What the operation counts is books uploaded of books found. It is
            // not the whole of the work — thumbnails and changes finish after
            // it — so those stay in the sentence, where they read as what is
            // left rather than as a second, disagreeing fraction.
            let (count, progress) = counted(operation.uploaded_books, operation.books);
            summaries.push(LibraryActivitySummary {
                kind: ActivityKind::Operation,
                urgent: operation.needs_attention,
                state: if operation.state == State::Waiting { ActivityState::Waiting } else { ActivityState::Running },
                text,
                count,
                progress,
            });
        }
        return summaries;
    }
    if let Some(progress) = group.scan_progress {
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
    }
    if group.thumbnail_work_running {
        let mut text = "Generating thumbnails".to_owned();
        let batch = group.thumbnail_progress.filter(|progress| progress.total > 0);
        if group.thumbnail_waiting {
            text.push_str(if group.scanner_running && batch.is_none_or(|progress| progress.succeeded >= progress.total) { " · waiting for scan" } else { " · waiting to retry" });
        }
        let (count, progress) = batch.map(|batch| counted(batch.succeeded, batch.total)).unwrap_or((None, None));
        summaries.push(LibraryActivitySummary { kind: ActivityKind::Thumbnails, urgent: false, state: if group.thumbnail_waiting { ActivityState::Waiting } else { ActivityState::Running }, text, count, progress });
    }
    summaries
}

/// Scans produce many short batches; keep their server row stable between jobs.
/// Once scanning settles, show individual transfer progress again.
pub(super) fn transfer_summaries(group: &LibraryTransfers) -> Vec<LibraryActivitySummary> {
    let operation = group.operation.is_some();
    if !operation && (group.scanner_running || group.scan_progress.is_some()) {
        let retrying = group.transfers.iter().any(|transfer| transfer.state == TransferState::Retrying);
        let failed = group.transfers.iter().any(|transfer| transfer.state == TransferState::Failed);
        return vec![LibraryActivitySummary {
            kind: ActivityKind::ScanSync,
            urgent: retrying || failed,
            state: if retrying || failed { ActivityState::Waiting } else { ActivityState::Running },
            text: if failed {
                "Syncing scanned books and thumbnails · transfer failed"
            } else if retrying {
                "Syncing scanned books and thumbnails · waiting to retry"
            } else {
                "Syncing scanned books and thumbnails"
            }
            .to_owned(),
            // Each small batch has its own denominator, not overall scan progress.
            count: None,
            progress: None,
        }];
    }
    let mut summaries = Vec::new();
    for (kind, running, label) in [(ActivityKind::Metadata, group.metadata_sync_running, "Syncing library changes"), (ActivityKind::Preparation, group.preparing_transfers, "Preparing file transfers")] {
        if !operation && running {
            summaries.push(LibraryActivitySummary { kind, urgent: false, state: ActivityState::Running, text: label.to_owned(), count: None, progress: None });
        }
    }
    summaries.extend(group.transfers.iter().filter(|transfer| !operation || transfer.origin == bokheim_backend::TransferOrigin::UserInitiated).filter_map(|transfer| {
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
    use bokheim_backend::{ContentHash, TransferJobKind, TransferOrigin, TransferStatus};

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
    fn library_operation_survives_scan_completion_and_keeps_explicit_download_separate() {
        use bokheim_backend::{LibraryOperation, LibraryOperationKind, LibraryOperationState};
        let mut library = group(true, vec![transfer(TransferJobKind::UploadBook, TransferState::Running, 0, 1, "1")], vec![]);
        library.operation = Some(LibraryOperation {
            id: "import-1".into(),
            kind: LibraryOperationKind::Import,
            state: LibraryOperationState::Running,
            scanning: true,
            books: 240,
            uploaded_books: 120,
            pending_uploads: 120,
            pending_changes: 5,
            pending_thumbnails: 60,
            failures: 0,
            needs_attention: false,
            waiting_reason: None,
        });
        let first = summarize(&library);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].kind, ActivityKind::Operation);
        assert_eq!(first[0].text, "Importing library · discovering files · 60 thumbnails pending · 120 uploads pending · 5 changes pending");
        assert_eq!(first[0].count.as_deref(), Some("120 / 240"));
        assert_eq!(first[0].progress, Some(0.5));
        assert!(transfer_summaries(&library).is_empty());
        library.scanner_running = false;
        let operation = library.operation.as_mut().unwrap();
        operation.scanning = false;
        operation.state = LibraryOperationState::Waiting;
        operation.waiting_reason = Some("Waiting to retry uploads".into());
        operation.failures = 2;
        operation.needs_attention = true;
        library.transfers.clear();
        let waiting = summarize(&library);
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].kind, first[0].kind);
        assert!(waiting[0].urgent);
        let mut download = transfer(TransferJobKind::DownloadBook, TransferState::Running, 0, 1, "2");
        download.origin = TransferOrigin::UserInitiated;
        library.transfers.push(download);
        assert_eq!(transfer_summaries(&library).len(), 1);
        library.operation.as_mut().unwrap().state = LibraryOperationState::Completed;
        assert!(summarize(&library).is_empty());
        assert_eq!(transfer_summaries(&library).len(), 1);
    }

    #[test]
    fn uploads_remain_visible_during_scanning() {
        let mut scanning = group(true, vec![transfer(TransferJobKind::UploadBook, TransferState::Running, 2, 5, "1")], vec![]);
        scanning.scan_progress = Some(bokheim_backend::ScanProgress { total: 10, succeeded: 3, failed: 0 });
        let summary = transfer_summaries(&scanning).remove(0);
        assert_eq!(summary.text, "Syncing scanned books and thumbnails");
        assert_eq!(summary.progress, None);
        assert!(summarize(&scanning).remove(0).text.starts_with("Scanning library"));
        scanning.scanner_running = false;
        scanning.scan_progress = None;
        assert!(summarize(&scanning).is_empty(), "network activity must not appear under device operations");
        assert!(transfer_summaries(&group(false, vec![], vec![])).is_empty());
    }

    #[test]
    fn scanning_server_summary_survives_small_batches_and_idle_gaps() {
        let mut library = group(true, vec![], vec![]);
        let stable = transfer_summaries(&library);
        library.metadata_sync_running = true;
        library.preparing_transfers = true;
        library.transfers = vec![transfer(TransferJobKind::UploadBook, TransferState::Running, 0, 1, "1")];
        assert_eq!(transfer_summaries(&library), stable);
        library.transfers.clear();
        library.metadata_sync_running = false;
        library.preparing_transfers = false;
        assert_eq!(transfer_summaries(&library), stable);
        library.transfers = vec![transfer(TransferJobKind::UploadBook, TransferState::Retrying, 0, 1, "2")];
        let retry = transfer_summaries(&library);
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].state, ActivityState::Waiting);
        assert!(retry[0].text.contains("waiting to retry"));
        library.transfers[0].state = TransferState::Failed;
        assert!(transfer_summaries(&library)[0].text.contains("transfer failed"));
        library.scanner_running = false;
        library.transfers[0].state = TransferState::Running;
        let running = transfer_summaries(&library).remove(0);
        assert_eq!(running.text, "Uploading books");
        assert_eq!(running.count.as_deref(), Some("0 / 1"));
        library.transfers.clear();
        assert!(transfer_summaries(&library).is_empty());
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

    fn group(scanner_running: bool, transfers: Vec<TransferStatus>, history: Vec<TransferStatus>) -> LibraryTransfers {
        LibraryTransfers {
            storage_quota_blocked: false,
            operation: None,
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
        assert_eq!(status.text, "Updating library files · folder is unavailable");
        assert!(status.urgent);
        files.file_work_pending = false;
        assert!(summarize(&files).is_empty());
    }

    #[test]
    fn overlapping_local_activities_remain_visible_and_clear_independently() {
        let mut library = group(true, vec![], vec![]);
        library.file_work_pending = true;
        library.thumbnail_work_running = true;
        library.scan_progress = Some(bokheim_backend::ScanProgress { total: 500, succeeded: 120, failed: 0 });
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
        library.thumbnail_progress = Some(bokheim_backend::ScanProgress { total: 10, succeeded: 4, failed: 0 });
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
        library.scan_progress = Some(bokheim_backend::ScanProgress { total: 20, succeeded: 7, failed: 3 });
        let summary = summarize(&library).remove(0);
        assert_eq!(summary.text, "Scanning library · 7 added · 3 failed");
        assert_eq!(summary.count.as_deref(), Some("10 / 20"));
        assert_eq!(summary.progress, Some(0.5));
        library.scan_progress = Some(bokheim_backend::ScanProgress::default());
        assert_eq!(summarize(&library).remove(0).progress, None);
    }

    #[test]
    fn a_scan_is_reported_when_nothing_is_transferring() {
        let summary = summarize(&group(true, Vec::new(), Vec::new())).remove(0);

        assert_eq!(summary.state, ActivityState::Running);
        assert!(summary.text.starts_with("Scanning library"));
    }
}

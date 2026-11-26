//! Background workers and event/activity observers for one library.
//! Timed workers ask the database for work; `LibraryEventRouter` wakes them.
//! They share one owned task that stops when the library closes or changes folders.

use account_client::{AccountSessionError, AuthError};
use client_protocol::Failure;
use std::future::Future;
use std::time::Duration;

use super::LibraryResources;
use super::events::{LibraryEvent, LibraryEventRouter};
#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
use super::import::watcher::watch;
use crate::BackendError;
#[cfg(all(feature = "kobo", target_os = "linux"))]
use client_platform_kobo::device::wifi::{self, Phase as WifiPhase};
use client_platform_runtime::executor::{BackendSend, BoxedBackendFuture};
use futures_util::FutureExt;
use library_database::DatabaseClient;
use library_model::LibraryUpdate;
use library_services::account::LibraryAccount;
use library_services::scheduler::SyncScheduler;
use std::panic::AssertUnwindSafe;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy)]
enum WorkerKind {
    Sync,
    Uploads,
    Downloads,
    Files,
    Enrichment,
}

impl WorkerKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sync => "synchronization",
            Self::Uploads => "uploads",
            Self::Downloads => "downloads",
            Self::Files => "file replay",
            Self::Enrichment => "enrichment",
        }
    }

    fn pass(self, session: &LibraryResources, remote: bool) -> BoxedBackendFuture<'_, Result<(), ()>> {
        match self {
            Self::Sync => Box::pin(sync_pass(session, remote)),
            Self::Uploads => Box::pin(asset_pass(session, true)),
            Self::Downloads => Box::pin(asset_pass(session, false)),
            Self::Files => Box::pin(replay_and_prepare(session)),
            Self::Enrichment => Box::pin(async move { session.enrichment.run().await }),
        }
    }

    async fn run(self, session: &LibraryResources, scheduler: &SyncScheduler) {
        // Workers share retry and panic recovery logic.
        // Box only the active pass so its large state does not fill the root.
        run_timed(
            self.name(),
            move |remote| async move {
                if matches!(self, Self::Sync | Self::Uploads | Self::Downloads) {
                    wait_for_network().await?;
                    match session.account.while_authorized(self.pass(session, remote)).await {
                        Ok(result) => result,
                        Err(error @ (AccountSessionError::Changed | AccountSessionError::Authentication(AuthError::SignInRequired(_)))) => {
                            log::debug!("library {} paused until its account is available: {error}", self.name());
                            Ok(())
                        }
                        Err(error) => {
                            log::warn!("could not read account state for library {}: {error}", self.name());
                            Err(())
                        }
                    }
                } else {
                    self.pass(session, remote).await
                }
            },
            scheduler,
        )
        .await;
    }
}

/// Subscribes immediately, so an account change between opening and the first
/// poll is not missed. Credential renewal also wakes workers; only identity
/// changes cancel their in-flight remote work.
pub(super) fn observe_account(account: LibraryAccount, events: LibraryEventRouter) -> impl Future<Output = ()> {
    let mut account_changed = account.subscribe();
    account_changed.borrow_and_update();
    async move {
        while account_changed.changed().await.is_ok() {
            events.emit(LibraryEvent::ContentsChanged);
            events.sync.request_remote_refresh();
            events.refresh_assets();
        }
    }
}

/// Runs every worker until dropped; none of them ends on its own.
pub(super) fn run_workers<'a>(session: &'a LibraryResources, observers: impl Future<Output = ()> + BackendSend + 'a) -> BoxedBackendFuture<'a, ()> {
    Box::pin(async move {
        {
            let database = &session.database;
            if let Err(error) = database.refresh_taxonomy_assignments().await {
                log::warn!("library taxonomy refresh will retry next launch: {error}");
            } else {
                session.events.refresh_contents();
            }
        }
        // Keep each scoped worker on the heap: their state machines contain large
        // parser/network futures, and the owning root must fit a normal thread stack.
        let observers: BoxedBackendFuture<'_, ()> = Box::pin(observers);
        let network: BoxedBackendFuture<'_, ()> = Box::pin(observe_network(session.events.clone()));
        let sync: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Sync.run(session, &session.events.sync));
        let uploads: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Uploads.run(session, &session.events.uploads));
        let downloads: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Downloads.run(session, &session.events.downloads));
        let files: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Files.run(session, &session.events.files));
        let enrichment: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Enrichment.run(session, &session.events.enrichment));
        #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
        let scan: BoxedBackendFuture<'_, ()> = Box::pin(run_scan_worker(session));
        #[cfg(any(not(feature = "scanner"), target_arch = "wasm32"))]
        let scan: BoxedBackendFuture<'_, ()> = Box::pin(async {});
        let text_search: BoxedBackendFuture<'_, ()> = Box::pin(super::browse::text_search::run(session, session.events.text_search.subscribe()));
        let thumbnails: BoxedBackendFuture<'_, ()> = Box::pin(run_thumbnails(session));
        futures_util::join!(observers, network, sync, uploads, downloads, files, enrichment, scan, text_search, thumbnails);
    })
}

pub(super) async fn process_events(resources: &LibraryResources) {
    let mut events = resources.events.subscribe();
    events.mark_changed();
    while events.changed().await.is_ok() {
        let storage = resources.state.write().await;
        resources.drain_events(&storage).await;
    }
}

pub(super) async fn publish_activity(resources: &LibraryResources) {
    let mut changes = resources.activity.subscribe();
    changes.mark_changed();
    while changes.changed().await.is_ok() {
        resources.publish_book_file_activity().await;
    }
}

/// Thumbnail CPU and file work has its own lane and never retains the
/// library's gate. File replay and foreground operations can continue meanwhile.
async fn run_thumbnails(session: &LibraryResources) {
    let mut changes = session.events.thumbnails.subscribe();
    loop {
        let failed = recover_pass("thumbnail generation", async { session.run_thumbnail_jobs().await.map_err(|error| log::warn!("thumbnail work will retry: {error}")) }).await.is_err();
        if failed {
            tokio::select! {
                _ = changes.changed() => {},
                _ = crate::executor::sleep(Duration::from_secs(30)) => {},
            }
        } else if changes.changed().await.is_err() {
            break;
        }
    }
}

async fn observe_network(_events: LibraryEventRouter) {
    #[cfg(all(feature = "kobo", target_os = "linux"))]
    {
        let Ok(mut changes) = wifi::subscribe() else {
            return;
        };
        let mut was_connected = false;
        loop {
            let connected = changes.borrow_and_update().phase == WifiPhase::Connected;
            if connected && !was_connected {
                _events.sync.request_remote_refresh();
                _events.refresh_assets();
            }
            was_connected = connected;
            if changes.changed().await.is_err() {
                break;
            }
        }
    }
}

/// Wakes within 100 ms coalesce. Kobo backs repeated failures off to 15 minutes.
async fn run_timed<F: Future<Output = Result<(), ()>>>(name: &str, mut pass: impl FnMut(bool) -> F, scheduler: &SyncScheduler) {
    let cycle = move |remote| {
        let pending = std::panic::catch_unwind(AssertUnwindSafe(|| pass(remote)));
        async move {
            match pending {
                Ok(pass) => recover_pass(name, pass).await,
                Err(_) => {
                    log::error!("library {name} panicked; queued work will retry");
                    Err(())
                }
            }
        }
    };
    #[cfg(feature = "kobo")]
    library_services::scheduler::run_timed_with_backoff(cycle, scheduler, Duration::from_secs(30), Duration::from_secs(15 * 60), Duration::from_millis(100)).await;
    #[cfg(not(feature = "kobo"))]
    library_services::scheduler::run_timed(cycle, scheduler, Duration::from_secs(30), Duration::from_millis(100)).await;
}

async fn wait_for_network() -> Result<(), ()> {
    #[cfg(all(feature = "kobo", target_os = "linux"))]
    wifi::wait_for_connection().await.map_err(|error| log::warn!("could not observe network availability: {error}"))?;
    Ok(())
}

/// These passes reconstruct their work from committed database state. Drop
/// the failed future and its activity guards before scheduling another pass.
/// Panic recovery applies when the target supports unwinding.
async fn recover_pass(name: &str, pass: impl Future<Output = Result<(), ()>>) -> Result<(), ()> {
    AssertUnwindSafe(pass).catch_unwind().await.unwrap_or_else(|_| {
        log::error!("library {name} panicked; queued work will retry");
        Err(())
    })
}

async fn sync_pass(library: &LibraryResources, remote: bool) -> Result<(), ()> {
    let sync = &library.sync;
    let events = &library.events;
    let result = async {
        if !sync.remote_ready().await.map_err(BackendError::operation)? {
            // Management confirms server registration before sync starts.
            // Keep retrying while account discovery or offline creation waits.
            return Err(BackendError::message("waiting for account session or library registration"));
        }
        if remote { sync.synchronize_state_mode(false).await.map(|_| ()).map_err(BackendError::operation) } else { sync.drain_outbox().await.map(|_| ()).map_err(BackendError::operation) }
    }
    .await;
    // Unrelated sync work may run before a reading position's quiet period ends.
    // Arrange another pass even if no further page turn or event arrives.
    match library.database.sync_queued_reading_positions().await {
        Ok(queued) => {
            let policy = events.reading_positions.borrow();
            let discarded = policy.no_longer_queued(&queued);
            if !discarded.is_empty() {
                events.emit(LibraryEvent::ReadingPositionsSelected(discarded));
            }
            if let Some(delay) = policy.delay(web_time::Instant::now(), &queued) {
                events.sync.wake_after(delay);
            }
        }
        Err(error) => {
            log::warn!("could not schedule reading-position sync; retrying: {error}");
            // A successful network pass must not suppress recovery of the
            // pending reading-position deadlines after a storage failure.
            events.sync.wake_after(Duration::from_secs(30));
        }
    }
    // Committed content changes wake file replay through ContentsChanged.
    // Waking it after an empty pass would loop: file preparation wakes sync.
    result.map_err(|error| log::warn!("library synchronization will resume: {error}"))
}

async fn asset_pass(library: &LibraryResources, uploads: bool) -> Result<(), ()> {
    if !library.sync.remote_ready().await.map_err(|error| log::warn!("could not check asset registration: {error}"))? {
        return Ok(());
    }
    let result = if uploads { library.transfers.run_upload_pass().await } else { library.transfers.run_download_pass().await };
    let quota_blocked = result.as_ref().err().and_then(|error| error.failure()) == Some(Failure::CloudStorageFull);
    if uploads && library.upload_quota_blocked.swap(quota_blocked, Ordering::AcqRel) != quota_blocked {
        library.events.updates.publish(LibraryUpdate::SyncStatusChanged);
    }
    match result {
        Ok(()) | Err(asset_transfer::TransferError::AuthenticationRequired) => Ok(()),
        Err(error) => {
            log::warn!("asset pass will retry: {error}");
            Err(())
        }
    }
}

async fn prepare_uploads(db: &DatabaseClient, events: &LibraryEventRouter) -> Result<(), BackendError> {
    // Only local file work prepares uploads. Enrichment is independent.
    db.prepare_uploads().await?;
    events.sync.wake();
    events.uploads.wake();
    Ok(())
}

/// File replay, then durable upload preparation. A scan or folder import owns the
/// files meanwhile; their completion events wake this worker again.
async fn replay_and_prepare(session: &LibraryResources) -> Result<(), ()> {
    let database = &session.database;
    let result = AssertUnwindSafe(async { session.run_file_jobs().await }).catch_unwind().await.unwrap_or_else(|_| Err(BackendError::message("library file replay panicked")));
    if let Err(error) = &result {
        log::warn!("library file replay will resume: {error}");
    }
    // A lease can leave durable work queued even after a successful pass.
    let failed = result.is_err() || database.has_pending_storage_work().await.unwrap_or(true);
    // Preparation never waits for a failed replay of other books.

    if let Err(error) = prepare_uploads(database, &session.events).await {
        log::warn!("upload preparation will resume: {error}");
        return Err(());
    }

    session.events.thumbnails.send_replace(());
    session.events.text_search.send_replace(());
    session.events.enrichment.wake();
    if failed { Err(()) } else { Ok(()) }
}

#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
type FolderWatcher = super::import::watcher::WatchSubscription;
#[cfg(all(feature = "scanner", not(feature = "watcher"), not(target_arch = "wasm32")))]
type FolderWatcher = ();

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
struct ScanRetry {
    watcher: Option<FolderWatcher>,
    blocked_delay: Duration,
    incomplete_delay: Duration,
    file_progress: u64,
}

/// Android scans on selection; other native hosts also watch and retry changes.
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
async fn run_scan_worker(session: &LibraryResources) {
    let wake = session.events.scan.clone();
    if cfg!(target_os = "android") {
        let mut changes = wake.subscribe();
        loop {
            changes.borrow_and_update();
            if wake.take_pending().wake_requested_at.is_none() {
                if changes.changed().await.is_err() {
                    return;
                }
                continue;
            }
            let _ = recover_pass("filesystem scan", async {
                match session.scan_folder().await {
                    Ok(super::import::filesystem_scan::FolderScan::Incomplete) => {
                        log::warn!("filesystem scan was incomplete; it will retry when the library is opened again");
                        Ok(())
                    }
                    Ok(_) => Ok(()),
                    Err(error) => {
                        log::warn!("filesystem scan failed; it will retry when the library is opened again: {error}");
                        Err(())
                    }
                }
            })
            .await;
            // Opening the same library during its scan shares that pass.
            // Android does not poll or retry scans after file-worker progress.
            wake.take_pending();
        }
    }
    #[cfg(feature = "watcher")]
    let assets = session.state.read().await.assets().clone();
    #[cfg(feature = "watcher")]
    let watcher = {
        let root = assets.native_library_root();
        watch(root, wake.clone()).map_err(|error| log::warn!("could not watch library folder {}: {error}", root.display())).ok()
    };
    #[cfg(not(feature = "watcher"))]
    let watcher: Option<FolderWatcher> = None;
    wake.wake();
    let wake = &wake;
    let state = Arc::new(tokio::sync::Mutex::new(ScanRetry { watcher, blocked_delay: Duration::from_secs(30), incomplete_delay: Duration::from_secs(30), file_progress: session.events.file_progress_revision() }));
    // The debounce is the quiet period while a file manager is still copying.
    library_services::scheduler::run_timed(
        move |_| {
            let state = state.clone();
            recover_pass("filesystem scan", async move { scan_and_watch(session, wake, &mut *state.lock().await).await })
        },
        wake,
        Duration::from_secs(30),
        Duration::from_millis(500),
    )
    .await;
}

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
async fn scan_and_watch(session: &LibraryResources, wake: &SyncScheduler, state: &mut ScanRetry) -> Result<(), ()> {
    let watcher = &mut state.watcher;
    let progress = session.events.file_progress_revision();
    if progress != state.file_progress {
        state.blocked_delay = Duration::from_secs(30);
        state.file_progress = progress;
    }
    // Arm before taking the snapshot so progress during discovery cannot be lost.
    session.events.set_scan_waiting(true);
    #[cfg(feature = "watcher")]
    let assets = session.state.read().await.assets().clone();
    #[cfg(feature = "watcher")]
    if watcher.as_ref().is_some_and(|watcher| !watcher.is_active()) {
        *watcher = None;
    }
    let result = session.scan_folder().await;
    if result.as_ref().is_ok_and(|result| !matches!(result, super::import::filesystem_scan::FolderScan::Incomplete)) {
        state.incomplete_delay = Duration::from_secs(30);
    }
    match result {
        Ok(super::import::filesystem_scan::FolderScan::Complete) => {
            session.events.set_scan_waiting(false);
            state.blocked_delay = Duration::from_secs(30);
        }
        Ok(super::import::filesystem_scan::FolderScan::ChangedDuringScan) => wake.wake_after(Duration::from_secs(1)),
        Ok(super::import::filesystem_scan::FolderScan::Deferred) => {
            wake.wake_after(state.blocked_delay);
            state.blocked_delay = (state.blocked_delay * 2).min(Duration::from_secs(300));
        }
        Ok(super::import::filesystem_scan::FolderScan::Incomplete) => {
            log::warn!("filesystem scan was incomplete; retrying in {} seconds", state.incomplete_delay.as_secs());
            wake.wake_after(state.incomplete_delay);
            state.incomplete_delay = (state.incomplete_delay * 2).min(Duration::from_secs(300));
        }
        Err(error) => {
            log::warn!("filesystem scan failed; retrying in 30 seconds: {error}");
            #[cfg(feature = "watcher")]
            if !assets.native_library_root().is_dir() {
                *watcher = None;
            }
            return Err(());
        }
    }
    #[cfg(feature = "watcher")]
    if watcher.is_none() {
        let root = assets.native_library_root();
        let started = watch(root, wake.clone()).map_err(|error| log::warn!("could not watch library folder {}: {error}", root.display())).ok();
        // Recheck files that may have arrived before registration.
        wake.wake_after(if started.is_some() { Duration::from_millis(250) } else { Duration::from_secs(30) });
        *watcher = started;
    }
    #[cfg(not(feature = "watcher"))]
    let _ = (wake, watcher);
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use library_services::scheduler::SyncScheduler;

    #[cfg(feature = "scanner")]
    #[tokio::test]
    async fn scan_retry_deadlines_back_off_and_reset_after_recovery_and_progress() {
        use crate::library::lifecycle::concurrency_tests::fixture;
        use web_time::Instant;

        // Opt in to the real 17.5-minute cycle when collecting runtime measurements.
        // The default regression below still checks every scheduled deadline quickly.
        if std::env::var_os("BOKHEIM_MEASURE_SCAN_RETRIES").is_some() {
            async fn measure(incomplete: bool) {
                let root = tempfile::tempdir().unwrap();
                let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
                if incomplete {
                    std::fs::create_dir(root.path().join("offline")).unwrap();
                    std::fs::write(root.path().join("offline/.biblos_uuid"), "invalid directory marker").unwrap();
                } else {
                    service.database.create_directory(&sync_common::ROOT_DIR_ID, &"Held".into()).await.unwrap();
                }
                #[cfg(feature = "watcher")]
                let watcher = Some(watch(root.path(), SyncScheduler::default()).unwrap());
                #[cfg(not(feature = "watcher"))]
                let watcher = None;
                let state = Arc::new(tokio::sync::Mutex::new(ScanRetry { watcher, blocked_delay: Duration::from_secs(30), incomplete_delay: Duration::from_secs(30), file_progress: service.events.file_progress_revision() }));
                let wake = SyncScheduler::default();
                wake.wake();
                let (sender, receiver) = async_channel::unbounded();
                let worker = library_services::scheduler::run_timed(
                    |_| {
                        let state = state.clone();
                        let service = &service;
                        let wake = &wake;
                        let sender = &sender;
                        async move {
                            let started = Instant::now();
                            scan_and_watch(service, wake, &mut *state.lock().await).await?;
                            sender.send((started, Instant::now())).await.unwrap();
                            Ok(())
                        }
                    },
                    &wake,
                    Duration::from_secs(30),
                    Duration::from_millis(500),
                );
                let observe = async {
                    let (_, mut previous_finish) = receiver.recv().await.unwrap();
                    for expected in [30, 60, 120, 240, 300, 300] {
                        let (started, finished) = receiver.recv().await.unwrap();
                        let elapsed = started.duration_since(previous_finish).as_secs_f64();
                        println!(
                            "{}",
                            serde_json::json!({"scenario": if incomplete { "incomplete" } else { "deferred" }, "expected_seconds": expected, "observed_seconds": elapsed, "scan_ms": finished.duration_since(started).as_secs_f64() * 1000.0})
                        );
                        assert!(elapsed >= expected as f64 - 0.05, "retry fired early: {elapsed}");
                        assert!(elapsed <= expected as f64 + 5.0, "retry fired late: {elapsed}");
                        previous_finish = finished;
                    }
                };
                tokio::time::timeout(Duration::from_secs(1100), async {
                    tokio::select! {
                        _ = worker => panic!("scanner worker stopped unexpectedly"),
                        () = observe => {}
                    }
                })
                .await
                .expect("full scanner retry cycle timed out");
            }
            // Independent libraries share the same clock, measuring both outcomes in one cycle.
            tokio::join!(measure(true), measure(false));
        }

        let root = tempfile::tempdir().unwrap();
        let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        let directory = root.path().join("offline");
        std::fs::create_dir(&directory).unwrap();
        let marker = directory.join(".biblos_uuid");
        std::fs::write(&marker, "invalid directory marker").unwrap();
        #[cfg(feature = "watcher")]
        let watcher = Some(watch(root.path(), SyncScheduler::default()).unwrap());
        #[cfg(not(feature = "watcher"))]
        let watcher = None;
        let mut state = ScanRetry { watcher, blocked_delay: Duration::from_secs(30), incomplete_delay: Duration::from_secs(30), file_progress: service.events.file_progress_revision() };

        // Each fresh scheduler represents the previous deadline having been consumed.
        // Inspect actual requested deadlines; Tokio's paused clock does not advance web_time.
        for expected in [30, 60, 120, 240, 300, 300] {
            let wake = SyncScheduler::default();
            let before = Instant::now();
            scan_and_watch(&service, &wake, &mut state).await.unwrap();
            let after = Instant::now();
            let schedule = *wake.subscribe().borrow();
            let deadline = schedule.deadline.expect("incomplete scan must retry");
            assert!(deadline >= before + Duration::from_secs(expected));
            assert!(deadline <= after + Duration::from_secs(expected));
            assert!(schedule.wake_requested_at.is_none());
            assert!(!service.scanner_running());
        }
        std::fs::remove_file(&marker).unwrap();
        // The newly discovered directory may require a second pass to converge.
        for _ in 0..2 {
            scan_and_watch(&service, &SyncScheduler::default(), &mut state).await.unwrap();
        }
        let wake = SyncScheduler::default();
        scan_and_watch(&service, &wake, &mut state).await.unwrap();
        assert!(wake.subscribe().borrow().deadline.is_none());
        assert_eq!(state.incomplete_delay, Duration::from_secs(30));

        service.database.create_directory(&sync_common::ROOT_DIR_ID, &"Held".into()).await.unwrap();
        assert!(!service.database.scan_snapshot(book_inspection::inspection_version(book_model::BookFormat::Pdf)).await.unwrap().excluded_paths.is_empty());
        for expected in [30, 60, 120, 240, 300, 300] {
            let wake = SyncScheduler::default();
            let before = Instant::now();
            scan_and_watch(&service, &wake, &mut state).await.unwrap();
            let after = Instant::now();
            let deadline = wake.subscribe().borrow().deadline.expect("deferred scan must retry");
            assert!(deadline >= before + Duration::from_secs(expected));
            assert!(deadline <= after + Duration::from_secs(expected));
        }
        assert!(service.events.scan.subscribe().borrow().wake_requested_at.is_none(), "blocked passes must not immediately wake the scanner");
        let progress = service.events.file_progress_revision();
        service.events.file_work_progressed();
        assert!(service.events.file_progress_revision() > progress);
        assert!(service.events.scan.subscribe().borrow().wake_requested_at.is_some());
        let wake = SyncScheduler::default();
        let before = Instant::now();
        scan_and_watch(&service, &wake, &mut state).await.unwrap();
        let after = Instant::now();
        let deadline = wake.subscribe().borrow().deadline.unwrap();
        assert!(deadline >= before + Duration::from_secs(30));
        assert!(deadline <= after + Duration::from_secs(30));
        assert_eq!(state.blocked_delay, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn worker_retries_factory_and_poll_panics_and_releases_activity() {
        let operation_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let scheduler = SyncScheduler::default();
        let (completed, results) = async_channel::unbounded();
        let mut attempts = 0;
        scheduler.wake();
        let worker = run_timed(
            "recovery test",
            move |_| {
                attempts += 1;
                let attempt = attempts;
                let activity = operation_lock.clone().try_lock_owned().expect("failed passes must release their lock");
                if attempt == 1 {
                    completed.try_send(attempt).unwrap();
                    panic!("failed while creating the pass");
                }
                let completed = completed.clone();
                async move {
                    let _activity = activity;
                    tokio::task::yield_now().await;
                    completed.try_send(attempt).unwrap();
                    if attempt == 2 {
                        panic!("failed while polling the pass");
                    }
                    Ok(())
                }
            },
            &scheduler,
        );
        let exercise = async {
            for attempt in 1..=3 {
                assert_eq!(results.recv().await.unwrap(), attempt);
                if attempt < 3 {
                    // A fresh refresh bypasses the normal retry delay.
                    scheduler.request_remote_refresh();
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! { _ = worker => panic!("worker ended before cancellation"), () = exercise => {} }
        })
        .await
        .expect("a failed pass must not end or strand the worker");
    }
}

//! Background workers and event/activity observers for one library.
//! Timed workers ask the database for work; `LibraryEventRouter` wakes them.
//! They share one owned task that stops when the library closes or changes folders.

use account_client::{AccountSessionError, AuthError};
use client_protocol::Failure;
use std::future::Future;
use std::time::Duration;

use super::events::{LibraryEvent, LibraryEventRouter};
#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
use super::import::watcher::watch;
use super::LibraryResources;
use crate::BackendError;
#[cfg(all(feature = "kobo", target_os = "linux"))]
use client_platform_kobo::device::wifi::{self, Phase as WifiPhase};
use client_platform_runtime::executor::{BackendSend, BoxedBackendFuture};
use client_platform_runtime::transfers;
use futures_util::FutureExt;
use library_database::DatabaseClient;
use library_model::LibraryUpdate;
use library_services::account::LibraryAccount;
use library_services::scheduler::SyncScheduler;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
use std::sync::Arc;

#[derive(Clone, Copy)]
enum WorkerKind {
    Sync,
    Assets,
    Files,
    Enrichment,
}

impl WorkerKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sync => "synchronization",
            Self::Assets => "asset transfers",
            Self::Files => "file replay",
            Self::Enrichment => "enrichment",
        }
    }

    fn pass(self, session: &LibraryResources, remote: bool) -> BoxedBackendFuture<'_, Result<(), ()>> {
        match self {
            Self::Sync => Box::pin(sync_pass(session, remote)),
            Self::Assets => Box::pin(asset_pass(session)),
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
                if matches!(self, Self::Sync | Self::Assets) {
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
            events.assets.request_remote_refresh();
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
        let mut transfer_changes = transfers::POLICY.subscribe();
        let transfer_policy: BoxedBackendFuture<'_, ()> = Box::pin(async move {
            while transfer_changes.changed().await.is_ok() {
                session.events.assets.wake();
                if transfers::allowed() {
                    session.events.notify_contents_changed();
                }
            }
        });
        let sync: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Sync.run(session, &session.events.sync));
        let assets: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Assets.run(session, &session.events.assets));
        let files: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Files.run(session, &session.events.files));
        let enrichment: BoxedBackendFuture<'_, ()> = Box::pin(WorkerKind::Enrichment.run(session, &session.events.enrichment));
        #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
        let scan: BoxedBackendFuture<'_, ()> = Box::pin(run_scan_worker(session));
        #[cfg(any(not(feature = "scanner"), target_arch = "wasm32"))]
        let scan: BoxedBackendFuture<'_, ()> = Box::pin(async {});
        let text_search: BoxedBackendFuture<'_, ()> = Box::pin(super::browse::text_search::run(session, session.events.text_search.subscribe()));
        let thumbnails: BoxedBackendFuture<'_, ()> = Box::pin(run_thumbnails(session));
        futures_util::join!(observers, network, transfer_policy, sync, assets, files, enrichment, scan, text_search, thumbnails);
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
                _events.assets.request_remote_refresh();
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
        if remote {
            sync.synchronize_state_mode(false).await.map(|_| ()).map_err(BackendError::operation)
        } else {
            sync.drain_outbox().await.map(|_| ()).map_err(BackendError::operation)
        }
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
    if result.is_ok() {
        // The file worker owns physical replay; sync only commits protocol pages.
        events.files.wake();
    }
    result.map_err(|error| log::warn!("library synchronization will resume: {error}"))
}

async fn asset_pass(library: &LibraryResources) -> Result<(), ()> {
    if !library.sync.remote_ready().await.map_err(|error| log::warn!("could not check asset registration: {error}"))? {
        return Ok(());
    }
    let result = library.transfers.run_asset_pass().await;
    let quota_blocked = result.as_ref().err().and_then(|error| error.failure()) == Some(Failure::CloudStorageFull);
    if library.upload_quota_blocked.swap(quota_blocked, Ordering::AcqRel) != quota_blocked {
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
    // File preparation and enrichment are separate gates. Prepared files
    // remain withheld from upload until enrichment finalizes the addition.
    db.prepare_uploads().await?;
    events.sync.wake();
    events.assets.wake();
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
    if failed {
        Err(())
    } else {
        Ok(())
    }
}

#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
type FolderWatcher = super::import::watcher::WatchSubscription;
#[cfg(all(feature = "scanner", not(feature = "watcher"), not(target_arch = "wasm32")))]
type FolderWatcher = ();

/// Scans on open, on folder changes, and after failures.
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
async fn run_scan_worker(session: &LibraryResources) {
    #[cfg(feature = "watcher")]
    let assets = session.state.read().await.assets().clone();
    let wake = SyncScheduler::default();
    #[cfg(feature = "watcher")]
    let watcher = {
        let root = assets.native_library_root();
        watch(root, wake.clone()).map_err(|error| log::warn!("could not watch library folder {}: {error}", root.display())).ok()
    };
    #[cfg(not(feature = "watcher"))]
    let watcher: Option<FolderWatcher> = None;
    wake.wake();
    let wake = &wake;
    let watcher = std::sync::Arc::new(tokio::sync::Mutex::new(watcher));
    // The debounce is the quiet period while a file manager is still copying.
    library_services::scheduler::run_timed(
        move |_| {
            let watcher = watcher.clone();
            recover_pass("filesystem scan", async move { scan_and_watch(session, wake, &mut *watcher.lock().await).await })
        },
        wake,
        Duration::from_secs(30),
        Duration::from_millis(500),
    )
    .await;
}

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
async fn scan_and_watch(session: &LibraryResources, wake: &SyncScheduler, watcher: &mut Option<FolderWatcher>) -> Result<(), ()> {
    #[cfg(feature = "watcher")]
    let assets = session.state.read().await.assets().clone();
    #[cfg(feature = "watcher")]
    if watcher.as_ref().is_some_and(|watcher| !watcher.is_active()) {
        *watcher = None;
    }
    match session.scan_folder().await {
        Ok(super::import::filesystem_scan::FolderScan::Complete) => {}
        // File replay owns excluded paths. Release the scan's busy state so
        // replay can finish, then verify those files even without a watch event.
        Ok(super::import::filesystem_scan::FolderScan::Deferred) => wake.wake_after(Duration::from_secs(1)),
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

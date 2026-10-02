//! Public handle for operations scoped to one concrete library.

use crate::library::ReadyCommand;

use crate::executor::BackendExecutor;
use crate::library::ResolvedBook;
use crate::{DirectoryImport, ImportFailure, LibrarySyncState, LibrarySyncStatus};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::rc::Rc;
pub(crate) use std::sync::Arc;
use std::time::Duration;

pub(crate) use crate::asset_store::AssetStore;
pub(crate) use crate::library::commands::LibraryCommand;
pub(crate) use crate::library::discovery::LibraryScanner;
pub(crate) use crate::library::events::{LibraryEvent, LibraryEventReceiver, LibraryEventSender, LibraryEventSubscriptions};
pub(crate) use crate::{ContentHash, DirId, DownloadState, LibraryId};
pub(crate) use library_database::Database;

pub(crate) use super::commands::{PreparedRequest, SessionCommand};
pub(crate) use super::pending::{BackgroundJob, Inbox, OperationKind, PendingOperations, PendingReply};
pub(crate) use crate::BackendError;
pub(crate) use futures_util::FutureExt;
use library_runtime::TransferJob;

/// All operations whose meaning is scoped to one concrete library.
///
/// Application workflows supply policy and coordinate networking; this handle
/// has no reference back to the application.
pub struct LibrarySession {
    pub(crate) id: LibraryId,
    pub(crate) db: std::sync::Arc<Database>,
    pub(crate) assets: AssetStore,
    pub(crate) metadata: metadata_client::MetadataClient,
    pub(crate) sync: Arc<crate::sync::LibrarySync>,
    pub(crate) event_tx: LibraryEventSender,
    pub(crate) event_rx: Option<LibraryEventReceiver>,
    pub(crate) subscriptions: LibraryEventSubscriptions,
    pub(crate) runtime: Arc<crate::library::session::LibraryRuntime>,
    /// All worker lifetimes are owned by this one concrete session.
    pub(crate) workers: super::workers::LibraryWorkers,
    pub(crate) sync_wake: library_runtime::scheduler::SyncScheduler,
    pub(crate) sync_requests: Option<async_channel::Receiver<library_runtime::scheduler::SyncRequest>>,
    pub(crate) asset_wake: library_runtime::scheduler::SyncScheduler,
    pub(crate) asset_requests: Option<async_channel::Receiver<library_runtime::scheduler::SyncRequest>>,
    pub(crate) account: crate::integration::LibraryAccount,
    pub(crate) remote_changes: crate::notification_interest::RemoteChanges,
    /// Kept only for library-owned accounting. The app registry never opens a
    /// library path merely to calculate a settings total.
    // Its Drop implementation stops and joins the scanner threads.
    pub(crate) _scanner: Arc<LibraryScanner>,
    pub(crate) cpu: std::sync::Arc<dyn cpu_host::CpuHost>,
    pub(crate) filesystem_scanning: std::cell::Cell<bool>,
    pub(crate) file_work_error: std::cell::RefCell<Option<String>>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type ScanResult = Result<Option<library_database::ScanApplyResult>, BackendError>;

pub(crate) enum OperationCompletion {
    Done,
    #[cfg(target_arch = "wasm32")]
    Book {
        reply: PendingReply<(client_runtime::reply::Reply, u64)>,
        result: Result<super::book::Opened, BackendError>,
    },
}

#[cfg(target_arch = "wasm32")]
pub(crate) type ScanResult = Result<(), BackendError>;

#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
pub(crate) fn start_filesystem_watcher(root: &str, sender: async_channel::Sender<()>) -> notify::Result<notify::RecommendedWatcher> {
    use notify::Watcher as _;
    let root = PathBuf::from(root);
    let watched_root = root.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                log::warn!("library filesystem watcher error: {error}");
                let _ = sender.try_send(());
                return;
            }
        };
        if event.kind.is_access() {
            return;
        }
        if event.need_rescan() || event.paths.is_empty() {
            let _ = sender.try_send(());
            return;
        }
        for path in event.paths {
            let Ok(relative) = path.strip_prefix(&watched_root) else { continue };
            let hidden = relative.components().any(|component| component.as_os_str().to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            let _ = sender.try_send(());
            return;
        }
    })?;
    watcher.watch(&root, notify::RecursiveMode::Recursive)?;
    Ok(watcher)
}

pub(crate) struct LibraryActor {
    pub(crate) operations: PendingOperations<OperationCompletion>,
    pub(crate) session: Rc<LibrarySession>,
    // Poll preparation beside commands/events, and keep filesystem replay
    // independent so network enrichment cannot hold up physical folder moves.
    pub(crate) enrichment_job: BackgroundJob<Result<(), BackendError>>,
    pub(crate) file_job: BackgroundJob<Result<(), BackendError>>,
    pub(crate) local_retry: BackgroundJob<()>,
    pub(crate) enrichment_retry: BackgroundJob<()>,
    pub(crate) scan_job: BackgroundJob<ScanResult>,
    pub(crate) watch_debounce: BackgroundJob<()>,
    pub(crate) watch_events: async_channel::Receiver<()>,
    pub(crate) _watch_events_sender: async_channel::Sender<()>,
    #[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
    pub(crate) _watcher: Option<notify::RecommendedWatcher>,
    /// Browser book readers and Blob URLs are thread-local resources.
    /// The actor retains them for the lifetime of the page subscription; only
    /// a Send cancellation guard leaves the actor.
    #[cfg(target_arch = "wasm32")]
    pub(crate) books: super::book::browser::BookResources,
}

impl Drop for LibraryActor {
    fn drop(&mut self) {
        self.session.subscriptions.close();
    }
}

impl LibraryActor {
    pub(crate) fn new(session: LibrarySession) -> Self {
        let (watch_events_sender, watch_events) = async_channel::bounded(1);
        #[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
        let watcher = if session.assets.local_access_enabled() {
            match start_filesystem_watcher(session._scanner.root(), watch_events_sender.clone()) {
                Ok(watcher) => Some(watcher),
                Err(error) => {
                    log::warn!("could not watch library folder {}: {error}", session._scanner.root());
                    None
                }
            }
        } else {
            None
        };
        Self {
            session: Rc::new(session),
            operations: Default::default(),
            scan_job: Default::default(),
            watch_debounce: Default::default(),
            watch_events,
            _watch_events_sender: watch_events_sender,
            #[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
            _watcher: watcher,
            enrichment_job: Default::default(),
            file_job: Default::default(),
            local_retry: Default::default(),
            enrichment_retry: Default::default(),
            #[cfg(target_arch = "wasm32")]
            books: Default::default(),
        }
    }

    // Commands never suspend this loop. It owns pending operations and polls
    // their completions beside events, scans, and preparation.
    pub(crate) async fn run(mut self, commands: async_channel::Receiver<LibraryActorCommand>, events: LibraryEventReceiver, event_commands: async_channel::Sender<LibraryActorCommand>) {
        let commands = Inbox(commands);
        let sync_wake = self.session.sync_wake.clone();
        let asset_wake = self.session.asset_wake.clone();
        loop {
            // Retirement closes admission immediately; do not execute a
            // backlog of commands against an already-retired generation.
            if commands.0.is_closed() {
                break;
            }
            tokio::select! {
                event = self.watch_events.recv() => if event.is_ok() {
                    // Reset the quiet period while a file manager is still copying.
                    self.watch_debounce.task = Some(Box::pin(crate::executor::sleep(Duration::from_millis(500))));
                },
                _ = self.watch_debounce.next() => {
                    self.watch_debounce.finish();
                    self.begin_scan();
                },
                Some(completion) = self.operations.next(), if !self.operations.is_empty() => self.complete_operation(completion),
                _ = self.local_retry.next() => { self.local_retry.finish(); self.begin_file_work(); },
                _ = self.enrichment_retry.next() => { self.enrichment_retry.finish(); self.begin_enrichment(); },
                result = self.file_job.next() => self.finish_file_work(result),
                result = self.enrichment_job.next() => self.finish_enrichment(result),
                result = self.scan_job.next() => self.finish_scan(result),
                event = events.recv() => match event {
                    Ok(event) => self.publish_event(event, &event_commands),
                    Err(_) => break,
                },
                command = commands.0.recv() => match command {
                    Ok(command) => {
                        if matches!(&command, LibraryActorCommand::Ui { command: SessionCommand::Ready(ReadyCommand::DownloadState { .. }), .. }) {
                            // A transfer may have committed while its earlier
                            // queued/progress events still sit in this inbox.
                            // Publish those before taking a subscription
                            // baseline, so they cannot replay after it.
                            while let Ok(event) = events.try_recv() {
                                self.publish_event(event, &event_commands);
                            }
                        }
                        if !self.handle_command(command, &sync_wake, &asset_wake) {
                            break;
                        }
                    }
                    Err(_) => break,
                },
            }
        }
    }

    /// Applies one command in receive order. Returns false on shutdown.
    fn handle_command(&mut self, command: LibraryActorCommand, sync_wake: &library_runtime::scheduler::SyncScheduler, asset_wake: &library_runtime::scheduler::SyncScheduler) -> bool {
        match command {
            LibraryActorCommand::WorkerState(request) => request.apply(&self.session.db),
            LibraryActorCommand::Ui { command, reply } => {
                let reply = PendingReply::new(reply);
                match super::commands::prepare_library_request(self.session.clone(), command) {
                    Ok(PreparedRequest::Ready(value)) => reply.finish(Ok(value)),
                    Ok(PreparedRequest::Pending(operation)) => self.submit(operation.kind, reply, operation.future),
                    Err(error) => reply.finish(Err(error)),
                }
            }
            #[cfg(target_arch = "wasm32")]
            LibraryActorCommand::SubscribeBook { content_hash, reply } => {
                let session = self.session.clone();
                self.operations.submit(OperationKind::Read, PendingReply::new(reply), async move { session.prepare_book_for_reader(content_hash).await }, |reply, result| OperationCompletion::Book { reply, result });
            }
            #[cfg(target_arch = "wasm32")]
            LibraryActorCommand::ReleaseBook { id } => {
                self.books.release(id);
            }
            #[cfg(not(target_arch = "wasm32"))]
            LibraryActorCommand::ResolveBook { content_hash, reply } => {
                let session = self.session.clone();
                self.submit(OperationKind::Read, PendingReply::new(reply), async move { session.prepare_book_for_reader(content_hash).await });
            }
            #[cfg(not(target_arch = "wasm32"))]
            LibraryActorCommand::ResolveAudiobookTrack { content_hash, index, reply } => {
                let session = self.session.clone();
                self.submit(OperationKind::Read, PendingReply::new(reply), async move { session.prepare_audiobook_track(content_hash, index).await });
            }
            #[cfg(not(target_arch = "wasm32"))]
            LibraryActorCommand::ImportDirectory { parent_id, directory, create_root, activity, reply } => {
                let queued = std::time::Instant::now();
                log::info!(target: "import_timing", "import_command_received");
                let session = self.session.clone();
                let cancelled = reply.clone();
                self.submit(OperationKind::Import, PendingReply::new(reply), async move {
                    log::info!(target: "import_timing", "import_admitted queue_ms={:.3}", queued.elapsed().as_secs_f64() * 1000.0);
                    session.import_selected_directory(parent_id, directory, create_root, activity, &|| cancelled.is_closed()).await
                });
            }
            LibraryActorCommand::Control(LibraryControl::WakeAssets(hashes)) => {
                if hashes.is_empty() {
                    asset_wake.wake();
                } else {
                    asset_wake.wake_hashes(hashes);
                }
                self.begin_file_work();
            }
            LibraryActorCommand::Control(LibraryControl::StartScanner) => {
                self.begin_scan();
            }
            LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence }) => {
                if let Err(error) = self.session.db.set_asset_storage_policy(enabled, reset_cloud_presence) {
                    log::warn!("could not update cloud-storage policy for {}: {error}", self.session.id);
                } else {
                    self.session.sync.clone().set_asset_storage_enabled(enabled);
                    asset_wake.wake();
                    sync_wake.request_remote_refresh();
                }
            }
            LibraryActorCommand::Control(LibraryControl::Rename { name, refresh_remote }) => {
                if let Err(error) = self.session.db.set_remote_library_name(&name) {
                    log::warn!("could not update the remote name for {}: {error}", self.session.id);
                } else if refresh_remote {
                    sync_wake.request_remote_refresh();
                }
            }
            LibraryActorCommand::Control(LibraryControl::Shutdown) => return false,
        }
        true
    }

    fn complete_operation(&mut self, completion: OperationCompletion) {
        match completion {
            OperationCompletion::Done => {}
            #[cfg(target_arch = "wasm32")]
            OperationCompletion::Book { reply, result } => {
                if reply.is_closed() {
                    return;
                }
                match result {
                    Ok((book, lease)) => {
                        // Both registration and reply occur synchronously on the
                        // browser thread, so cancellation cannot interleave.
                        let id = self.books.retain(lease);
                        reply.finish(Ok((client_runtime::reply::Reply::new(book), id)));
                    }
                    Err(error) => reply.finish(Err(error)),
                }
            }
        }
    }

    fn finish_file_work(&mut self, result: Result<(), BackendError>) {
        let pending = self.file_job.finish();
        *self.session.file_work_error.borrow_mut() = result.as_ref().err().map(ToString::to_string);
        self.session.emit(LibraryEvent::TransferStatusChanged);
        if let Err(error) = result {
            log::warn!("library file replay will resume: {error}");
            self.local_retry.retry_after(30);
        }
        // A lease can leave durable work queued even after a successful pass.
        if self.session.db.has_pending_file_work().unwrap_or(true) || self.session.db.has_pending_purge_work().unwrap_or(true) {
            self.local_retry.retry_after(30);
        }
        // Enrichment follows local replay, but never prevents a later replay.
        self.admit_and_enrich();
        if pending {
            self.begin_file_work();
        }
    }

    fn finish_enrichment(&mut self, result: Result<(), BackendError>) {
        let pending = self.enrichment_job.finish();
        if let Err(error) = &result {
            log::warn!("library enrichment will resume: {error}");
        }
        if result.is_err() || self.session.assets.local_access_enabled() && self.session.db.pending_thumbnail_job_count().map_or(true, |count| count > 0) {
            self.enrichment_retry.retry_after(30);
        } else if let Some(retry_at) = self.session.db.enrichment_next_retry().unwrap_or(None) {
            // Failed lookups retry within minutes; answers refresh after weeks
            // and are picked up by a later pass.
            let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            if retry_at <= now + 3_600 {
                self.enrichment_retry.retry_after((retry_at - now).max(30) as u64);
            }
        }
        if pending {
            self.begin_enrichment();
        }
    }

    pub(crate) fn submit<T: 'static>(&mut self, kind: OperationKind, reply: PendingReply<T>, operation: impl std::future::Future<Output = Result<T, BackendError>> + 'static) {
        self.operations.submit(kind, reply, operation, |reply, result| {
            reply.finish(result);
            OperationCompletion::Done
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn begin_scan(&mut self) {
        if self.scan_job.task.is_some() {
            self.scan_job.pending = true;
            return;
        }
        self.session.filesystem_scanning.set(true);
        let session = self.session.clone();
        self.scan_job.task = Some(Box::pin(async move { AssertUnwindSafe(session.start_scanner()).catch_unwind().await.map_err(|_| BackendError::message("library scan panicked")) }));
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn finish_scan(&mut self, result: ScanResult) {
        self.scan_job.finish();
        self.session.filesystem_scanning.set(false);
        if let Err(error) = result {
            log::warn!("library scan failed: {error}");
        }
        self.session.emit(LibraryEvent::ScanComplete);
    }

    pub(crate) fn begin_file_work(&mut self) {
        if !self.session.assets.local_access_enabled() {
            self.admit_and_enrich();
            return;
        }
        if self.session.scanner_running() {
            self.local_retry.retry_after(30);
            return;
        }
        if !self.file_job.begin() {
            return;
        }
        let session = self.session.clone();
        self.file_job.task = Some(Box::pin(async move {
            AssertUnwindSafe(async {
                // A scan can start after this job was queued but before polling.
                if session.scanner_running() {
                    return Err(BackendError::message("Waiting for library scan"));
                }
                let Some(_files) = session.runtime.begin_local_work(session.event_tx.clone()) else {
                    return Err(BackendError::message("local file preparation is already running"));
                };
                session.run_file_jobs().await
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(BackendError::message("library file replay panicked")))
        }));
    }

    pub(crate) fn admit_and_enrich(&mut self) {
        if let Err(error) = self.session.admit_uploads() {
            log::warn!("upload admission will resume: {error}");
            self.local_retry.retry_after(30);
            return;
        }
        self.begin_enrichment();
    }

    pub(crate) fn begin_enrichment(&mut self) {
        if !self.enrichment_job.begin() {
            return;
        }
        let session = self.session.clone();
        self.enrichment_job.task = Some(Box::pin(async move { AssertUnwindSafe(session.run_enrichment()).catch_unwind().await.unwrap_or_else(|_| Err(BackendError::message("library enrichment panicked"))) }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn begin_scan(&mut self) {
        if self.scan_job.task.is_some() {
            self.scan_job.pending = true;
            return;
        }
        if !self.session.assets.local_access_enabled() || !self.session._scanner.enabled() {
            self.session.filesystem_scanning.set(false);
            return;
        }
        self.session.filesystem_scanning.set(true);
        let snapshot = match self.session.db.scan_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!("could not create filesystem scan snapshot; retrying in 30 seconds: {error}");
                self.retry_scan(Duration::from_secs(30));
                return;
            }
        };
        self.session.emit(LibraryEvent::ScanStarted);
        let scanner = self.session._scanner.clone();
        let session = self.session.clone();
        self.scan_job.task = Some(Box::pin(async move {
            // Walking directories and fingerprinting files is blocking I/O.
            // Keep it off the library actor's thread as well as its command loop.
            let (snapshot, discovery) = crate::executor::run_blocking({
                let scanner = scanner.clone();
                move || {
                    let discovery = scanner.discover(&snapshot);
                    (snapshot, discovery)
                }
            })
            .await
            .map_err(BackendError::operation)?;
            AssertUnwindSafe({
                let mut snapshot: library_database::ScanSnapshot = snapshot;
                let mut discovery: library_database::FilesystemScan = discovery;
                async move {
                    let mut readable = discovery.readable;
                    for files in discovery.files.chunks(filesystem_scanner::filesystem_scan::PUBLISH_BATCH_SIZE) {
                        let chunk = library_database::FilesystemScan { files: files.to_vec(), readable: discovery.readable, ..Default::default() };
                        let inspected = session._scanner.inspect(&chunk, &snapshot, &session.cpu).await;
                        readable &= inspected.readable;
                        if !inspected.books.is_empty() || !inspected.failures.is_empty() {
                            let result = session.db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &inspected, false).map_err(BackendError::operation)?;
                            if result == library_database::ScanApplyResult::Stale {
                                return Ok(Some(result));
                            }
                            discovery.directories.clear();
                            session.emit(LibraryEvent::ContentsChanged);
                            session.sync_wake.wake();
                        }
                        // Deliver queries and partial contents before inspecting another batch.
                        tokio::task::yield_now().await;
                    }
                    let result = session.db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &library_database::InspectedFilesystemScan { failures: vec![], readable, books: vec![] }, true).map_err(BackendError::operation)?;
                    if result == library_database::ScanApplyResult::Applied && !discovery.readable {
                        return Err(BackendError::message("filesystem scan was incomplete"));
                    }
                    Ok(Some(result))
                }
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(BackendError::message("library scan panicked")))
        }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn retry_scan(&mut self, delay: Duration) {
        self.scan_job.task = Some(Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(None)
        }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn finish_scan(&mut self, result: ScanResult) {
        let pending = self.scan_job.finish();
        let result = match result {
            Ok(None) => {
                self.begin_scan();
                return;
            }
            Ok(Some(result)) => Ok(result),
            Err(error) => Err(error),
        };
        match result {
            Ok(library_database::ScanApplyResult::Applied) => {
                self.session.filesystem_scanning.set(false);
                self.session.emit(LibraryEvent::ContentsChanged);
                self.session.emit(LibraryEvent::ScanComplete);
                // A completed scan can leave lifecycle and metadata mutations
                // that need a sync pass before dependent asset work is ready.
                self.session.sync_wake.wake();
                let mut next_scan = pending.then_some(Duration::from_millis(250));
                #[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
                if self._watcher.is_none() && self.session.assets.local_access_enabled() {
                    self._watcher = match start_filesystem_watcher(self.session._scanner.root(), self._watch_events_sender.clone()) {
                        Ok(watcher) => Some(watcher),
                        Err(error) => {
                            log::warn!("could not watch library folder {}: {error}", self.session._scanner.root());
                            None
                        }
                    };
                    // Recheck files that may have arrived before registration.
                    next_scan = Some(if self._watcher.is_some() { Duration::from_millis(250) } else { Duration::from_secs(30) });
                }
                if let Some(delay) = next_scan {
                    self.retry_scan(delay);
                }
            }
            Ok(library_database::ScanApplyResult::Stale) => {
                // Coalesce changes while allowing the actor to serve requests.
                self.retry_scan(Duration::from_millis(250));
            }
            Err(error) => {
                log::warn!("filesystem scan failed; retrying in 30 seconds: {error}");
                #[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
                if !std::path::Path::new(self.session._scanner.root()).is_dir() {
                    self._watcher = None;
                }
                self.retry_scan(Duration::from_secs(30));
            }
        }
    }

    /// Event book is part of the library actor, not a second worker
    /// that borrows session state through shared runtime objects.
    pub(crate) fn publish_event(&self, event: LibraryEvent, commands: &async_channel::Sender<LibraryActorCommand>) {
        let busy = self.session.runtime.has_directory_imports() || self.session.scanner_running();
        self.session.subscriptions.publish(event.clone(), busy);
        if matches!(event, LibraryEvent::ContentsChanged | LibraryEvent::ImportComplete | LibraryEvent::ScanComplete | LibraryEvent::DownloadStatusChanged { state: crate::DownloadState::Downloaded, .. }) {
            let hashes = match &event {
                LibraryEvent::DownloadStatusChanged { content_hash, .. } => vec![*content_hash],
                _ => Vec::new(),
            };
            let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(hashes)));
        }
    }
}

/// The only app-facing control surface for a running library.  It deliberately
/// exposes no database, asset store, or mutable worker state.
#[derive(Clone)]
pub struct LibraryHandle {
    pub(crate) id: LibraryId,
    pub commands: async_channel::Sender<LibraryActorCommand>,
    pub(crate) subscriptions: LibraryEventSubscriptions,
    pub(crate) runtime: Arc<super::session::LibraryRuntime>,
    pub(crate) event_tx: LibraryEventSender,
    pub(crate) sync_wake: library_runtime::scheduler::SyncScheduler,
}

#[derive(Clone, Debug)]
pub enum LibraryControl {
    WakeAssets(Vec<ContentHash>),
    StartScanner,
    SetAssetStorage { enabled: bool, reset_cloud_presence: bool },
    Rename { name: String, refresh_remote: bool },
    Shutdown,
}

pub enum LibraryActorCommand {
    WorkerState(super::worker_state::WorkerStateRequest),
    Control(LibraryControl),
    Ui {
        command: SessionCommand,
        reply: async_channel::Sender<Result<client_runtime::reply::Reply, BackendError>>,
    },
    #[cfg(target_arch = "wasm32")]
    SubscribeBook {
        content_hash: ContentHash,
        reply: async_channel::Sender<Result<(client_runtime::reply::Reply, u64), BackendError>>,
    },
    #[cfg(target_arch = "wasm32")]
    ReleaseBook {
        id: u64,
    },
    /// Reader resources and filesystem imports are host-shaped. Browsers open
    /// books through their own page-side adapter, and the resources they
    /// produce are not `Send`, so these never cross the browser actor channel.
    #[cfg(not(target_arch = "wasm32"))]
    ResolveBook {
        content_hash: ContentHash,
        reply: async_channel::Sender<Result<ResolvedBook, BackendError>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ResolveAudiobookTrack {
        content_hash: ContentHash,
        index: usize,
        reply: async_channel::Sender<Result<ResolvedBook, BackendError>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ImportDirectory {
        parent_id: DirId,
        directory: DirectoryImport,
        create_root: bool,
        activity: Option<uuid::Uuid>,
        reply: async_channel::Sender<Result<Vec<ImportFailure>, BackendError>>,
    },
}

/// A Send-only token for a browser book held by the library actor.
/// Dropping the worker subscription releases its actor-owned resource.
#[cfg(target_arch = "wasm32")]
pub struct BookSubscription {
    pub commands: async_channel::Sender<LibraryActorCommand>,
    pub(crate) id: u64,
}

#[cfg(target_arch = "wasm32")]
impl Drop for BookSubscription {
    fn drop(&mut self) {
        let _ = self.commands.try_send(LibraryActorCommand::ReleaseBook { id: self.id });
    }
}

impl LibraryHandle {
    /// Open one complete library runtime.  The returned handle is the only
    /// object the application may retain; resource setup and worker startup
    /// are entirely session-owned.
    ///
    /// Native opens the database connection, and builds everything that
    /// references it (including the sync worker's own connection), entirely
    /// on the library's dedicated thread — the connection is `Send` but not
    /// `Sync`, so it must never exist anywhere else, even transiently on the
    /// calling thread. `run_blocking` hands `spec` over and waits for the
    /// finished handle, which holds no database reference and is safe to
    /// return.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(spec: super::LibraryRuntimeSpec, _executor: &BackendExecutor) -> Result<Self, BackendError> {
        let library_executor = crate::executor::LibraryExecutor::new().map_err(BackendError::operation)?;
        let executor_for_actor = library_executor.clone();
        let handle = library_executor.run_blocking(move || -> Result<LibraryHandle, BackendError> {
            let session = LibrarySession::open(spec)?;
            session.sync_wake.request_remote_refresh();
            Ok(session.spawn_actor(executor_for_actor))
        })?;
        let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(Vec::new())));
        // Reconcile files on every native open, including startup and newly
        // registered folders. Queue after spawning the actor so async book
        // inspection runs on the library's executor.
        let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::StartScanner));
        Ok(handle)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn open(spec: super::LibraryRuntimeSpec, executor: &BackendExecutor) -> Result<Self, BackendError> {
        let session = LibrarySession::open(spec)?;
        let needs_scan = session.db.needs_scan()?;
        session.sync_wake.request_remote_refresh();
        let handle = session.spawn_actor(executor);
        let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(Vec::new())));
        if needs_scan {
            let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::StartScanner));
        }
        Ok(handle)
    }

    pub fn id(&self) -> LibraryId {
        self.id
    }

    pub fn shutdown(&self) {
        self.subscriptions.close();
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::Shutdown));
        self.commands.close();
    }

    /// Reads and updates shared import progress directly; resource-bearing
    /// commands are delivered to the owning actor.
    pub async fn dispatch(&self, command: LibraryCommand) -> Result<client_runtime::reply::Reply, BackendError> {
        if self.commands.is_closed() {
            return Err("library actor is no longer running".into());
        }
        let command = match command {
            LibraryCommand::Runtime(command) => return Ok(super::commands::dispatch_runtime_request(&self.runtime, &self.event_tx, &self.sync_wake, command)),
            LibraryCommand::Session(command) => command,
        };
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::Ui { command, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())?
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn subscribe_book(&self, content_hash: ContentHash) -> Result<(client_runtime::reply::Reply, BookSubscription), BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::SubscribeBook { content_hash, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        let (initial, id) = result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())??;
        Ok((initial, BookSubscription { commands: self.commands.clone(), id }))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_book(&self, content_hash: ContentHash) -> Result<ResolvedBook, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ResolveBook { content_hash, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_audiobook_track(&self, content_hash: ContentHash, index: usize) -> Result<ResolvedBook, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ResolveAudiobookTrack { content_hash, index, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_selected_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool, activity: Option<uuid::Uuid>) -> Result<Vec<ImportFailure>, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ImportDirectory { parent_id, directory, create_root, activity, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())?
    }

    pub async fn download_changes(&self, content_hash: ContentHash) -> Result<(async_channel::Receiver<DownloadState>, DownloadState), BackendError> {
        let durable = self.dispatch(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::DownloadState { content_hash }))).await?.take::<DownloadState>()?;
        Ok({
            let subscriptions = &self.subscriptions;

            let (sender, receiver) = async_channel::unbounded();
            let mut subscribers = subscriptions.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let initial = subscribers.latest_downloads.get(&content_hash).cloned().unwrap_or(durable);
            if !subscribers.closed {
                subscribers.downloads.retain(|(_, sender)| !sender.is_closed());
                subscribers.downloads.push((content_hash, sender));
            }
            (receiver, initial)
        })
    }
}

impl LibrarySession {
    /// Combines library-owned durable sync details with the application-owned
    /// account/discovery snapshot for settings presentation.
    pub(crate) fn sync_status(&self, signed_in: bool, remote_present: Option<bool>) -> Result<LibrarySyncStatus, BackendError> {
        let sync_details = self.db.stored_sync_status()?;
        let state = match signed_in {
            false => LibrarySyncState::LocalOnly,
            true => {
                if remote_present == Some(false) {
                    LibrarySyncState::RemoteMissing
                } else if !sync_details.has_unsynced_changes && sync_details.last_synced_at.is_none() {
                    if remote_present.is_some() {
                        LibrarySyncState::Receiving
                    } else {
                        LibrarySyncState::Checking
                    }
                } else if sync_details.has_unsynced_changes {
                    LibrarySyncState::SyncPending
                } else {
                    LibrarySyncState::Synced
                }
            }
        };
        Ok(LibrarySyncStatus { asset_storage_enabled: self.sync.clone().asset_storage_enabled(), library_id: self.id, state, has_unsynced_changes: sync_details.has_unsynced_changes, last_synced_at: sync_details.last_synced_at })
    }

    /// Announces that the set of books or folders changed.
    ///
    /// A write site cannot tell whether it is alone or the four-thousandth
    /// commit of an import, so it must not decide how often this is worth
    /// saying. An import in flight already brackets itself and announces its
    /// result once at [`Self::finish_directory_import`]; every commit inside
    /// that bracket is the same announcement repeated, and each one costs every
    /// open view a full reload.
    pub(crate) fn notify_contents_changed(&self) {
        if !self.runtime.has_directory_imports() {
            let _ = self.event_tx.try_send(LibraryEvent::ContentsChanged);
            self.sync_wake.wake();
        }
    }

    pub(crate) fn notify_storage_changed(&self) {
        // Folder imports publish their totals once through ImportComplete.
        if !self.runtime.has_directory_imports() {
            let _ = self.event_tx.try_send(LibraryEvent::StorageChanged);
        }
    }

    pub(crate) fn begin_directory_import(&self, id: uuid::Uuid) {
        if self.runtime.begin_directory_import(id) {
            let _ = self.event_tx.try_send(LibraryEvent::ImportStarted);
        }
    }

    pub(crate) fn advance_directory_import(&self, id: uuid::Uuid, total: u64, succeeded: u64, failed: u64) {
        if self.runtime.advance_directory_import(id, total, succeeded, failed) {
            let _ = self.event_tx.try_send(LibraryEvent::TransferStatusChanged);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_import_file(&self, id: uuid::Uuid, name: String, total_bytes: Option<u64>) -> crate::asset_store::ImportProgressObserver {
        let progress = self.runtime.start_import_file(id, name, total_bytes);
        std::sync::Arc::new(move |stage| {
            progress.lock().unwrap_or_else(|e| e.into_inner()).stage = stage;
        })
    }

    pub(crate) fn finish_directory_import(&self, id: uuid::Uuid) {
        // Duplicate and late replies cannot complete another import.
        if self.runtime.finish_directory_import(id) {
            let _ = self.event_tx.try_send(LibraryEvent::ImportComplete);
            self.sync_wake.wake();
        }
    }

    pub(crate) async fn local_storage_usage(&self) -> Result<Option<u64>, BackendError> {
        if !self.assets.local_access_enabled() {
            return Ok(None);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let paths = self.db.local_book_paths()?;
            self.assets.book_bytes_at_paths(paths).await.map(Some).map_err(BackendError::operation)
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.assets.local_book_bytes().await.map(Some).map_err(BackendError::operation)
        }
    }

    /// Transfers sole ownership of this session to its long-lived actor.
    /// Callers retain only the returned handle, never a resource-bearing
    /// session clone.
    ///
    /// Native gives every opened library its own dedicated OS thread (see
    /// [`crate::executor::LibraryExecutor`]): the session's database
    /// connection is `Send` but not `Sync`, so the actor loop and its
    /// workers run there instead of on the shared multi-threaded executor,
    /// where holding it across an `.await` would not type-check. Wasm has
    /// only one thread to begin with, so it keeps using the shared executor
    /// directly — the same one the browser's own worker boundary already
    /// requires this handle's command protocol for.
    /// Called only from inside a job already running on `library_executor`'s
    /// dedicated thread (see [`Self::open`]) — everything here executes
    /// there, so nothing it touches needs to be `Send`.
    pub(crate) fn spawn_actor(mut self, #[cfg(not(target_arch = "wasm32"))] library_executor: crate::executor::LibraryExecutor, #[cfg(target_arch = "wasm32")] executor: &BackendExecutor) -> LibraryHandle {
        let (commands, receiver) = async_channel::unbounded();
        let handle = LibraryHandle { id: self.id, commands, subscriptions: self.subscriptions.clone(), runtime: self.runtime.clone(), event_tx: self.event_tx.clone(), sync_wake: self.sync_wake.clone() };
        let account_observer = super::workers::observe_account_changes(self.account.clone(), handle.commands.clone(), self.sync_wake.clone(), self.event_tx.clone());
        let remote = super::workers::observe_remote_changes(self.remote_changes.clone(), self.sync_wake.clone());
        let sync = super::workers::run_sync_scheduler(
            self.sync.clone(),
            self.sync_requests.take().expect("a library session has one sync receiver"),
            #[cfg(not(target_arch = "wasm32"))]
            self.assets.clone(),
            super::worker_state::WorkerState::new(handle.commands.clone()),
        );
        let assets = super::workers::run_asset_scheduler(self.sync.clone(), self.asset_requests.take().expect("a library session has one asset receiver"), super::worker_state::WorkerState::new(handle.commands.clone()));
        let events = self.event_rx.take().expect("a library session has one event receiver");
        let event_commands = handle.commands.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::executor::LibraryExecutor;
            self.workers.account_observer = Some(LibraryExecutor::spawn_local(account_observer));
            self.workers.remote = Some(LibraryExecutor::spawn_local(remote));
            self.workers.sync = Some(LibraryExecutor::spawn_local(sync));
            self.workers.assets = Some(LibraryExecutor::spawn_local(assets));
            LibraryExecutor::spawn_local_detached(async move {
                // Keep the dedicated executor alive until the actor retires.
                let _executor = library_executor;
                LibraryActor::new(self).run(receiver, events, event_commands).await;
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.workers.account_observer = Some(executor.spawn(account_observer));
            self.workers.remote = Some(executor.spawn(remote));
            self.workers.sync = Some(executor.spawn(sync));
            self.workers.assets = Some(executor.spawn(assets));
            executor.spawn_detached(async move {
                LibraryActor::new(self).run(receiver, events, event_commands).await;
            });
        }
        handle
    }

    pub(crate) fn emit(&self, event: LibraryEvent) {
        let _ = self.event_tx.try_send(event);
    }

    pub fn id(&self) -> &LibraryId {
        &self.id
    }

    /// True while the local filesystem scanner is scheduled or actively
    /// walking this library.
    pub(crate) fn discovery_enabled(&self) -> bool {
        self._scanner.enabled()
    }

    pub fn scanner_running(&self) -> bool {
        self.filesystem_scanning.get() || self.runtime.has_directory_imports()
    }

    #[cfg(all(test, feature = "scanner", not(target_os = "android")))]
    pub(crate) fn shares_scanner_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self._scanner, &other._scanner)
    }

    /// Enables the initial filesystem scan for a handle opened in deferred mode.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn start_scanner(&self) {
        if self.assets.local_access_enabled() {
            ({
                async move {
                    let snapshot = match self.db.scan_snapshot() {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            log::warn!("could not create filesystem scan snapshot: {error}");
                            return;
                        }
                    };
                    self.emit(LibraryEvent::ScanStarted);
                    let discovery = self._scanner.discover(&snapshot);
                    let inspected = self._scanner.inspect(&discovery, &snapshot, &self.cpu).await;
                    match self.db.apply_filesystem_scan(&snapshot, &discovery, &inspected) {
                        Ok(library_database::ScanApplyResult::Applied) => {
                            self.emit(LibraryEvent::ContentsChanged);
                            self.sync_wake.wake();
                        }
                        Ok(library_database::ScanApplyResult::Stale) => log::debug!("filesystem scan became stale before apply"),
                        Err(error) => log::warn!("could not apply filesystem scan: {error}"),
                    }
                    self.emit(LibraryEvent::ScanComplete);
                }
            })
            .await;
        }
    }

    pub(crate) fn download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let durable = self.persistent_download_state(content_hash)?;
        Ok(match (&durable, self.subscriptions.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).latest_downloads.get(&content_hash).cloned()) {
            (DownloadState::Queued, Some(state @ DownloadState::Downloading(_))) => state,
            _ => durable,
        })
    }

    pub(crate) fn queue_download(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let state = self.download_state(content_hash)?;
        if matches!(state, DownloadState::Downloaded | DownloadState::Downloading(_)) {
            return Ok(state);
        }
        let sync = self.sync.clone();
        if !sync.is_signed_in() {
            return Err(BackendError::message("sign in before downloading this book"));
        }
        {
            let mut subscribers = self.subscriptions.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if matches!(subscribers.latest_downloads.get(&content_hash), Some(DownloadState::Downloaded)) {
                subscribers.latest_downloads.remove(&content_hash);
            }
        }
        self.request_downloads(&[content_hash])?;
        Ok(DownloadState::Queued)
    }

    pub(crate) fn request_downloads(&self, hashes: &[ContentHash]) -> Result<(), BackendError> {
        if hashes.is_empty() {
            return Ok(());
        }
        self.sync.clone().request_downloads(hashes).map_err(BackendError::operation)?;
        // Enqueue and wake together: the next pass plans exactly these
        // hashes first instead of discovering them in the drain.
        self.asset_wake.wake_hashes(hashes.to_vec());
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "concurrency_tests.rs"]
mod concurrency_tests;

#[cfg(all(test, feature = "scanner", not(target_arch = "wasm32")))]
mod background_scan_tests {
    use super::*;

    #[tokio::test]
    async fn subject_compaction_reaches_client_responses_and_reacts_to_finished_filter() {
        use library_database::LibraryBrowseData;
        use library_model::{LibraryBrowseQuery, LibraryFileTypeFilter};
        let root = tempfile::tempdir().unwrap();
        #[derive(Clone, Debug)]
        struct SignedOut;
        impl account_client::SessionPersistence for SignedOut {
            fn load_account_session(&self) -> std::io::Result<Option<account_client::Session>> {
                Ok(None)
            }
            fn save_account_session(&self, _: &account_client::Session) -> std::io::Result<()> {
                unreachable!("signed-out fixture")
            }
            fn clear_account_session(&self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let server: account_client::ServerUrl = "http://127.0.0.1:1".parse().unwrap();
        let account = library_runtime::account::LibraryAccountSession::load(SignedOut, server.clone()).unwrap();
        let session = LibrarySession::open(crate::library::LibraryRuntimeSpec {
            config: crate::library::LibraryOpenConfig {
                id: LibraryId::new_v4(),
                display_name: "Fixture".into(),
                root: root.path().to_string_lossy().into_owned(),
                database_locator: root.path().join("library.sqlite").to_string_lossy().into_owned(),
                default_sync_server_url: server,
                metadata_server_url: "http://127.0.0.1:1".into(),
                asset_storage_enabled: true,
                reset_cloud_presence: false,
            },
            account: account.library_account(),
            notification_interest: Default::default(),
            remote_changes: Default::default(),
        })
        .unwrap();
        let session = Rc::new(session);
        let library = &session;
        async fn wire_value<T: serde::de::DeserializeOwned>(library: &Rc<LibrarySession>, command: LibraryCommand) -> T {
            let reply = crate::library::commands::dispatch_library_request_in_session(library.clone(), command).await.unwrap();
            client_runtime::wire::decode_worker_payload(reply.into_wire().unwrap()).unwrap()
        }
        library_database::execute_owner_fixture_sql(
            &library.db,
            "
         DETACH curated; ATTACH ':memory:' AS curated;
         CREATE TABLE curated.taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
         CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
         INSERT INTO curated.concept VALUES(920001,'History'),(920002,'U.S.'),(920003,'Period');
         CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
         INSERT INTO curated.unified_concept_route VALUES(1,920001,0),(2,920002,1),(3,920003,2);
         INSERT INTO book(content_hash,title,format,read_progress) VALUES
            (printf('%064x',911),'General history',1,100),
            (printf('%064x',912),'Specific history',1,0);
         INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
            SELECT '00000000-0000-0000-0000-000000000000',row_id,title,title,1 FROM book;
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version)
            SELECT row_id,CASE title WHEN 'General history' THEN 920002 ELSE 920003 END,1 FROM book;
         ",
        )
        .unwrap();
        let mut query = LibraryBrowseQuery {
            location: library_model::BrowseLocation::Subject(library_model::ROOT_SUBJECT_ROUTE_ID),
            search: String::new(),
            file_types: vec![LibraryFileTypeFilter::All],
            chip_sort: Default::default(),
            book_sort: Default::default(),
            languages: Vec::new(),
            hide_finished: false,
            include_direct_child_books: false,
        };
        let unfiltered: LibraryBrowseData = wire_value(library, LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Browse { query: query.clone() }))).await;
        assert!(unfiltered.contents.children.is_empty(), "small subject subtrees are compacted before filtering");
        assert_eq!(unfiltered.contents.books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["General history", "Specific history"]);
        query.hide_finished = true;
        let filtered: LibraryBrowseData = wire_value(library, LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Browse { query: query.clone() }))).await;
        assert!(filtered.contents.children.is_empty());
        assert_eq!(filtered.contents.books[0].title, "Specific history", "the finished filter hides only the completed book");
        assert_eq!(filtered.format_counts, unfiltered.format_counts, "facets still describe all books in the location");
        assert_eq!(library.browse_contents(&query, true).unwrap(), filtered.contents);
        query.location = library_model::BrowseLocation::Subject(2);
        let collapsed: LibraryBrowseData = wire_value(library, LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Browse { query: query.clone() }))).await;
        assert!(collapsed.contents.children.is_empty() && collapsed.contents.books.is_empty(), "the original narrow route is no longer browsable");
        query.location = library_model::BrowseLocation::Subject(library_model::ROOT_SUBJECT_ROUTE_ID);
        let grouped: LibraryBrowseData = wire_value(library, LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Browse { query: query.clone() }))).await;
        assert!(grouped.contents.children.is_empty());
        assert_eq!(grouped.contents.books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["Specific history"], "small subtrees also compact at the root");
        query.location = library_model::BrowseLocation::Folder(sync_common::ROOT_DIR_ID);
        let folder: LibraryBrowseData = wire_value(library, LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Browse { query }))).await;
        assert_eq!(folder.contents.books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["Specific history"]);
        assert_eq!(folder.format_counts, unfiltered.format_counts, "folder facets also retain finished books");
    }
}

#[cfg(test)]
mod activity_subscription_tests {
    use super::*;

    #[tokio::test]
    async fn subscription_reports_thumbnail_cancellation_and_overlapping_work() {
        let runtime = Arc::new(crate::library::session::LibraryRuntime::default());
        let (events, changes) = async_channel::unbounded();
        let first = runtime.begin_thumbnail_generation(events.clone());
        assert!(matches!(changes.try_recv(), Ok(LibraryEvent::TransferStatusChanged)));
        let mut work = Box::pin(async {
            let _activity = runtime.begin_thumbnail_generation(events);
            std::future::pending::<()>().await;
        });
        assert!(futures_util::poll!(work.as_mut()).is_pending());
        assert!(matches!(changes.try_recv(), Ok(LibraryEvent::TransferStatusChanged)));
        drop(work);
        assert!(matches!(changes.try_recv(), Ok(LibraryEvent::TransferStatusChanged)));
        assert_eq!(runtime.thumbnail_generations_running(), 1);
        drop(first);
        assert!(matches!(changes.try_recv(), Ok(LibraryEvent::TransferStatusChanged)));
        assert_eq!(runtime.thumbnail_generations_running(), 0);
        assert!(changes.try_recv().is_err());
    }
}

#[cfg(all(test, feature = "watcher", not(target_arch = "wasm32")))]
mod watcher_recovery_tests {
    use super::*;

    #[test]
    fn watcher_can_register_after_library_folder_appears() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("library");
        let (sender, _receiver) = async_channel::bounded(1);
        assert!(start_filesystem_watcher(root.to_str().unwrap(), sender.clone()).is_err());
        std::fs::create_dir(&root).unwrap();
        assert!(start_filesystem_watcher(root.to_str().unwrap(), sender).is_ok());
    }
}

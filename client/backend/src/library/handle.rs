//! Public handle for operations scoped to one concrete library.

use std::sync::Arc;

use crate::library::discovery::LibraryScanner;
use crate::library::events::{LibraryEvent, LibraryEventReceiver, LibraryEventSender, LibraryEventSubscriptions};
use crate::storage::files::AssetStore;
use crate::{ContentHash, DirId, DownloadState, LibraryId};
use library_runtime::database::Database;

use crate::BackendError;

/// All operations whose meaning is scoped to one concrete library.
///
/// Application workflows supply policy and coordinate networking; this handle
/// has no reference back to the application.
pub(crate) struct LibrarySession {
    pub(super) id: LibraryId,
    pub(super) db: Database,
    pub(super) assets: AssetStore,
    metadata: metadata_client::MetadataClient,
    sync: Option<Arc<crate::sync::LibrarySync>>,
    event_tx: LibraryEventSender,
    event_rx: Option<LibraryEventReceiver>,
    subscriptions: LibraryEventSubscriptions,
    runtime: Arc<library_runtime::session::LibraryRuntime>,
    /// All worker lifetimes are owned by this one concrete session.
    workers: library_runtime::workers::LibraryWorkers,
    pub(super) sync_wake: library_runtime::scheduler::SyncScheduler,
    pub(super) sync_requests: Option<async_channel::Receiver<library_runtime::scheduler::SyncRequest>>,
    pub(super) asset_wake: library_runtime::scheduler::SyncScheduler,
    pub(super) asset_requests: Option<async_channel::Receiver<library_runtime::scheduler::SyncRequest>>,
    remote_changes: Option<crate::notification_interest::RemoteChanges>,
    /// Kept only for library-owned accounting. The app registry never opens a
    /// library path merely to calculate a settings total.
    storage_locators: Option<(String, String)>,
    // Its Drop implementation stops and joins the scanner threads.
    pub(super) _scanner: Arc<LibraryScanner>,
}

struct LibraryActor {
    session: LibrarySession,
}

impl LibraryActor {
    /// Event publication is part of the library actor, not a second worker
    /// that borrows session state through shared runtime objects.
    fn publish_event(&self, event: LibraryEvent, commands: &async_channel::Sender<LibraryActorCommand>) {
        let busy = self.session.runtime.has_directory_imports() || self.session._scanner.progress().is_some();
        self.session.subscriptions.publish(event.clone(), busy);
        if matches!(event, LibraryEvent::ContentsChanged | LibraryEvent::ImportComplete | LibraryEvent::ScanComplete | LibraryEvent::DownloadStatusChanged { state: crate::DownloadState::Downloaded, .. }) {
            let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets));
        }
        if matches!(event, LibraryEvent::ContentsChanged) {
            let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: false }));
        }
    }

    /// Returns false only when the session has been asked to shut down.
    async fn handle_command(&self, command: LibraryActorCommand, sync_wake: &library_runtime::scheduler::SyncScheduler, asset_wake: &library_runtime::scheduler::SyncScheduler) -> bool {
        match command {
            LibraryActorCommand::Ui { command, reply } => {
                let _ = reply.send(super::commands::dispatch_library_request_in_session(&self.session, command).await).await;
            }
            LibraryActorCommand::Scanning { reply } => {
                let _ = reply.send(self.session.scanner_running()).await;
            }
            LibraryActorCommand::DownloadState { content_hash, reply } => {
                let _ = reply.send(self.session.download_state(content_hash)).await;
            }
            LibraryActorCommand::TransferSnapshot { reply } => {
                let _ = reply.send(self.session.transfer_snapshot()).await;
            }
            LibraryActorCommand::SyncStatus { library_name, account, fallback_server_url, remote_present, reply } => {
                let _ = reply.send(self.session.sync_status(library_name, account.as_ref(), &fallback_server_url, remote_present)).await;
            }
            LibraryActorCommand::LocalStorageUsage { reply } => {
                let _ = reply.send(self.session.local_storage_usage().await).await;
            }
            LibraryActorCommand::ResolveBook { content_hash, reply } => {
                let _ = reply.send(self.session.prepare_book_for_reader(content_hash).await).await;
            }
            LibraryActorCommand::ImportDirectory { parent_id, directory, create_root, reply } => {
                let _ = reply.send(self.session.import_selected_directory(parent_id, directory, create_root).await).await;
            }
            LibraryActorCommand::ImportBookPath { parent_id, file_name, path, reply } => {
                let _ = reply.send(self.session.import_book_path(&parent_id, file_name, &path).await).await;
            }
            LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: true }) => sync_wake.request_remote_refresh(),
            LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: false }) => sync_wake.wake(),
            LibraryActorCommand::Control(LibraryControl::WakeAssets) => {
                asset_wake.wake();
                if let Err(error) = self.session.run_upload_preparation().await {
                    log::warn!("library preparation will resume: {error}");
                }
            }
            LibraryActorCommand::Control(LibraryControl::StartScanner) => self.session.start_scanner(),
            LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence }) => {
                let result = (|| -> Result<(), BackendError> {
                    if reset_cloud_presence {
                        client_library::assets::reset_cloud_presence(&self.session.db.get()?, enabled)?;
                    }
                    client_library::assets::set_cloud_storage_policy(&self.session.db.get()?, enabled)?;
                    self.session.sync().ok_or_else(|| BackendError::message("library sync is unavailable"))?.set_asset_storage_enabled(enabled);
                    Ok(())
                })();
                if let Err(error) = result {
                    log::warn!("could not update cloud-storage policy for {}: {error}", self.session.id);
                } else {
                    asset_wake.wake();
                    sync_wake.request_remote_refresh();
                }
            }
            LibraryActorCommand::Control(LibraryControl::RepairInventory) => {
                if let Some(sync) = self.session.sync() {
                    if let Err(error) = sync.repair_inventory().await {
                        log::warn!("library inventory repair will resume: {error}");
                    }
                }
            }
            LibraryActorCommand::Control(LibraryControl::SetDisplayName { name }) => {
                let result = (|| -> Result<(), BackendError> {
                    self.session.db.set_remote_library_name(&name)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    log::warn!("could not update the remote name for {}: {error}", self.session.id);
                }
            }
            LibraryActorCommand::Control(LibraryControl::Rename { name }) => {
                let result = (|| -> Result<(), BackendError> {
                    self.session.db.set_remote_library_name(&name)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    log::warn!("could not update the remote name for {}: {error}", self.session.id);
                } else {
                    sync_wake.request_remote_refresh();
                }
            }
            LibraryActorCommand::Control(LibraryControl::Shutdown) => return false,
        }
        true
    }
}

/// The only app-facing control surface for a running library.  It deliberately
/// exposes no database, asset store, or mutable worker state.
#[derive(Clone)]
pub(crate) struct LibraryHandle {
    id: LibraryId,
    commands: async_channel::Sender<LibraryActorCommand>,
    subscriptions: LibraryEventSubscriptions,
}

#[derive(Clone, Debug)]
enum LibraryControl {
    WakeSync { refresh_remote: bool },
    WakeAssets,
    StartScanner,
    SetAssetStorage { enabled: bool, reset_cloud_presence: bool },
    RepairInventory,
    SetDisplayName { name: String },
    Rename { name: String },
    Shutdown,
}

enum LibraryActorCommand {
    Control(LibraryControl),
    Ui { command: crate::runtime::LibraryCommand, reply: async_channel::Sender<Result<crate::runtime::WorkerPayload, String>> },
    Scanning { reply: async_channel::Sender<bool> },
    DownloadState { content_hash: ContentHash, reply: async_channel::Sender<Result<DownloadState, BackendError>> },
    TransferSnapshot { reply: async_channel::Sender<Result<crate::LibraryTransfers, BackendError>> },
    SyncStatus { library_name: String, account: Option<account_client::Session>, fallback_server_url: String, remote_present: Option<bool>, reply: async_channel::Sender<Result<crate::LibrarySyncStatus, BackendError>> },
    LocalStorageUsage { reply: async_channel::Sender<Result<Option<u64>, BackendError>> },
    ResolveBook { content_hash: ContentHash, reply: async_channel::Sender<Result<crate::library::ResolvedBook, BackendError>> },
    ImportDirectory { parent_id: DirId, directory: crate::DirectoryImport, create_root: bool, reply: async_channel::Sender<Result<Vec<crate::ImportFailure>, String>> },
    ImportBookPath { parent_id: DirId, file_name: String, path: std::path::PathBuf, reply: async_channel::Sender<Result<ContentHash, BackendError>> },
}

impl LibraryHandle {
    /// Open one complete library runtime.  The returned handle is the only
    /// object the application may retain; resource setup and worker startup
    /// are entirely session-owned.
    pub(crate) fn open(spec: super::LibraryRuntimeSpec, executor: &crate::executor::BackendExecutor) -> Result<Self, BackendError> {
        let session = LibrarySession::open(spec)?;
        let handle = session.spawn_actor(executor);
        handle.wake_sync(true);
        handle.wake_assets();
        Ok(handle)
    }

    pub(crate) fn id(&self) -> LibraryId {
        self.id
    }

    pub(crate) fn wake_sync(&self, refresh_remote: bool) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote }));
    }

    pub(crate) fn wake_assets(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets));
    }

    pub(crate) fn start_scanner(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::StartScanner));
    }

    /// Applies a registry-selected cloud policy inside the owning session.
    pub(crate) fn set_asset_storage(&self, enabled: bool, reset_cloud_presence: bool) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence }));
    }

    pub(crate) fn repair_inventory(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::RepairInventory));
    }

    /// Applies a name discovered remotely without creating a new rename
    /// mutation to send back to that same remote.
    pub(crate) fn set_display_name(&self, name: String) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetDisplayName { name }));
    }

    pub(crate) fn rename(&self, name: String) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::Rename { name }));
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::Shutdown));
    }

    /// Delivers the concrete UI protocol command to its owning actor.
    pub(crate) async fn dispatch(&self, command: crate::runtime::LibraryCommand) -> Result<crate::runtime::WorkerPayload, String> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::Ui { command, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())?
    }

    pub(crate) async fn scanning(&self) -> Result<bool, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::Scanning { reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))
    }

    pub(crate) async fn download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::DownloadState { content_hash, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub(crate) async fn transfer_snapshot(&self) -> Result<crate::LibraryTransfers, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::TransferSnapshot { reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    /// Reads sync state through the session actor. The application supplies
    /// account discovery context, but never opens the library database itself.
    pub(crate) async fn sync_status(&self, library_name: String, account: Option<account_client::Session>, fallback_server_url: String, remote_present: Option<bool>) -> Result<crate::LibrarySyncStatus, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::SyncStatus { library_name, account, fallback_server_url, remote_present, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub(crate) async fn local_storage_usage(&self) -> Result<Option<u64>, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::LocalStorageUsage { reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub(crate) async fn resolve_book(&self, content_hash: ContentHash) -> Result<crate::library::ResolvedBook, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ResolveBook { content_hash, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub(crate) async fn import_selected_directory(&self, parent_id: DirId, directory: crate::DirectoryImport, create_root: bool) -> Result<Vec<crate::ImportFailure>, String> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ImportDirectory { parent_id, directory, create_root, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())?
    }

    pub(crate) async fn import_book_path(&self, parent_id: DirId, file_name: String, path: std::path::PathBuf) -> Result<ContentHash, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ImportBookPath { parent_id, file_name, path, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub(crate) fn subscribe_updates(&self) -> async_channel::Receiver<library_model::LibraryUpdate> {
        self.subscriptions.subscribe_updates()
    }

    pub(crate) fn subscribe_download(&self, content_hash: ContentHash) -> async_channel::Receiver<DownloadState> {
        self.subscriptions.subscribe_download(content_hash)
    }
}

impl LibrarySession {
    /// Combines library-owned durable sync details with the application-owned
    /// account/discovery snapshot for settings presentation.
    fn sync_status(&self, library_name: String, account: Option<&account_client::Session>, fallback_server_url: &str, remote_present: Option<bool>) -> Result<crate::LibrarySyncStatus, BackendError> {
        let (bound_user_id, sync_details) = self.db.stored_sync_status()?;
        let (state, server_url, account_id) = match account {
            None if bound_user_id.is_none() => (crate::LibrarySyncState::LocalOnly, None, None),
            None => (crate::LibrarySyncState::SignInRequired, Some(fallback_server_url.to_owned()), bound_user_id),
            Some(account) if bound_user_id.as_deref().is_some_and(|bound| bound != account.user_id()) => (crate::LibrarySyncState::DifferentAccount, Some(account.server_url().as_str().to_owned()), bound_user_id),
            Some(account) => {
                let state = if remote_present == Some(false) {
                    crate::LibrarySyncState::RemoteMissing
                } else if !sync_details.has_unsynced_changes && sync_details.last_synced_at.is_none() {
                    if remote_present.is_some() { crate::LibrarySyncState::Receiving } else { crate::LibrarySyncState::Checking }
                } else if sync_details.has_unsynced_changes {
                    crate::LibrarySyncState::SyncPending
                } else {
                    crate::LibrarySyncState::Synced
                };
                (state, Some(account.server_url().as_str().to_owned()), Some(account.user_id().to_owned()))
            }
        };
        Ok(crate::LibrarySyncStatus {
            asset_storage_enabled: self.sync().is_some_and(|sync| sync.asset_storage_enabled()),
            library_id: self.id,
            library_name,
            state,
            server_url,
            account_id,
            has_unsynced_changes: sync_details.has_unsynced_changes,
            last_synced_at: sync_details.last_synced_at,
        })
    }

    /// Announces that the set of books or folders changed.
    ///
    /// A write site cannot tell whether it is alone or the four-thousandth
    /// commit of an import, so it must not decide how often this is worth
    /// saying. An import in flight already brackets itself and announces its
    /// result once at [`Self::finish_directory_import`]; every commit inside
    /// that bracket is the same announcement repeated, and each one costs every
    /// open view a full reload.
    pub(super) fn notify_contents_changed(&self) {
        if !self.runtime.has_directory_imports() {
            let _ = self.event_tx.try_send(LibraryEvent::ContentsChanged);
        }
    }

    pub(super) fn notify_storage_changed(&self) {
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

    pub(crate) fn scan_progress(&self) -> Option<crate::ScanProgress> {
        if let Some(progress) = self.directory_import_progress() {
            return Some(progress);
        }
        self._scanner.progress()
    }

    pub(crate) fn directory_import_progress(&self) -> Option<crate::ScanProgress> {
        self.runtime.directory_import_progress()
    }

    pub(crate) fn finish_directory_import(&self, id: uuid::Uuid) {
        // Duplicate and late replies cannot complete another import.
        if self.runtime.finish_directory_import(id) {
            let _ = self.event_tx.try_send(LibraryEvent::ImportComplete);
        }
    }

    pub(crate) fn from_parts(id: LibraryId, db: Database, assets: AssetStore, metadata: metadata_client::MetadataClient, event_tx: LibraryEventSender, event_rx: LibraryEventReceiver, scanner: LibraryScanner) -> Self {
        let (sync_wake, sync_requests) = library_runtime::scheduler::SyncScheduler::new();
        let (asset_wake, asset_requests) = library_runtime::scheduler::SyncScheduler::new();
        Self {
            id,
            db,
            assets,
            metadata,
            sync: None,
            event_tx,
            event_rx: Some(event_rx),
            subscriptions: LibraryEventSubscriptions::default(),
            runtime: Arc::new(library_runtime::session::LibraryRuntime::default()),
            workers: library_runtime::workers::LibraryWorkers::default(),
            sync_wake,
            sync_requests: Some(sync_requests),
            asset_wake,
            asset_requests: Some(asset_requests),
            remote_changes: None,
            storage_locators: None,
            _scanner: Arc::new(scanner),
        }
    }

    pub(crate) fn with_sync(mut self, sync: Arc<crate::sync::LibrarySync>) -> Self {
        self.sync = Some(sync);
        self
    }

    pub(crate) fn with_remote_changes(mut self, remote_changes: crate::notification_interest::RemoteChanges) -> Self {
        self.remote_changes = Some(remote_changes);
        self
    }

    pub(crate) fn with_storage_locators(mut self, root: String, database: String) -> Self {
        self.storage_locators = Some((root, database));
        self
    }

    async fn local_storage_usage(&self) -> Result<Option<u64>, BackendError> {
        if !self.assets.local_access_enabled() {
            return Ok(None);
        }
        let Some((root, database)) = &self.storage_locators else {
            return Ok(None);
        };
        crate::storage::files::book_bytes(root, database).await.map(Some).map_err(BackendError::operation)
    }

    pub(crate) fn metadata(&self) -> &metadata_client::MetadataClient {
        &self.metadata
    }

    pub(crate) fn sync(&self) -> Option<Arc<crate::sync::LibrarySync>> {
        self.sync.clone()
    }

    pub(crate) fn subscribe_updates(&self) -> async_channel::Receiver<library_model::LibraryUpdate> {
        self.subscriptions.subscribe_updates()
    }

    /// Transfers sole ownership of this session to its long-lived actor.
    /// Callers retain only the returned handle, never a resource-bearing
    /// session clone.
    pub(crate) fn spawn_actor(mut self, executor: &crate::executor::BackendExecutor) -> LibraryHandle {
        let (commands, receiver) = async_channel::unbounded();
        let handle = LibraryHandle { id: self.id, commands, subscriptions: self.subscriptions.clone() };
        let worker_commands = handle.commands.clone();
        self.db.observe_writes(move |writes| {
            if writes.assets {
                let _ = worker_commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets));
            }
            if writes.metadata {
                let _ = worker_commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: false }));
            }
        });
        self.start_account_observer(executor, handle.commands.clone());
        self.start_remote_observer(executor, handle.commands.clone());
        self.start_sync(executor);
        self.start_assets(executor);
        let mut events = self.event_rx.take().expect("a library session has one event receiver");
        let event_commands = handle.commands.clone();
        executor.spawn_detached(async move {
            let actor = LibraryActor { session: self };
            let sync_wake = actor.session.sync_wake.clone();
            let asset_wake = actor.session.asset_wake.clone();
            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Ok(event) => actor.publish_event(event, &event_commands),
                        Err(_) => break,
                    },
                    command = receiver.recv() => match command {
                        Ok(command) => {
                            if !actor.handle_command(command, &sync_wake, &asset_wake).await {
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
            drop(actor);
        });
        handle
    }

    /// Starts the durable sync worker owned by this session.
    pub(crate) fn start_sync(&mut self, executor: &crate::executor::BackendExecutor) {
        let Some(requests) = self.sync_requests.take() else {
            return;
        };
        let scheduler = self.sync_wake.clone();
        let Some(sync) = self.sync() else {
            return;
        };
        self.workers.set_sync(executor.spawn(scheduler.run(
            move |refresh_remote| {
                let sync = sync.clone();
                async move {
                    let result = async {
                        if !sync.bind_current_account().map_err(|error| error.to_string())? {
                            return Ok(());
                        }
                        sync.ensure_server_library().await?;
                        if refresh_remote { sync.synchronize_state().await.map(|_| ()).map_err(|error| error.to_string()) } else { sync.drain_outbox().await.map(|_| ()).map_err(|error| error.to_string()) }
                    }
                    .await;
                    result.map_err(|error| log::warn!("library synchronization will resume: {error}"))
                }
            },
            requests,
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(100),
        )));
    }

    /// Account identity is global, but its consequences are local: each
    /// opened library resumes its own durable queues and republishes its own
    /// views.  There is intentionally no AppState -> library fan-out.
    fn start_account_observer(&mut self, executor: &crate::executor::BackendExecutor, commands: async_channel::Sender<LibraryActorCommand>) {
        let Some(sync) = self.sync() else {
            return;
        };
        let Some(mut changed) = sync.subscribe_account_changes() else {
            return;
        };
        changed.borrow_and_update();
        let events = self.event_tx.clone();
        self.workers.set_account_observer(executor.spawn(async move {
            while changed.changed().await.is_ok() {
                let _ = events.try_send(LibraryEvent::ContentsChanged);
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: true }));
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets));
            }
        }));
    }

    /// Server notifications are global transport input, but reconciliation is
    /// owned by each concrete library.  The signal is intentionally lossy:
    /// durable sync state makes a single remote refresh sufficient.
    fn start_remote_observer(&mut self, executor: &crate::executor::BackendExecutor, commands: async_channel::Sender<LibraryActorCommand>) {
        let Some(remote_changes) = &self.remote_changes else {
            return;
        };
        let mut changed = remote_changes.subscribe();
        changed.borrow_and_update();
        self.workers.set_remote(executor.spawn(async move {
            while changed.changed().await.is_ok() {
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: true }));
            }
        }));
    }

    /// Starts this library's durable asset worker.
    pub(crate) fn start_assets(&mut self, executor: &crate::executor::BackendExecutor) {
        let Some(requests) = self.asset_requests.take() else {
            return;
        };
        let scheduler = self.asset_wake.clone();
        let Some(sync) = self.sync() else {
            return;
        };
        self.workers.set_assets(executor.spawn(scheduler.run(
            move |_| {
                let sync = sync.clone();
                async move {
                    if !sync.bind_current_account().map_err(|error| log::warn!("could not bind account before asset reconciliation: {error}"))? {
                        return Ok(());
                    }
                    sync.reconcile_assets().await.map_err(|error| log::warn!("asset reconciliation will resume: {error}"))
                }
            },
            requests,
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(100),
        )));
    }

    pub(crate) fn subscribe_download(&self, content_hash: ContentHash) -> async_channel::Receiver<DownloadState> {
        self.subscriptions.subscribe_download(content_hash)
    }

    pub(crate) fn publish_update(&self, update: library_model::LibraryUpdate) {
        self.subscriptions.publish_update(update);
    }

    pub(super) fn emit(&self, event: LibraryEvent) {
        let _ = self.event_tx.try_send(event);
    }

    pub fn id(&self) -> &LibraryId {
        &self.id
    }

    pub(crate) fn thumbnail_work_running(&self) -> bool {
        self.runtime.thumbnail_work_running()
    }

    pub(crate) fn thumbnail_batch_progress(&self) -> Option<crate::ScanProgress> {
        self.runtime.thumbnail_batch_progress()
    }

    pub(crate) fn thumbnail_batch_waiting(&self) -> bool {
        self.runtime.thumbnail_batch_waiting()
    }

    pub(super) fn prepare_thumbnail_batch(&self, pending: u64) {
        self.runtime.prepare_thumbnail_batch(pending);
    }

    pub(super) fn complete_thumbnail_batch_item(&self) {
        self.runtime.complete_thumbnail_batch_item();
        let _ = self.event_tx.try_send(LibraryEvent::TransferStatusChanged);
    }

    pub(super) fn begin_thumbnail_generation(&self) -> library_runtime::session::ThumbnailActivity {
        self.runtime.begin_thumbnail_generation(self.event_tx.clone())
    }

    pub(crate) fn begin_thumbnail_work(&self) -> Option<LibraryActivity> {
        self.runtime.begin_thumbnail_work(self.event_tx.clone())
    }

    pub(crate) fn begin_local_work(&self) -> Option<LibraryActivity> {
        self.runtime.begin_local_work(self.event_tx.clone())
    }

    /// True while the local filesystem scanner is scheduled or actively
    /// walking this library.
    pub(crate) fn discovery_enabled(&self) -> bool {
        self._scanner.enabled()
    }

    pub fn scanner_running(&self) -> bool {
        if self.runtime.has_directory_imports() {
            return true;
        }
        self._scanner.progress().is_some()
    }

    #[cfg(all(test, feature = "scanner", not(target_os = "android")))]
    pub(crate) fn shares_scanner_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self._scanner, &other._scanner)
    }

    /// Parked scanners have queued work but are not running user operations.
    pub(crate) fn operation_scanning(&self) -> bool {
        self._scanner.operation_scanning()
    }

    /// Enables the initial filesystem scan for a handle opened in deferred mode.
    pub fn start_scanner(&self) {
        if self.assets.local_access_enabled() {
            self._scanner.start_scanning();
        }
    }

    pub async fn import_book_bytes(&self, parent_id: &DirId, file_name: String, bytes: Vec<u8>) -> Result<ContentHash, BackendError> {
        self.import_book_reader(parent_id, Box::new(std::io::Cursor::new(bytes)), file_name).await
    }

    pub(crate) fn download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let conn = self.db.get()?;
        self.persistent_download_state(&conn, content_hash)
    }

    pub(crate) async fn request_download(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        if matches!(self.download_state(content_hash)?, DownloadState::Downloaded) {
            return Ok(DownloadState::Downloaded);
        }
        let sync = self.sync().ok_or_else(|| BackendError::message("library sync is unavailable"))?;
        sync.request_download(content_hash).map_err(BackendError::operation)?;
        sync.download_requested(content_hash).await.map_err(BackendError::operation)?;
        self.download_state(content_hash)
    }

    pub(crate) fn request_downloads(&self, hashes: &[ContentHash]) -> Result<(), BackendError> {
        if hashes.is_empty() {
            return Ok(());
        }
        self.sync().ok_or_else(|| BackendError::message("library sync is unavailable"))?.request_downloads(hashes).map_err(BackendError::operation)
    }
}

pub use client_library::{LibraryAuthorsView, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};

#[cfg(all(test, feature = "scanner", not(target_arch = "wasm32")))]
mod import_activity_tests {
    use super::*;

    #[test]
    fn directory_import_duplicate_finish_and_late_progress_leave_other_import_active() {
        let root = tempfile::tempdir().unwrap();
        let app = crate::AppBackend::new(crate::BackendContext::initialize(crate::AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let entry = app.ensure_import_library().unwrap();
        let library = app.library_directory().get(entry.library_id()).unwrap();
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        library.begin_directory_import(first);
        library.begin_directory_import(second);
        library.advance_directory_import(second, 10, 2, 0);
        library.finish_directory_import(first);
        library.finish_directory_import(first);
        library.advance_directory_import(first, 99, 99, 99);
        let progress = library.directory_import_progress().expect("second import still owns activity");
        assert_eq!((progress.total, progress.succeeded, progress.failed), (10, 2, 0));
        library.finish_directory_import(second);
        assert!(library.directory_import_progress().is_none());
    }
}

#[cfg(test)]
mod activity_subscription_tests {
    use super::*;

    #[tokio::test]
    async fn subscription_reports_thumbnail_cancellation_and_overlapping_work() {
        let runtime = Arc::new(library_runtime::session::LibraryRuntime::default());
        let (events, changes) = crate::library::events::library_event_channel();
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

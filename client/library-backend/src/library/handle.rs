//! Public handle for operations scoped to one concrete library.

use std::sync::Arc;

use crate::asset_store::AssetStore;
use crate::library::commands::LibraryCommand;
use crate::library::discovery::LibraryScanner;
use crate::library::events::{LibraryEvent, LibraryEventReceiver, LibraryEventSender, LibraryEventSubscriptions};
use crate::{ContentHash, DirId, DownloadState, LibraryId};
use library_database::Database;
use library_runtime::session::LibraryActivity;

use crate::BackendError;

/// All operations whose meaning is scoped to one concrete library.
///
/// Application workflows supply policy and coordinate networking; this handle
/// has no reference back to the application.
pub struct LibrarySession {
    pub(super) id: LibraryId,
    pub(super) db: std::sync::Arc<Database>,
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
    // Its Drop implementation stops and joins the scanner threads.
    pub(super) _scanner: Arc<LibraryScanner>,
}

struct LibraryActor {
    session: LibrarySession,
    /// Browser book readers and Blob URLs are thread-local resources.
    /// The actor retains them for the lifetime of the page subscription; only
    /// a Send cancellation guard leaves the actor.
    #[cfg(target_arch = "wasm32")]
    books: std::collections::HashMap<u64, super::browser_book::BookLease>,
    #[cfg(target_arch = "wasm32")]
    next_book: u64,
}

impl LibraryActor {
    /// Event book is part of the library actor, not a second worker
    /// that borrows session state through shared runtime objects.
    fn publish_event(&self, event: LibraryEvent, commands: &async_channel::Sender<LibraryActorCommand>) {
        let busy = self.session.runtime.has_directory_imports() || self.session.scanner_running();
        self.session.subscriptions.publish(event.clone(), busy);
        if matches!(event, LibraryEvent::ContentsChanged | LibraryEvent::ImportComplete | LibraryEvent::ScanComplete | LibraryEvent::DownloadStatusChanged { state: crate::DownloadState::Downloaded, .. }) {
            let hashes = match &event {
                LibraryEvent::DownloadStatusChanged { content_hash, .. } => vec![*content_hash],
                _ => Vec::new(),
            };
            let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(hashes)));
        }
        if matches!(event, LibraryEvent::ContentsChanged) {
            let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: false }));
        }
    }

    /// Returns false only when the session has been asked to shut down.
    async fn handle_command(&mut self, command: LibraryActorCommand, sync_wake: &library_runtime::scheduler::SyncScheduler, asset_wake: &library_runtime::scheduler::SyncScheduler) -> bool {
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
            #[cfg(target_arch = "wasm32")]
            LibraryActorCommand::SubscribeBook { content_hash, reply } => {
                let result = async {
                    let (book, lease) = self.session.prepare_book_for_reader(content_hash).await?;
                    let id = self.next_book;
                    self.next_book = self.next_book.wrapping_add(1);
                    self.books.insert(id, lease);
                    Ok::<_, BackendError>((library_runtime::wire::encode_reply(book).map_err(BackendError::operation)?, id))
                }
                .await
                .map_err(|error| error.to_string());
                if let Err(error) = reply.send(result).await {
                    if let Ok((_, id)) = error.0 {
                        self.books.remove(&id);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            LibraryActorCommand::ReleaseBook { id } => {
                self.books.remove(&id);
            }
            #[cfg(not(target_arch = "wasm32"))]
            LibraryActorCommand::ResolveBook { content_hash, reply } => {
                let _ = reply.send(self.session.prepare_book_for_reader(content_hash).await).await;
            }
            #[cfg(not(target_arch = "wasm32"))]
            LibraryActorCommand::ImportDirectory { parent_id, directory, create_root, reply } => {
                let _ = reply.send(self.session.import_selected_directory(parent_id, directory, create_root).await).await;
            }
            #[cfg(not(target_arch = "wasm32"))]
            LibraryActorCommand::ImportBookPath { parent_id, file_name, path, reply } => {
                let _ = reply.send(self.session.import_book_path(&parent_id, file_name, &path).await).await;
            }
            LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: true }) => sync_wake.request_remote_refresh(),
            LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: false }) => sync_wake.wake(),
            LibraryActorCommand::Control(LibraryControl::WakeAssets(hashes)) => {
                if hashes.is_empty() {
                    asset_wake.wake();
                } else {
                    asset_wake.wake_hashes(hashes);
                }
                if let Err(error) = self.session.run_upload_preparation().await {
                    log::warn!("library preparation will resume: {error}");
                }
            }
            LibraryActorCommand::Control(LibraryControl::StartScanner) => self.session.start_scanner(),
            LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence }) => {
                let result = (|| -> Result<(), BackendError> {
                    self.session.db.set_asset_storage_policy(enabled, reset_cloud_presence)?;
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
pub struct LibraryHandle {
    id: LibraryId,
    commands: async_channel::Sender<LibraryActorCommand>,
    subscriptions: LibraryEventSubscriptions,
    /// Keeps this library's dedicated thread alive for as long as any handle
    /// to it exists. Never used directly beyond that; see
    /// [`crate::executor::LibraryExecutor`].
    #[cfg(not(target_arch = "wasm32"))]
    _library_executor: crate::executor::LibraryExecutor,
}

#[derive(Clone, Debug)]
enum LibraryControl {
    WakeSync { refresh_remote: bool },
    WakeAssets(Vec<ContentHash>),
    StartScanner,
    SetAssetStorage { enabled: bool, reset_cloud_presence: bool },
    RepairInventory,
    SetDisplayName { name: String },
    Rename { name: String },
    Shutdown,
}

enum LibraryActorCommand {
    Control(LibraryControl),
    Ui {
        command: LibraryCommand,
        reply: async_channel::Sender<Result<library_runtime::wire::WorkerPayload, String>>,
    },
    Scanning {
        reply: async_channel::Sender<bool>,
    },
    DownloadState {
        content_hash: ContentHash,
        reply: async_channel::Sender<Result<DownloadState, BackendError>>,
    },
    TransferSnapshot {
        reply: async_channel::Sender<Result<crate::LibraryTransfers, BackendError>>,
    },
    SyncStatus {
        library_name: String,
        account: Option<account_client::Session>,
        fallback_server_url: String,
        remote_present: Option<bool>,
        reply: async_channel::Sender<Result<crate::LibrarySyncStatus, BackendError>>,
    },
    LocalStorageUsage {
        reply: async_channel::Sender<Result<Option<u64>, BackendError>>,
    },
    #[cfg(target_arch = "wasm32")]
    SubscribeBook {
        content_hash: ContentHash,
        reply: async_channel::Sender<Result<(library_runtime::wire::WorkerPayload, u64), String>>,
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
        reply: async_channel::Sender<Result<crate::library::ResolvedBook, BackendError>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ImportDirectory {
        parent_id: DirId,
        directory: crate::DirectoryImport,
        create_root: bool,
        reply: async_channel::Sender<Result<Vec<crate::ImportFailure>, String>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ImportBookPath {
        parent_id: DirId,
        file_name: String,
        path: std::path::PathBuf,
        reply: async_channel::Sender<Result<ContentHash, BackendError>>,
    },
}

/// A Send-only token for a browser book held by the library actor.
/// Dropping the worker subscription releases its actor-owned resource.
#[cfg(target_arch = "wasm32")]
pub(crate) struct BookSubscription {
    commands: async_channel::Sender<LibraryActorCommand>,
    id: u64,
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
    pub fn open(spec: super::LibraryRuntimeSpec, _executor: &crate::executor::BackendExecutor) -> Result<Self, BackendError> {
        let library_executor = crate::executor::LibraryExecutor::new().map_err(BackendError::operation)?;
        let executor_for_actor = library_executor.clone();
        let handle = library_executor.run_blocking(move || -> Result<LibraryHandle, BackendError> {
            let session = LibrarySession::open(spec)?;
            Ok(session.spawn_actor(executor_for_actor))
        })?;
        handle.wake_sync(true);
        handle.wake_assets();
        Ok(handle)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn open(spec: super::LibraryRuntimeSpec, executor: &crate::executor::BackendExecutor) -> Result<Self, BackendError> {
        let session = LibrarySession::open(spec)?;
        let handle = session.spawn_actor(executor);
        handle.wake_sync(true);
        handle.wake_assets();
        Ok(handle)
    }

    pub fn id(&self) -> LibraryId {
        self.id
    }

    pub fn wake_sync(&self, refresh_remote: bool) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote }));
    }

    pub fn wake_assets(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(Vec::new())));
    }

    pub fn start_scanner(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::StartScanner));
    }

    /// Applies a registry-selected cloud policy inside the owning session.
    pub fn set_asset_storage(&self, enabled: bool, reset_cloud_presence: bool) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence }));
    }

    pub fn repair_inventory(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::RepairInventory));
    }

    /// Applies a name discovered remotely without creating a new rename
    /// mutation to send back to that same remote.
    pub fn set_display_name(&self, name: String) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetDisplayName { name }));
    }

    pub fn rename(&self, name: String) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::Rename { name }));
    }

    pub fn shutdown(&self) {
        let _ = self.commands.try_send(LibraryActorCommand::Control(LibraryControl::Shutdown));
    }

    /// Delivers the concrete UI protocol command to its owning actor.
    pub async fn dispatch(&self, command: LibraryCommand) -> Result<library_runtime::wire::WorkerPayload, String> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::Ui { command, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())?
    }

    pub async fn scanning(&self) -> Result<bool, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::Scanning { reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))
    }

    pub async fn download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::DownloadState { content_hash, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub async fn transfer_snapshot(&self) -> Result<crate::LibraryTransfers, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::TransferSnapshot { reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    /// Reads sync state through the session actor. The application supplies
    /// account discovery context, but never opens the library database itself.
    pub async fn sync_status(&self, library_name: String, account: Option<account_client::Session>, fallback_server_url: String, remote_present: Option<bool>) -> Result<crate::LibrarySyncStatus, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::SyncStatus { library_name, account, fallback_server_url, remote_present, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub async fn local_storage_usage(&self) -> Result<Option<u64>, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::LocalStorageUsage { reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn subscribe_book(&self, content_hash: ContentHash) -> Result<(library_runtime::wire::WorkerPayload, BookSubscription), String> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::SubscribeBook { content_hash, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        let (initial, id) = result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())??;
        Ok((initial, BookSubscription { commands: self.commands.clone(), id }))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn resolve_book(&self, content_hash: ContentHash) -> Result<crate::library::ResolvedBook, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ResolveBook { content_hash, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_selected_directory(&self, parent_id: DirId, directory: crate::DirectoryImport, create_root: bool) -> Result<Vec<crate::ImportFailure>, String> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ImportDirectory { parent_id, directory, create_root, reply }).await.map_err(|_| "library actor is no longer running".to_owned())?;
        result.recv().await.map_err(|_| "library actor stopped before replying".to_owned())?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_book_path(&self, parent_id: DirId, file_name: String, path: std::path::PathBuf) -> Result<ContentHash, BackendError> {
        let (reply, result) = async_channel::bounded(1);
        self.commands.send(LibraryActorCommand::ImportBookPath { parent_id, file_name, path, reply }).await.map_err(|_| BackendError::message("library actor is no longer running"))?;
        result.recv().await.map_err(|_| BackendError::message("library actor stopped before replying"))?
    }

    pub fn subscribe_updates(&self) -> async_channel::Receiver<library_model::LibraryUpdate> {
        self.subscriptions.subscribe_updates()
    }

    pub fn subscribe_download(&self, content_hash: ContentHash) -> async_channel::Receiver<DownloadState> {
        self.subscriptions.subscribe_download(content_hash)
    }
}

impl LibrarySession {
    /// Combines library-owned durable sync details with the application-owned
    /// account/discovery snapshot for settings presentation.
    fn sync_status(&self, library_name: String, account: Option<&account_client::Session>, fallback_server_url: &str, remote_present: Option<bool>) -> Result<crate::LibrarySyncStatus, BackendError> {
        let sync_details = self.db.stored_sync_status()?;
        let (state, server_url, account_id) = match account {
            None if sync_details.has_unsynced_changes || sync_details.last_synced_at.is_none() => (crate::LibrarySyncState::SignInRequired, Some(fallback_server_url.to_owned()), None),
            None => (crate::LibrarySyncState::LocalOnly, None, None),
            Some(account) => {
                let state = if remote_present == Some(false) {
                    crate::LibrarySyncState::RemoteMissing
                } else if !sync_details.has_unsynced_changes && sync_details.last_synced_at.is_none() {
                    if remote_present.is_some() {
                        crate::LibrarySyncState::Receiving
                    } else {
                        crate::LibrarySyncState::Checking
                    }
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
        None
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

    pub(crate) fn from_parts(id: LibraryId, db: std::sync::Arc<Database>, assets: AssetStore, metadata: metadata_client::MetadataClient, event_tx: LibraryEventSender, event_rx: LibraryEventReceiver, scanner: LibraryScanner) -> Self {
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

    async fn local_storage_usage(&self) -> Result<Option<u64>, BackendError> {
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
            Ok(None)
        }
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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spawn_actor(mut self, library_executor: crate::executor::LibraryExecutor) -> LibraryHandle {
        let (commands, receiver) = async_channel::unbounded();
        let handle_commands = commands.clone();
        let subscriptions = self.subscriptions.clone();
        let id = self.id;
        self.start_account_observer(handle_commands.clone());
        self.start_remote_observer(handle_commands.clone());
        self.start_sync();
        self.start_assets();
        let mut events = self.event_rx.take().expect("a library session has one event receiver");
        let event_commands = handle_commands;
        crate::executor::LibraryExecutor::spawn_local_detached(async move {
            let mut actor = LibraryActor { session: self };
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
        LibraryHandle { id, commands, subscriptions, _library_executor: library_executor }
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn spawn_actor(mut self, executor: &crate::executor::BackendExecutor) -> LibraryHandle {
        let (commands, receiver) = async_channel::unbounded();
        let handle = LibraryHandle { id: self.id, commands, subscriptions: self.subscriptions.clone() };
        self.start_account_observer(executor, handle.commands.clone());
        self.start_remote_observer(executor, handle.commands.clone());
        self.start_sync(executor);
        self.start_assets(executor);
        let mut events = self.event_rx.take().expect("a library session has one event receiver");
        let event_commands = handle.commands.clone();
        executor.spawn_detached(async move {
            let mut actor = LibraryActor { session: self, books: std::collections::HashMap::new(), next_book: 1 };
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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_sync(&mut self) {
        let Some(requests) = self.sync_requests.take() else {
            return;
        };
        let scheduler = self.sync_wake.clone();
        let Some(sync) = self.sync() else {
            return;
        };
        let database = self.db.clone();
        let assets = self.assets.clone();
        self.workers.set_sync(crate::executor::LibraryExecutor::spawn_local(scheduler.run(
            move |input| {
                let sync = sync.clone();
                let database = database.clone();
                let assets = assets.clone();
                async move {
                    let result = async {
                        if !sync.bind_current_account().map_err(|error| error.to_string())? {
                            return Ok(());
                        }
                        sync.ensure_server_library().await?;
                        if input.remote {
                            sync.synchronize_state().await.map(|_| ()).map_err(|error| error.to_string())
                        } else {
                            sync.drain_outbox().await.map(|_| ()).map_err(|error| error.to_string())
                        }
                    }
                    .await;
                    // Sync can't replay this itself (SyncStore futures must
                    // stay Send); the orchestrator owns local state instead.
                    if result.is_ok() {
                        if let Err(error) = assets.run_directory_work(&database).await {
                            log::warn!("directory rename will retry from the durable queue: {error}");
                        }
                    }
                    result.map_err(|error| log::warn!("library synchronization will resume: {error}"))
                }
            },
            requests,
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(100),
        )));
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn start_sync(&mut self, executor: &crate::executor::BackendExecutor) {
        let Some(requests) = self.sync_requests.take() else {
            return;
        };
        let scheduler = self.sync_wake.clone();
        let Some(sync) = self.sync() else {
            return;
        };
        let database = self.db.clone();
        self.workers.set_sync(executor.spawn(scheduler.run(
            move |input| {
                let sync = sync.clone();
                let database = database.clone();
                async move {
                    let result = async {
                        if !sync.bind_current_account().map_err(|error| error.to_string())? {
                            return Ok(());
                        }
                        sync.ensure_server_library().await?;
                        if input.remote {
                            sync.synchronize_state().await.map(|_| ()).map_err(|error| error.to_string())
                        } else {
                            sync.drain_outbox().await.map(|_| ()).map_err(|error| error.to_string())
                        }
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
    #[cfg(not(target_arch = "wasm32"))]
    fn start_account_observer(&mut self, commands: async_channel::Sender<LibraryActorCommand>) {
        let Some(sync) = self.sync() else {
            return;
        };
        let Some(mut changed) = sync.subscribe_account_changes() else {
            return;
        };
        changed.borrow_and_update();
        let events = self.event_tx.clone();
        self.workers.set_account_observer(crate::executor::LibraryExecutor::spawn_local(async move {
            while changed.changed().await.is_ok() {
                let _ = events.try_send(LibraryEvent::ContentsChanged);
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: true }));
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(Vec::new())));
            }
        }));
    }
    #[cfg(target_arch = "wasm32")]
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
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(Vec::new())));
            }
        }));
    }

    /// Server notifications are global transport input, but reconciliation is
    /// owned by each concrete library.  The signal is intentionally lossy:
    /// durable sync state makes a single remote refresh sufficient.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_remote_observer(&mut self, commands: async_channel::Sender<LibraryActorCommand>) {
        let Some(remote_changes) = &self.remote_changes else {
            return;
        };
        let mut changed = remote_changes.subscribe();
        changed.borrow_and_update();
        self.workers.set_remote(crate::executor::LibraryExecutor::spawn_local(async move {
            while changed.changed().await.is_ok() {
                let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeSync { refresh_remote: true }));
            }
        }));
    }
    #[cfg(target_arch = "wasm32")]
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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_assets(&mut self) {
        let Some(requests) = self.asset_requests.take() else {
            return;
        };
        let scheduler = self.asset_wake.clone();
        let Some(sync) = self.sync() else {
            return;
        };
        let database = self.db.clone();
        self.workers.set_assets(crate::executor::LibraryExecutor::spawn_local(scheduler.run(
            move |input| {
                let sync = sync.clone();
                let database = database.clone();
                async move {
                    if !sync.bind_current_account().map_err(|error| log::warn!("could not bind account before asset reconciliation: {error}"))? {
                        return Ok(());
                    }
                    sync.reconcile_assets(input.hashes).await.map_err(|error| log::warn!("asset reconciliation will resume: {error}"))
                }
            },
            requests,
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(100),
        )));
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn start_assets(&mut self, executor: &crate::executor::BackendExecutor) {
        let Some(requests) = self.asset_requests.take() else {
            return;
        };
        let scheduler = self.asset_wake.clone();
        let Some(sync) = self.sync() else {
            return;
        };
        let database = self.db.clone();
        self.workers.set_assets(executor.spawn(scheduler.run(
            move |input| {
                let sync = sync.clone();
                let database = database.clone();
                async move {
                    if !sync.bind_current_account().map_err(|error| log::warn!("could not bind account before asset reconciliation: {error}"))? {
                        return Ok(());
                    }
                    sync.reconcile_assets(input.hashes).await.map_err(|error| log::warn!("asset reconciliation will resume: {error}"))
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
        self.runtime.has_directory_imports()
    }

    #[cfg(all(test, feature = "scanner", not(target_os = "android")))]
    pub(crate) fn shares_scanner_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self._scanner, &other._scanner)
    }

    /// Parked scanners have queued work but are not running user operations.
    pub(crate) fn operation_scanning(&self) -> bool {
        false
    }

    /// Enables the initial filesystem scan for a handle opened in deferred mode.
    pub fn start_scanner(&self) {
        if self.assets.local_access_enabled() {
            self.run_filesystem_scan();
        }
    }

    /// The session, not the filesystem scanner, owns the scan protocol and
    /// its application events. The scanner sees only immutable input and
    /// returns data; this method decides whether and how to apply that data.
    fn run_filesystem_scan(&self) {
        let snapshot = match self.db.scan_catalog_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!("could not create filesystem scan snapshot: {error}");
                return;
            }
        };
        self.emit(LibraryEvent::ScanStarted);
        let discovery = self._scanner.discover(&snapshot);
        let inspected = self._scanner.inspect(&discovery);
        match self.db.apply_filesystem_scan(&snapshot, &discovery, &inspected) {
            Ok(library_database::ScanApplyResult::Applied) => self.emit(LibraryEvent::ContentsChanged),
            Ok(library_database::ScanApplyResult::Stale) => log::debug!("filesystem scan became stale before apply"),
            Err(error) => log::warn!("could not apply filesystem scan: {error}"),
        }
        self.emit(LibraryEvent::ScanComplete);
    }

    pub async fn import_book_bytes(&self, parent_id: &DirId, file_name: String, bytes: Vec<u8>) -> Result<ContentHash, BackendError> {
        self.import_book_reader(parent_id, Box::new(std::io::Cursor::new(bytes)), file_name).await
    }

    pub(crate) fn download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        self.persistent_download_state(content_hash)
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
        self.sync().ok_or_else(|| BackendError::message("library sync is unavailable"))?.request_downloads(hashes).map_err(BackendError::operation)?;
        // Enqueue and wake together: the next pass plans exactly these
        // hashes first instead of discovering them in the drain.
        self.asset_wake.wake_hashes(hashes.to_vec());
        Ok(())
    }
}

pub use library_database::{LibraryAuthorsView, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};

#[cfg(all(test, feature = "scanner", feature = "app-integration-tests", not(target_arch = "wasm32")))]
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

pub(super) use super::data::{AppDataLocation, BackendContext, LibraryEntry, LibraryRegistry};
pub(super) use crate::LibraryId;
use crate::executor::{BackendExecutor, BackendTask};
use serde::Serialize;
pub(super) use std::collections::HashSet;
pub(super) use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub(super) const BACKGROUND_SYNC_RETRY_INTERVAL: Duration = Duration::from_secs(30);

pub(super) use crate::BackendError;
pub(super) use crate::DeviceAccountSession;
pub(super) use crate::library::{LibrarySyncState, LibrarySyncStatus};

#[derive(Clone, Debug, serde::Deserialize, Serialize)]
pub struct LibraryListState {
    pub libraries: Vec<LibraryEntry>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
}

pub(super) fn subscribe_event(subscribers: &mut Vec<async_channel::Sender<()>>) -> async_channel::Receiver<()> {
    let (sender, receiver) = async_channel::bounded(1);
    subscribers.push(sender);
    receiver
}

/// One unread notification is sufficient. A full channel remains subscribed.
pub(super) fn invalidate_events(subscribers: &mut Vec<async_channel::Sender<()>>) {
    subscribers.retain(|sender| !matches!(sender.try_send(()), Err(async_channel::TrySendError::Closed(()))));
}

pub(crate) struct AppState {
    pub(super) executor: BackendExecutor,
    pub(super) registry: LibraryRegistry,
    /// Durable registry/account policy remain app-owned. The library module
    /// owns the in-memory directory of resource-bearing sessions.
    pub(super) library_directory: crate::library::LibraryDirectory,
    pub(super) account: DeviceAccountSession,
    pub(super) library_list_subscribers: std::sync::Mutex<Vec<async_channel::Sender<()>>>,
    pub(super) account_scheduler: SyncScheduler,
    pub(super) remote_changes: crate::notification_interest::RemoteChanges,
    notification_task: Option<BackendTask<()>>,
    pub(super) notification_interest: crate::notification_interest::NotificationInterest,
}

#[cfg(test)]
pub(super) use crate::sync::PersistedSyncWork;
#[cfg(test)]
use crate::sync::SyncRequest;
pub(super) use crate::sync::SyncScheduler;

impl Drop for AppState {
    fn drop(&mut self) {
        self.account_scheduler.stop();
        self.notification_task.take();
        self.library_directory.shutdown_all();
    }
}

#[derive(Clone)]
pub(crate) struct AppBackend {
    pub(super) state: Arc<AppState>,
}

impl AppBackend {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn run_native_worker(self, worker: crate::runtime::native::AppWorker) {
        let executor = self.state.executor.clone();
        executor.block_on(worker.serve(self));
    }

    pub fn initialize(location: AppDataLocation) -> Result<Self, BackendError> {
        Self::new(BackendContext::initialize(location).map_err(BackendError::message)?)
    }

    pub fn new(context: BackendContext) -> Result<Self, BackendError> {
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        let notification_interest = crate::notification_interest::NotificationInterest::default();
        let remote_changes = crate::notification_interest::RemoteChanges::default();
        let (account_scheduler, account_wake_rx) = SyncScheduler::new();
        let api_server = crate::api_server_url();
        let account = DeviceAccountSession::load(context.clone(), api_server).map_err(BackendError::operation)?;
        let notification_task = executor.spawn(super::notifications::run(account.clone(), account_scheduler.clone(), notification_interest.clone(), remote_changes.clone()));
        let state = Arc::new(AppState {
            executor: executor.clone(),
            registry: context.registry,
            library_directory: crate::library::LibraryDirectory::default(),
            account: account.clone(),
            library_list_subscribers: std::sync::Mutex::new(Vec::new()),
            account_scheduler: account_scheduler.clone(),
            remote_changes: remote_changes.clone(),
            notification_task: Some(notification_task),
            notification_interest: notification_interest.clone(),
        });
        let weak = Arc::downgrade(&state);
        super::account::tasks::start_maintenance(&executor, &weak, (account_scheduler.clone(), account_wake_rx));
        let core = Self { state };
        core.open_registered_libraries()?;
        core.state.account_scheduler.request_remote_refresh();
        Ok(core)
    }

    pub(crate) fn library_list_changes(&self) -> async_channel::Receiver<()> {
        subscribe_event(&mut self.state.library_list_subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    pub(super) fn send_library_list_changed(&self) {
        invalidate_events(&mut self.state.library_list_subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    }

    /// Runtime routing may resolve an already-open actor here. It cannot open
    /// or configure libraries; that remains the explicit lifecycle path.
    pub(crate) fn library_directory(&self) -> &crate::library::LibraryDirectory {
        &self.state.library_directory
    }

    pub(super) fn library_entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        self.state.registry.entry(library_id).map_err(BackendError::operation)?.ok_or_else(|| BackendError::message(format!("library not found: {library_id}")))
    }
}

pub(super) fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}

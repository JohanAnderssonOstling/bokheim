//! Shared async access and lifetime observation for one library.

use super::workers::{observe_account, process_events, publish_activity, run_workers};
use super::{LibraryOpenOptions, LibraryResources};
use crate::executor::{BackendExecutor, BackendTask};
use crate::{BackendError, LibraryId};
use client_platform_runtime::executor::BoxedBackendFuture;
use client_platform_runtime::task_scope::until_cancelled;
use library_database::DatabaseClient;
use library_placement::LibraryState;
use std::future::Future;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::OwnedRwLockReadGuard;

/// Keep a file operation admitted until it finishes. Shared access does not
/// serialize ordinary actions; only root replacement takes exclusive access.
pub(super) struct StorageSnapshot {
    state: LibraryState,
    #[cfg(not(target_arch = "wasm32"))]
    _access: OwnedRwLockReadGuard<()>,
}

impl Deref for StorageSnapshot {
    type Target = LibraryState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

#[derive(Clone)]
pub struct LibraryHandle {
    pub(super) owner: Arc<LibraryOwner>,
    pub(super) host: Option<Arc<dyn Send + Sync>>,
}

/// Caller-owned lifetime. Workers retain only `resources`, so dropping the last
/// handle stops them without a cycle through their own task handle.
pub(super) struct LibraryOwner {
    pub resources: Arc<LibraryResources>,
    pub executor: BackendExecutor,
    pub stopped: watch::Sender<bool>,
    pub workers: Mutex<Option<BackendTask<()>>>,
}

impl LibraryOwner {
    fn close(&self) {
        self.stopped.send_replace(true);
        self.resources.event_state.lock().expect("library events").close();
        self.workers.lock().expect("library workers").take();
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut pause) = self.resources.root_pause.try_lock() {
            pause.take();
        }
        #[cfg(target_arch = "wasm32")]
        self.resources.books.lock().expect("library book leases").clear();
    }
}

impl Drop for LibraryOwner {
    fn drop(&mut self) {
        self.close();
    }
}

impl LibraryHandle {
    pub async fn open(options: LibraryOpenOptions, executor: &BackendExecutor) -> Result<Self, BackendError> {
        let resources = LibraryResources::open(options, executor).await?;
        Ok(Self::from_resources(resources, executor.clone()))
    }

    pub(super) fn from_resources(resources: LibraryResources, executor: BackendExecutor) -> Self {
        let resources = Arc::new(resources);
        let stopped = watch::channel(false).0;
        let handle = Self { owner: Arc::new(LibraryOwner { resources, executor, stopped, workers: Mutex::new(None) }), host: None };
        handle.start_workers();
        handle
    }

    pub(super) fn start_workers(&self) {
        let mut workers = self.owner.workers.lock().expect("library workers");
        if self.is_closed() {
            return;
        }
        let resources = self.owner.resources.clone();
        let task = self.owner.executor.spawn(async move {
            resources.events.sync.request_remote_refresh();
            resources.events.assets.wake();
            resources.events.files.wake();
            let observers = observe_account(resources.account.clone(), resources.events.clone());
            let workers = run_workers(&resources, observers);
            let events: BoxedBackendFuture<'_, ()> = Box::pin(process_events(&resources));
            let activity: BoxedBackendFuture<'_, ()> = Box::pin(publish_activity(&resources));
            futures_util::join!(workers, events, activity);
        });
        workers.replace(task);
    }

    /// Attach the application lifetime to a client copy. The service retains
    /// handles without a host, so this does not create an ownership cycle.
    pub fn new(mut handle: Self, host: Arc<dyn Send + Sync>) -> Self {
        handle.host = Some(host);
        handle
    }

    /// Wait while the root is paused, then retain shared file access for this
    /// operation. Shutdown also wakes callers waiting for recovery.
    pub(super) async fn storage(&self) -> Result<StorageSnapshot, BackendError> {
        until_cancelled(
            async {
                // Root replacement drains file access before locking state.
                #[cfg(not(target_arch = "wasm32"))]
                let access = self.owner.resources.file_access.clone().read_owned().await;
                let state = self.owner.resources.state.read().await;
                StorageSnapshot {
                    state: state.clone(),
                    #[cfg(not(target_arch = "wasm32"))]
                    _access: access,
                }
            },
            self.closed(),
        )
        .await
        .ok_or_else(|| BackendError::message("library is no longer running"))
    }

    /// Database calls own their scheduling and connection access.
    pub(super) fn database(&self) -> &DatabaseClient {
        &self.owner.resources.database
    }

    /// Provide the native I/O runtime context for asynchronous work.
    /// Callers explicitly choose any root-change guards or activity reporting.
    pub(super) async fn run_io<T>(&self, operation: impl Future<Output = Result<T, BackendError>>) -> Result<T, BackendError> {
        self.owner.executor.with_io_context(operation).await
    }

    pub fn is_closed(&self) -> bool {
        *self.owner.stopped.borrow()
    }

    pub async fn closed(&self) {
        let mut stopped = self.owner.stopped.subscribe();
        let _ = stopped.wait_for(|closed| *closed).await;
    }

    pub fn id(&self) -> &LibraryId {
        &self.owner.resources.id
    }

    pub fn shutdown(&self) {
        self.owner.close();
    }
}

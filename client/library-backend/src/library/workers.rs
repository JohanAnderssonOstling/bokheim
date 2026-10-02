//! Lifetime ownership for this library's background tasks.

use std::future::Future;
use std::time::Duration;
use client_platform_runtime::executor::BackendTask;

/// Dropping this value cancels/joins every task held by the platform task
/// wrapper. A session therefore owns all of its workers as one unit.
#[derive(Default)]
pub struct LibraryWorkers {
    pub(super) account_observer: Option<BackendTask<()>>,
    pub(super) remote: Option<BackendTask<()>>,
    pub(super) sync: Option<BackendTask<()>>,
    pub(super) assets: Option<BackendTask<()>>,
}

use super::events::{LibraryEvent, LibraryEventSender};
use super::handle::{LibraryActorCommand, LibraryControl};
use super::worker_state::WorkerState;
use crate::{sync::LibrarySync, BackendError};
use library_runtime::scheduler::{SyncRequest, SyncScheduler};
use std::sync::Arc;

/// Subscribe before spawning so startup cannot miss an account change.
pub(super) fn observe_account_changes(account: crate::integration::LibraryAccount, commands: async_channel::Sender<LibraryActorCommand>, sync_wake: SyncScheduler, events: LibraryEventSender) -> impl Future<Output = ()> {
    let mut changed = account.subscribe();
    changed.borrow_and_update();
    async move {
        while changed.changed().await.is_ok() {
            let _ = events.try_send(LibraryEvent::ContentsChanged);
            sync_wake.request_remote_refresh();
            let _ = commands.try_send(LibraryActorCommand::Control(LibraryControl::WakeAssets(Vec::new())));
        }
    }
}

pub(super) fn observe_remote_changes(remote_changes: crate::notification_interest::RemoteChanges, sync_wake: SyncScheduler) -> impl Future<Output = ()> {
    let mut changed = remote_changes.subscribe();
    changed.borrow_and_update();
    async move {
        while changed.changed().await.is_ok() {
            sync_wake.request_remote_refresh();
        }
    }
}

/// Create the remote library once for the current server and user.
async fn ensure_remote_library(sync: &LibrarySync, state: &WorkerState) -> Result<(), BackendError> {
    let account = sync.account().ok_or_else(|| BackendError::message("library has no account capability"))?;
    let session = account.require().map_err(crate::BackendError::operation)?;
    let identity = format!("{}|{}", session.server_url(), session.user_id());
    if let Some(name) = state.registration(identity.clone()).await? {
        account.create_library(sync.library_id, name).await.map_err(crate::BackendError::operation)?;
        state.registered(identity).await?;
    }
    Ok(())
}

pub(super) async fn run_sync_scheduler(sync: Arc<LibrarySync>, requests: async_channel::Receiver<SyncRequest>, #[cfg(not(target_arch = "wasm32"))] assets: crate::asset_store::AssetStore, state: WorkerState) {
    library_runtime::scheduler::run_timed(
        move |input| {
            let sync = sync.clone();
            let state = state.clone();
            #[cfg(not(target_arch = "wasm32"))]
            let assets = assets.clone();
            async move {
                let result = async {
                    if !sync.bind_current_account().map_err(crate::BackendError::operation)? {
                        // Account discovery can lag behind library startup.
                        // This is a deferred pass, not a successful sync:
                        // returning an error preserves the scheduler retry
                        // until the account observer wakes us on sign-in.
                        return Err(BackendError::message("waiting for account session"));
                    }
                    ensure_remote_library(&sync, &state).await?;
                    if input.remote {
                        sync.synchronize_state_mode(false).await.map(|_| ()).map_err(crate::BackendError::operation)
                    } else {
                        sync.drain_outbox().await.map(|_| ()).map_err(crate::BackendError::operation)
                    }
                }
                .await;
                // Sync can't replay this itself (SyncStore futures must
                // stay Send); the orchestrator owns local state instead.
                #[cfg(not(target_arch = "wasm32"))]
                if result.is_ok() {
                    if let Err(error) = state.replay_directories(&assets).await {
                        log::warn!("directory rename will retry from the durable queue: {error}");
                    }
                }
                result.map_err(|error| log::warn!("library synchronization will resume: {error}"))
            }
        },
        requests,
        Duration::from_secs(30),
        Duration::from_millis(100),
    )
    .await;
}

pub(super) async fn run_asset_scheduler(sync: Arc<LibrarySync>, requests: async_channel::Receiver<SyncRequest>, state: WorkerState) {
    library_runtime::scheduler::run_timed(
        move |input| {
            let sync = sync.clone();
            let state = state.clone();
            async move {
                if !sync.bind_current_account().map_err(|error| log::warn!("could not bind account before asset reconciliation: {error}"))? {
                    // Do not consume a durable asset wake while account
                    // discovery is still in progress. The scheduler will
                    // retry, and the account observer wakes it immediately
                    // when a usable session arrives.
                    return Err(());
                }
                sync.reconcile_assets(input.hashes).await.map_err(|error: crate::sync::error::SyncError| log::warn!("asset reconciliation will resume: {error}"))?;
                // A scheduler wake is only a hint; the database owns the
                // actual queue. `sync_assets` drains eligible paged work
                // directly; remaining work here is blocked and should
                // retain the normal retry policy.
                if state.pending_assets().await.map_err(|error| log::warn!("could not check remaining asset work: {error}"))? {
                    return Err(());
                }
                Ok(())
            }
        },
        requests,
        Duration::from_secs(30),
        Duration::from_millis(100),
    )
    .await;
}

//! Exclusive library-folder moves and recovery.

#[cfg(not(target_arch = "wasm32"))]
use super::activity::ForegroundActivity;
#[cfg(not(target_arch = "wasm32"))]
use super::{LibraryHandle, LibraryResources};
#[cfg(not(target_arch = "wasm32"))]
use crate::executor::run_blocking;
#[cfg(not(target_arch = "wasm32"))]
use crate::BackendError;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::task_scope::until_cancelled;
#[cfg(not(target_arch = "wasm32"))]
use library_model::{LibraryBusyReason, LibraryUpdate};
#[cfg(not(target_arch = "wasm32"))]
use library_placement::LibraryState;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::OwnedRwLockWriteGuard;

#[cfg(not(target_arch = "wasm32"))]
pub type RootMoveOperation = Box<dyn FnOnce(String) -> (String, Result<(), BackendError>) + Send>;

/// The library stays paused until a replacement root is installed. Retaining
/// acquired guards also preserves the pause if an attempt fails or is cancelled.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(super) struct RootPause {
    storage: Option<OwnedRwLockWriteGuard<LibraryState>>,
    file_access: Option<OwnedRwLockWriteGuard<()>>,
    root_access: Option<OwnedRwLockWriteGuard<()>>,
    activity: Option<ForegroundActivity>,
}

#[cfg(not(target_arch = "wasm32"))]
impl LibraryHandle {
    #[cfg(not(target_arch = "wasm32"))]
    async fn pause_files(&self, reason: LibraryBusyReason) -> Result<(), BackendError> {
        let resources = &self.owner.resources;
        let closed = || BackendError::message("library is no longer running");
        let mut pending = until_cancelled(resources.root_pause.lock(), self.closed()).await.ok_or_else(closed)?;
        let pause = pending.get_or_insert_with(RootPause::default);
        pause.activity = resources.activity.start(Some(reason));
        resources.events.updates.publish(LibraryUpdate::BookFileActivity(resources.activity.snapshot(false)));

        if pause.file_access.is_none() {
            // Stop worker admission before draining foreground file workflows.
            let workers = self.owner.workers.lock().expect("library workers").take();
            if let Some(workers) = workers {
                workers.abort();
                let _ = workers.await;
            }
            pause.file_access = Some(until_cancelled(resources.file_access.clone().write_owned(), self.closed()).await.ok_or_else(closed)?);
        }
        // Drain file workflows before taking state: admitted workflows may
        // still need state access to finish. Both stay closed across retries.
        if pause.storage.is_none() {
            pause.storage = Some(until_cancelled(resources.state.clone().write_owned(), self.closed()).await.ok_or_else(closed)?);
        }
        if pause.root_access.is_none() {
            let assets = pause.storage.as_ref().expect("paused library state").assets().clone();
            pause.root_access = Some(until_cancelled(assets.lock_root_exclusive(), self.closed()).await.ok_or_else(closed)?);
        }
        // Aborted workers may still be finishing a synchronous database batch.
        resources.database.run(|_| Ok(())).await?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn root_unavailable(&self) {
        if let Err(error) = self.pause_files(LibraryBusyReason::RecoveringLibrary).await {
            log::warn!("could not pause unavailable library storage: {error}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn change_root(&self, mut root: String, operation: Option<RootMoveOperation>) -> Result<(), BackendError> {
        self.owner
            .executor
            .with_io_context(async {
                let moving = operation.is_some();
                self.pause_files(if moving { LibraryBusyReason::MovingLibrary } else { LibraryBusyReason::RecoveringLibrary }).await?;
                let resources = &self.owner.resources;
                let mut pending = resources.root_pause.lock().await;
                if self.is_closed() {
                    return Err(BackendError::message("library is no longer running"));
                }
                let pause = pending.as_mut().ok_or_else(|| BackendError::message("library root pause was released"))?;
                let mut ready = false;
                let result = async {
                    let storage = pause.storage.as_mut().expect("paused library state");
                    resources.events.updates.reset_transfer_progress();
                    let move_result = if let Some(operation) = operation {
                        let source = storage.assets().native_storage_identity().to_owned();
                        let (committed_root, result) = run_blocking(move || operation(source)).await?;
                        root = committed_root;
                        if std::path::Path::new(&root) == storage.assets().native_library_root() {
                            ready = storage.assets().root_available();
                            return result;
                        }
                        Some(result)
                    } else {
                        None
                    };
                    // Keep the old handles inaccessible while opening the committed
                    // destination. A failed open leaves the complete pause in place.
                    let assets = LibraryResources::open_assets(root.clone(), storage.database()).await?;
                    storage.database().apply_native_root_move(&root).await?;
                    if !moving {
                        resources.reset_local_storage(storage).await?;
                    }
                    resources.watch_root(&assets);
                    storage.replace_assets(assets);
                    ready = true;
                    move_result.unwrap_or(Ok(()))
                }
                .await;
                if ready {
                    pending.take();
                    drop(pending);
                    if !self.is_closed() {
                        resources.publish_book_file_activity().await;
                        self.start_workers();
                        resources.events.refresh_contents();
                    }
                } else {
                    pause.activity = resources.activity.start(Some(LibraryBusyReason::RecoveringLibrary));
                    resources.events.updates.publish(LibraryUpdate::BookFileActivity(resources.activity.snapshot(false)));
                    // Recovery owns its retry backoff. Only failed moves enqueue it.
                    if moving {
                        if let Some(failures) = &resources.root_failures {
                            let _ = failures.try_send(resources.id);
                        }
                    }
                }
                result
            })
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn move_root(&self, operation: RootMoveOperation) -> Result<(), BackendError> {
        self.change_root(String::new(), Some(operation)).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn recover_root(&self, root: String) -> Result<(), BackendError> {
        self.change_root(root, None).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "concurrency_tests.rs"]
pub(super) mod concurrency_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "transfer_tests.rs"]
mod transfer_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "browser_tests.rs"]
mod browser_tests;

#[cfg(all(test, feature = "watcher", not(target_arch = "wasm32")))]
mod watcher_recovery_tests {
    use crate::library::import::watcher::watch;

    #[test]
    fn watcher_can_register_after_library_folder_appears() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("library");
        let scan = library_services::scheduler::SyncScheduler::default();
        assert!(watch(&root, scan.clone()).is_err());
        std::fs::create_dir(&root).unwrap();
        assert!(watch(&root, scan).is_ok());
    }
}

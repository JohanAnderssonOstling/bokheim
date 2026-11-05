//! Changes to cloud storage policy and retention of local books.

use crate::library::{LibraryHandle, LibraryResources};
use crate::BackendError;
use client_platform_runtime::task_scope::OnDrop;
use library_model::{LibraryBusyReason, LibraryUpdate};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

impl LibraryHandle {
    /// Disable eviction immediately, save required local copies, then let the
    /// host commit its storage policy. Preparation failure restores cloud mode.
    pub async fn change_asset_storage(&self, enabled: bool, save_books: bool, commit_policy: impl Future<Output = Result<(), BackendError>>) -> Result<(), BackendError> {
        self.run_io(async {
            let _activity = self.owner.resources.activity.start(save_books.then_some(LibraryBusyReason::SavingBooks));
            let _files = self.storage().await?;
            let library = &self.owner.resources;
            let was_enabled = library.transfers.asset_storage_enabled();
            let committed = AtomicBool::new(false);
            let _restore = OnDrop::new(|| {
                if !committed.load(Ordering::Acquire) {
                    library.transfers.set_asset_storage_enabled(was_enabled);
                }
                library.events.updates.publish(LibraryUpdate::SyncStatusChanged);
            });
            {
                // Wait for any current eviction before disabling further ones.
                let _evictions = library.evictions.write().await;
                if !enabled {
                    library.transfers.set_asset_storage_enabled(false);
                    library.events.updates.publish(LibraryUpdate::SyncStatusChanged);
                }
            }
            if save_books {
                if !library.sync.remote_ready().await.map_err(BackendError::operation)? {
                    return Err(BackendError::message("library is not registered for this account"));
                }
                library.sync.synchronize_state_mode(false).await.map_err(BackendError::operation)?;
                Box::pin(library.save_books_locally()).await?;
            }
            // Only the host's durable commit may release the cloud copies.
            commit_policy.await?;
            committed.store(true, Ordering::Release);
            library.apply_storage_policy(enabled, true).await
        })
        .await
    }

    /// Apply a setting already committed by the host, without starting a task.
    pub async fn apply_storage_policy(&self, enabled: bool, reset_cloud_presence: bool) -> Result<(), BackendError> {
        let library = &self.owner.resources;
        library.apply_storage_policy(enabled, reset_cloud_presence).await
    }
}

impl LibraryResources {
    pub(crate) async fn apply_storage_policy(&self, enabled: bool, reset_cloud_presence: bool) -> Result<(), BackendError> {
        let _evictions = self.evictions.write().await;
        // A failed cache write must not allow stale server-presence flags to
        // authorize eviction after the durable account policy turned off.
        if !enabled {
            self.transfers.set_asset_storage_enabled(false);
        }
        self.database.set_asset_storage_policy(enabled, reset_cloud_presence).await?;
        self.transfers.set_asset_storage_enabled(enabled);
        self.events.assets.wake();
        self.events.sync.request_remote_refresh();
        self.events.updates.publish(library_model::LibraryUpdate::SyncStatusChanged);
        Ok(())
    }
}

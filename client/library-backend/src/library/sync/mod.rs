//! Synchronization status, remote registration, and refresh requests.

use super::{LibraryHandle, LibraryResources};
use crate::{BackendError, LibrarySyncState, LibrarySyncStatus};
use std::sync::atomic::Ordering;

mod policy;

impl LibraryHandle {
    pub async fn sync_status(&self, signed_in: bool, remote_present: Option<bool>) -> Result<crate::LibrarySyncStatus, BackendError> {
        let library = &self.owner.resources;
        library.sync_status(signed_in, remote_present).await
    }

    pub fn request_remote_refresh(&self) {
        if !self.is_closed() {
            self.owner.resources.events.sync.request_remote_refresh();
        }
    }

    pub fn refresh_catalog(&self) {
        if !self.is_closed() {
            self.owner.resources.events.refresh_contents();
        }
    }

    /// Durable confirmation used by management to resume registration after restart.
    pub async fn remote_registration_identity(&self) -> Result<Option<String>, BackendError> {
        let database = self.database();
        Ok(database.sync_metadata("server_library_created").await?)
    }

    /// Management has created or discovered this library for the captured account.
    /// Wake remote work only after the confirmation is durably recorded.
    pub async fn mark_remote_registered(&self, identity: String) -> Result<(), BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let account = library.account.require_session().await.map_err(BackendError::operation)?;
            if account.identity() != identity {
                return Err(BackendError::message("account changed during library registration"));
            }
            library.sync.bind_account(account.server_url().as_str()).map_err(BackendError::operation)?;
            let database = &library.database;
            if database.sync_metadata("server_library_created").await?.as_deref() != Some(identity.as_str()) {
                database.set_sync_metadata("server_library_created", &identity).await?;
                library.events.sync.request_remote_refresh();
                library.events.assets.wake();
            }
            Ok(())
        })
        .await
    }
}

impl LibraryResources {
    /// Combines library-owned durable sync details with the application-owned
    /// account/discovery snapshot for settings presentation.
    pub(crate) async fn sync_status(&self, signed_in: bool, remote_present: Option<bool>) -> Result<LibrarySyncStatus, BackendError> {
        let sync_details = self.database.stored_sync_status().await?;
        let state = match signed_in {
            false => LibrarySyncState::LocalOnly,
            true if self.upload_quota_blocked.load(Ordering::Acquire) => LibrarySyncState::CloudStorageFull,
            true if remote_present == Some(false) => LibrarySyncState::RemoteMissing,
            true if sync_details.has_unsynced_changes => LibrarySyncState::SyncPending,
            true if sync_details.last_synced_at.is_none() => {
                if remote_present.is_some() {
                    LibrarySyncState::Receiving
                } else {
                    LibrarySyncState::Checking
                }
            }
            true => LibrarySyncState::Synced,
        };
        Ok(LibrarySyncStatus {
            book_file_activity: self.activity.snapshot(sync_details.has_download_requests),
            asset_storage_enabled: self.transfers.asset_storage_enabled(),
            storage: sync_details.storage,
            library_id: self.id,
            state,
            has_unsynced_changes: sync_details.has_unsynced_changes,
            last_synced_at: sync_details.last_synced_at,
            issues: sync_details.issues,
            issue_details: sync_details.issue_details,
        })
    }
}

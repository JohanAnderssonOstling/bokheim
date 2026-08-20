//! Loads and applies the combined account, library, transfer, and session snapshot.

use std::future::Future;

use epub_svelte_lib::LibraryTransfers;
use epub_svelte_lib::app::{AccountStatus, AccountStorageUsage, AppBackend, LibrarySyncStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
use gpui::Context;

use super::SyncPage;

pub(super) struct CloudSnapshot {
    pub(super) account: AccountStatus,
    pub(super) storage_usage: Option<AccountStorageUsage>,
    pub(super) sync_statuses: Vec<LibrarySyncStatus>,
    pub(super) local_storage_usage: Vec<LocalLibraryStorageUsage>,
    /// `None` from a server without the per-library storage endpoint.
    pub(super) server_storage_usage: Option<Vec<ServerLibraryStorageUsage>>,
    pub(super) transfers: Vec<LibraryTransfers>,
}

pub(super) async fn load_cloud_snapshot(core: &AppBackend) -> Result<CloudSnapshot, String> {
    let library_list = core.reconcile_libraries().await.map_err(|error| error.to_string())?;
    let account = core.account_status().map_err(|error| error.to_string())?;
    let storage_usage = if account.signed_in() { core.account_storage_usage_if_supported().await.map_err(|error| error.to_string())? } else { None };
    let local_storage_usage = core.scan_local_library_storage_usage().await.map_err(|error| error.to_string())?;
    let server_storage_usage = if account.signed_in() { core.account_library_storage_usage_if_supported().await.map_err(|error| error.to_string())? } else { None };
    let transfers = core.transfer_statuses().map_err(|error| error.to_string())?;
    Ok(CloudSnapshot { account, storage_usage, sync_statuses: library_list.sync_statuses, local_storage_usage, server_storage_usage, transfers })
}

impl SyncPage {
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_with_feedback(true, cx);
    }

    pub(super) fn refresh_with_feedback(&mut self, show_feedback: bool, cx: &mut Context<Self>) {
        if self.busy {
            if !show_feedback {
                self.refresh_pending = true;
            }
            return;
        }
        self.busy = true;
        self.error = None;
        if show_feedback {
            self.message = Some("Refreshing…".into());
        }
        cx.notify();
        let operation = self.services.backend.spawn(|core| async move { load_cloud_snapshot(&core).await });
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| match result {
                Ok(snapshot) => page.apply_snapshot(snapshot, None, cx),
                Err(error) => page.apply_error(error, cx),
            });
        }));
    }

    fn apply_snapshot(&mut self, snapshot: CloudSnapshot, message: Option<&str>, cx: &mut Context<Self>) {
        self.account_status = snapshot.account;
        self.storage_usage = snapshot.storage_usage;
        self.sync_statuses = std::sync::Arc::new(snapshot.sync_statuses);
        self.local_storage_usage = std::sync::Arc::new(snapshot.local_storage_usage);
        self.server_storage_usage = snapshot.server_storage_usage.map(std::sync::Arc::new);
        self.transfers.groups = std::sync::Arc::new(snapshot.transfers);
        self.busy = false;
        self.account_action_busy = false;
        self.error = None;
        self.message = message.map(Into::into);
        let refresh_again = self.refresh_pending;
        self.refresh_pending = false;
        cx.notify();
        if refresh_again {
            self.schedule_refresh(cx);
        }
    }

    pub(super) fn apply_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.account_action_busy = false;
        self.message = None;
        self.error = Some(error.into());
        cx.notify();
    }

    pub(super) fn start_snapshot_operation<F, Fut>(&mut self, progress: &'static str, success: &'static str, reveal_password_reset_on_error: bool, operation: F, cx: &mut Context<Self>)
    where
        F: FnOnce(AppBackend) -> Fut,
        Fut: Future<Output = Result<CloudSnapshot, String>> + Send + 'static,
    {
        if self.busy {
            return;
        }
        self.busy = true;
        self.account_action_busy = true;
        if reveal_password_reset_on_error {
            self.login_failed = false;
        }
        self.error = None;
        self.message = Some(progress.into());
        cx.notify();
        let task = self.services.backend.spawn(operation);
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| match result {
                Ok(snapshot) => {
                    page.login_failed = false;
                    page.apply_snapshot(snapshot, Some(success), cx);
                }
                Err(error) => {
                    page.login_failed = reveal_password_reset_on_error;
                    page.apply_error(error, cx);
                }
            });
        }));
    }

    pub(super) fn start_logout(&mut self, cx: &mut Context<Self>) {
        self.start_snapshot_operation(
            "Signing out…",
            "Signed out",
            false,
            |core| async move {
                core.logout().await.map_err(|error| error.to_string())?;
                load_cloud_snapshot(&core).await
            },
            cx,
        );
    }
}

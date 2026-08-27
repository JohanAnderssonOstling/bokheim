//! Loads and applies the combined account, library, transfer, and session snapshot.

use gpui::Context;

use super::SyncPage;
use crate::services::backend::{UiFuture, app_async};
use bokheim_backend::{AppBackend, BackendError};

#[derive(Clone, Debug)]
pub(super) struct CloudSnapshot {
    account: bokheim_backend::AccountStatus,
    storage_usage: Option<bokheim_backend::AccountStorageUsage>,
    sync_statuses: Vec<bokheim_backend::LibrarySyncStatus>,
    local_storage_usage: Vec<bokheim_backend::LocalLibraryStorageUsage>,
    server_storage_usage: Option<Vec<bokheim_backend::ServerLibraryStorageUsage>>,
    transfers: Vec<bokheim_backend::LibraryTransfers>,
}

async fn load_snapshot(backend: &AppBackend) -> Result<CloudSnapshot, BackendError> {
    let library_list = backend.reconcile_libraries().await?;
    let account = backend.account_status()?;
    let storage_usage = if account.signed_in() { backend.account_storage_usage_if_supported().await? } else { None };
    let local_storage_usage = backend.scan_local_library_storage_usage().await?;
    let server_storage_usage = if account.signed_in() { backend.account_library_storage_usage_if_supported().await? } else { None };
    let transfers = backend.transfer_statuses()?;
    Ok(CloudSnapshot { account, storage_usage, sync_statuses: library_list.sync_statuses, local_storage_usage, server_storage_usage, transfers })
}

fn snapshot(backend: &AppBackend) -> UiFuture<CloudSnapshot> {
    app_async(backend, move |backend| async move { load_snapshot(&backend).await })
}

pub(super) fn login(backend: &AppBackend, email: String, password: String) -> UiFuture<CloudSnapshot> {
    app_async(backend, move |backend| async move {
        backend.login(&email, &password).await?;
        load_snapshot(&backend).await
    })
}

pub(super) fn verify_email(backend: &AppBackend, email: String, pin: String) -> UiFuture<CloudSnapshot> {
    app_async(backend, move |backend| async move {
        backend.verify_email(&email, &pin).await?;
        load_snapshot(&backend).await
    })
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
        let operation = snapshot(&self.services.backend);
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await;
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

    pub(super) fn start_snapshot_operation(&mut self, progress: &'static str, success: &'static str, reveal_password_reset_on_error: bool, operation: UiFuture<CloudSnapshot>, cx: &mut Context<Self>) {
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
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await;
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
        let operation = app_async(&self.services.backend, move |backend| async move {
            backend.logout().await?;
            load_snapshot(&backend).await
        });
        self.start_snapshot_operation("Signing out…", "Signed out", false, operation, cx);
    }

    pub(super) fn start_sync(&mut self, cx: &mut Context<Self>) {
        let operation = app_async(&self.services.backend, move |backend| async move {
            backend.sync_all_libraries().await?;
            load_snapshot(&backend).await
        });
        self.start_snapshot_operation("Syncing…", "All libraries synced", false, operation, cx);
    }
}

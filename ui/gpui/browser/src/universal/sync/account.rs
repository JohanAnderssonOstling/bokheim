//! Loads and applies the combined account, library, transfer, and session snapshot.

use std::future::Future;

use epub_svelte_lib::LibraryTransfers;
use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::app::{AccountSessionSummary, AccountStatus, AppBackend, LibrarySyncStatus};
use gpui::Context;

use super::{SyncPage, SyncPageEvent};

pub(super) struct CloudSnapshot {
    pub(super) account: AccountStatus,
    pub(super) libraries: Vec<LibraryEntry>,
    pub(super) sync_statuses: Vec<LibrarySyncStatus>,
    pub(super) transfers: Vec<LibraryTransfers>,
    pub(super) sessions: Vec<AccountSessionSummary>,
}

pub(super) async fn load_cloud_snapshot(core: &AppBackend) -> Result<CloudSnapshot, String> {
    let catalogue = core.reconcile_library_catalogue().await.map_err(|error| error.to_string())?;
    let account = core.account_status().map_err(|error| error.to_string())?;
    let transfers = core.transfer_statuses().map_err(|error| error.to_string())?;
    let sessions = if account.signed_in() { core.account_sessions().await.map_err(|error| error.to_string())? } else { Vec::new() };
    Ok(CloudSnapshot { account, libraries: catalogue.libraries, sync_statuses: catalogue.sync_statuses, transfers, sessions })
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
        self.sync_statuses = std::sync::Arc::new(snapshot.sync_statuses);
        self.transfers.groups = std::sync::Arc::new(snapshot.transfers);
        self.sessions.items = std::sync::Arc::new(snapshot.sessions);
        self.busy = false;
        self.error = None;
        self.message = message.map(Into::into);
        let refresh_again = self.refresh_pending;
        self.refresh_pending = false;
        cx.emit(SyncPageEvent::CatalogueReconciled(snapshot.libraries));
        cx.notify();
        if refresh_again {
            self.schedule_refresh(cx);
        }
    }

    pub(super) fn apply_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.message = None;
        self.error = Some(error.into());
        cx.notify();
    }

    pub(super) fn start_snapshot_operation<F, Fut>(&mut self, progress: &'static str, success: &'static str, operation: F, cx: &mut Context<Self>)
    where
        F: FnOnce(AppBackend) -> Fut,
        Fut: Future<Output = Result<CloudSnapshot, String>> + Send + 'static,
    {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.message = Some(progress.into());
        cx.notify();
        let task = self.services.backend.spawn(operation);
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| match result {
                Ok(snapshot) => page.apply_snapshot(snapshot, Some(success), cx),
                Err(error) => page.apply_error(error, cx),
            });
        }));
    }

    pub(super) fn start_logout(&mut self, cx: &mut Context<Self>) {
        self.start_snapshot_operation(
            "Signing out…",
            "Signed out",
            |core| async move {
                core.logout().await.map_err(|error| error.to_string())?;
                load_cloud_snapshot(&core).await
            },
            cx,
        );
    }
}

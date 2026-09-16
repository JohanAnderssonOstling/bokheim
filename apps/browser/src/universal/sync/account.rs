//! Loads and applies the combined account, library, transfer, and session snapshot.

use gpui::Context;

use super::SyncPage;
use crate::services::UiFuture;
use bokheim_backend::AppClient;

pub(super) type CloudSnapshot = bokheim_backend::AccountSnapshot;

fn snapshot(backend: &AppClient) -> UiFuture<CloudSnapshot> {
    let backend = backend.clone();
    Box::pin(async move { backend.account_snapshot().await })
}

pub(super) fn login(backend: &AppClient, email: String, password: String) -> UiFuture<CloudSnapshot> {
    let backend = backend.clone();
    Box::pin(async move { backend.login(email, password).await })
}

pub(super) fn verify_email(backend: &AppClient, email: String, pin: String) -> UiFuture<CloudSnapshot> {
    let backend = backend.clone();
    Box::pin(async move { backend.verify_email(email, pin).await })
}

impl SyncPage {
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_with_feedback(true, cx);
    }

    /// Coalesces bursts of account and synchronization notifications.
    pub(crate) fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_scheduled {
            return;
        }
        self.refresh_scheduled = true;
        let timer = cx.background_executor().timer(std::time::Duration::from_millis(200));
        cx.spawn(async move |page, cx| {
            timer.await;
            let _ = page.update(cx, |page, cx| {
                page.refresh_scheduled = false;
                page.refresh_with_feedback(false, cx);
            });
        })
        .detach();
    }

    pub(super) fn refresh_with_feedback(&mut self, show_feedback: bool, cx: &mut Context<Self>) {
        if self.is_busy() {
            if !show_feedback {
                self.refresh_pending = true;
            }
            return;
        }
        self.activity = super::AccountActivity::Refreshing;
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
        let next_account_status = snapshot.account;
        let identity_changed = self.account_status.signed_in() != next_account_status.signed_in() || self.account_status.email() != next_account_status.email();
        self.account_status = next_account_status;
        #[cfg(target_arch = "wasm32")]
        if self.account_status.signed_in() {
            self.web_authentication = None;
        }
        if identity_changed {
            let account_status = self.account_status.clone();
            self.identity.update(cx, |identity, cx| identity.set_account_status(account_status, cx));
            // Arriving at an account is what the form was for. A password change
            // does not get here — the identity is the same afterwards — so that
            // form closes when its own screen is done with.
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            if self.account_status.signed_in() {
                self.access_open = false;
            }
        }
        self.storage_usage = snapshot.storage_usage;
        self.sync_statuses = std::sync::Arc::new(snapshot.sync_statuses);
        self.local_storage_usage = std::sync::Arc::new(snapshot.local_storage_usage);
        self.server_storage_usage = snapshot.server_storage_usage.map(std::sync::Arc::new);
        self.activity = super::AccountActivity::Idle;
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
        self.activity = super::AccountActivity::Idle;
        self.message = None;
        self.error = Some(error.into());
        cx.notify();
    }

    pub(super) fn start_snapshot_operation(&mut self, progress: &'static str, success: &'static str, reveal_password_reset_on_error: bool, operation: UiFuture<CloudSnapshot>, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        self.activity = super::AccountActivity::AccountRequest;
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
        let backend = self.services.backend.clone();
        let operation = Box::pin(async move { backend.logout().await });
        self.start_snapshot_operation("Signing out…", "Signed out", false, operation, cx);
    }
}

impl SyncPage {
    pub(super) fn change_asset_storage(&mut self, library_id: sync_common::LibraryId, enabled: bool, cx: &mut Context<Self>) {
        if !self.asset_storage_pending.insert(library_id) {
            return;
        }
        let backend = self.services.backend.clone();
        cx.spawn(async move |page, cx| {
            let result = backend.set_library_asset_storage(library_id, enabled).await;
            let _ = page.update(cx, |page, cx| {
                page.asset_storage_pending.remove(&library_id);
                match result {
                    Ok(()) => {
                        for status in std::sync::Arc::make_mut(&mut page.sync_statuses).iter_mut().filter(|status| status.library_id == library_id) {
                            status.asset_storage_enabled = enabled;
                        }
                        page.schedule_refresh(cx);
                    }
                    Err(error) => page.error = Some(error.into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

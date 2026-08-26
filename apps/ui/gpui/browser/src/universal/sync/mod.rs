//! Account, library sync state and transfer queue.
//!
//! Universal in full: it holds account identity and aggregates *across* every
//! library, so it must outlive any one selected-library root. Reconciling the
//! library list is reported upward so the root can update the shared
//! [`NavigationState`] rather than this page reaching into the navigation UI tree.

mod account;
mod authentication;
mod transfers;
mod view;

use std::sync::Arc;

use app_contract::account::{AccountStatus, AccountStorageUsage, LibrarySyncState, LibrarySyncStatus, LibrarySyncSummary, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, IntoElement, SharedString, Subscription, Task, Window, px};
use gpui_component::IconName;
use gpui_component::alert::Alert;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::group_box::{GroupBox, GroupBoxVariants};
use gpui_component::input::{Input, InputState, OtpInput};
use gpui_component::label::Label;
use gpui_component::tag::Tag;
use gpui_component::{Sizable, Size};
use ui_components as components;

use crate::model::AccountFlow;
use crate::navigation::NavigationState;
use crate::services::AppServices;

use self::authentication::AuthenticationInputs;
use self::transfers::TransferState;

pub(crate) struct SyncPage {
    services: AppServices,
    active_library: Entity<NavigationState>,
    account_status: AccountStatus,
    storage_usage: Option<AccountStorageUsage>,
    sync_statuses: Arc<Vec<LibrarySyncStatus>>,
    local_storage_usage: Arc<Vec<LocalLibraryStorageUsage>>,
    /// `None` until a server that supports the per-library breakdown answers.
    server_storage_usage: Option<Arc<Vec<ServerLibraryStorageUsage>>>,
    transfers: TransferState,
    flow: AccountFlow,
    /// Whether the password field on the current flow is showing its text. One
    /// flag, because only one flow is on screen and each carries one password.
    password_revealed: bool,
    busy: bool,
    account_action_busy: bool,
    /// Password recovery is offered only after an actual sign-in request has
    /// failed, rather than as a permanent third authentication mode.
    login_failed: bool,
    message: Option<SharedString>,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    refresh_scheduled: bool,
    refresh_pending: bool,
    authentication: AuthenticationInputs,
    _sync_updates: Task<()>,
    _transfer_updates: Task<()>,
    _active_library_subscription: Subscription,
}

impl SyncPage {
    pub(crate) fn new(services: AppServices, active_library: Entity<NavigationState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let account_status = services.backend.account_status().unwrap_or_else(|error| {
            log::error!("failed to load account status: {error}");
            AccountStatus::default()
        });
        let authentication = AuthenticationInputs::new(window, cx);
        let active_library_subscription = cx.observe(&active_library, |_, _, cx| cx.notify());
        let sync_changes = services.backend.sync_changes();
        let sync_updates = cx.spawn(async move |page, cx| {
            while sync_changes.recv().await.is_ok() {
                let _ = page.update(cx, |page, cx| page.schedule_refresh(cx));
            }
        });
        let transfer_changes = services.backend.transfer_changes();
        let transfer_updates = cx.spawn(async move |page, cx| {
            while transfer_changes.recv().await.is_ok() {
                let _ = page.update(cx, |page, cx| page.schedule_refresh(cx));
            }
        });
        let mut page = Self {
            services,
            active_library,
            account_status,
            storage_usage: None,
            sync_statuses: Arc::new(Vec::new()),
            local_storage_usage: Arc::new(Vec::new()),
            server_storage_usage: None,
            transfers: TransferState::default(),
            flow: AccountFlow::SignIn,
            password_revealed: false,
            busy: false,
            account_action_busy: false,
            login_failed: false,
            message: None,
            error: None,
            task: None,
            refresh_scheduled: false,
            refresh_pending: false,
            authentication,
            _sync_updates: sync_updates,
            _transfer_updates: transfer_updates,
            _active_library_subscription: active_library_subscription,
        };
        page.refresh(cx);
        page
    }
}

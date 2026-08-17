//! Account, library sync state and transfer queue.
//!
//! Universal in full: it holds account identity and aggregates *across* every
//! library, so it must outlive any one selected-library root. Reconciling the
//! catalogue is reported upward so the root can update the shared
//! [`LibraryState`] rather than this page reaching into the library UI tree.

mod account;
mod authentication;
mod transfers;
mod view;

use std::sync::Arc;

use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::app::{AccountStatus, AccountStorageUsage, LibrarySyncState, LibrarySyncStatus, LibrarySyncSummary, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, EventEmitter, IntoElement, Render, SharedString, Task, Window, div, px};
use gpui_component::IconName;
use gpui_component::alert::Alert;
use gpui_component::form::{Field, Form};
use gpui_component::group_box::{GroupBox, GroupBoxVariants};
use gpui_component::input::{Input, OtpInput};
use gpui_component::label::Label;
use gpui_component::tag::Tag;
use gpui_component::{Sizable, Size};
use ui_components as components;

use crate::library::LibraryState;
use crate::model::AccountFlow;
use crate::services::AppServices;

use self::authentication::AuthenticationInputs;
use self::transfers::TransferState;

#[derive(Clone)]
pub(crate) enum SyncPageEvent {
    /// The library catalogue was re-read from the backend.
    CatalogueReconciled(Vec<LibraryEntry>),
}

pub(crate) struct SyncPage {
    services: AppServices,
    active_library: Entity<LibraryState>,
    account_status: AccountStatus,
    storage_usage: Option<AccountStorageUsage>,
    sync_statuses: Arc<Vec<LibrarySyncStatus>>,
    local_storage_usage: Arc<Vec<LocalLibraryStorageUsage>>,
    /// `None` until a server that supports the per-library breakdown answers.
    server_storage_usage: Option<Arc<Vec<ServerLibraryStorageUsage>>>,
    transfers: TransferState,
    flow: AccountFlow,
    /// Batch job history stays behind the activity footer until asked for: it
    /// is empty most of the time, and a section that says nothing pushes the
    /// libraries off the page.
    activity_expanded: bool,
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
}

impl EventEmitter<SyncPageEvent> for SyncPage {}

impl SyncPage {
    pub(crate) fn new(services: AppServices, active_library: Entity<LibraryState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let account_status = services.backend.account_status().unwrap_or_else(|error| {
            log::error!("failed to load account status: {error}");
            AccountStatus::default()
        });
        let authentication = AuthenticationInputs::new(window, cx);
        Self {
            services,
            active_library,
            account_status,
            storage_usage: None,
            sync_statuses: Arc::new(Vec::new()),
            local_storage_usage: Arc::new(Vec::new()),
            server_storage_usage: None,
            transfers: TransferState::default(),
            flow: AccountFlow::SignIn,
            activity_expanded: false,
            busy: false,
            account_action_busy: false,
            login_failed: false,
            message: None,
            error: None,
            task: None,
            refresh_scheduled: false,
            refresh_pending: false,
            authentication,
        }
    }
}

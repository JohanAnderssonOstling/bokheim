//! Account, sessions, library sync state and transfer queue.
//!
//! Universal in full: it holds account identity and aggregates *across* every
//! library, so it must outlive any one selected-library root. Reconciling the
//! catalogue is reported upward so the root can update the shared
//! [`LibraryState`] rather than this page reaching into the library UI tree.

mod account;
mod authentication;
mod sessions;
mod transfers;
mod view;

use std::sync::Arc;

use bokheim_ui_components as components;
use epub_svelte_lib::app::LibraryEntry;
use epub_svelte_lib::app::{AccountStatus, LibrarySyncState, LibrarySyncStatus};
use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, IntoElement, Render, SharedString, Task, Window};
use gpui_component::IconName;
use gpui_component::input::Input;

use crate::library::LibraryState;
use crate::model::AccountFlow;
use crate::services::AppServices;

use self::authentication::AuthenticationInputs;
use self::sessions::SessionsState;
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
    sync_statuses: Arc<Vec<LibrarySyncStatus>>,
    transfers: TransferState,
    sessions: SessionsState,
    flow: AccountFlow,
    busy: bool,
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
            sync_statuses: Arc::new(Vec::new()),
            transfers: TransferState::default(),
            sessions: SessionsState::default(),
            flow: AccountFlow::SignIn,
            busy: false,
            message: None,
            error: None,
            task: None,
            refresh_scheduled: false,
            refresh_pending: false,
            authentication,
        }
    }
}

//! Account, library sync state and transfer queue.
//!
//! Universal in full: it holds account identity and aggregates *across* every
//! library, so it must outlive any one selected-library root. Reconciling the
//! library list is reported upward so the root can update the shared
//! [`Libraries`] rather than this page reaching into the navigation UI tree.

mod account;
#[cfg(target_os = "android")]
mod android_authentication;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod authentication;
mod transfers;
mod view;
#[cfg(target_arch = "wasm32")]
mod web_authentication;

use std::sync::Arc;

use bokheim_backend::{AccountStatus, AccountStorageUsage, LibrarySyncState, LibrarySyncStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, IntoElement, Render, SharedString, Subscription, Task, Window, div, px};
use gpui_component::IconName;
use gpui_component::alert::Alert;
use gpui_component::button::ButtonVariants;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use gpui_component::input::{Input, InputState, OtpInput};
use gpui_component::tag::Tag;
use gpui_component::{Sizable, Size};
use ui_components as components;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use crate::model::AccountFlow;
use crate::navigation::Libraries;
use crate::services::AppServices;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use self::authentication::AuthenticationInputs;
use self::transfers::{ActivityState, LibraryActivity, LibraryActivitySummary};
pub(crate) use self::view::LibrariesView;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccountActivity {
    Idle,
    Refreshing,
    AccountRequest,
}

pub(crate) struct SyncPage {
    services: AppServices,
    active_library: Entity<Libraries>,
    account_status: AccountStatus,
    identity: Entity<view::AccountIdentityView>,
    storage_usage: Option<AccountStorageUsage>,
    sync_statuses: Arc<Vec<LibrarySyncStatus>>,
    asset_storage_pending: std::collections::HashSet<sync_common::LibraryId>,
    local_storage_usage: Arc<Vec<LocalLibraryStorageUsage>>,
    /// `None` until a server that supports the per-library breakdown answers.
    server_storage_usage: Option<Arc<Vec<ServerLibraryStorageUsage>>>,
    /// Where a new library is created. It belongs to the library list rather
    /// than to a storage group of its own: it is the setting that decides where
    /// the next row in that list will live. `None` until the backend answers,
    /// and on platforms that do not let the user choose.
    #[cfg(feature = "filesystem-libraries")]
    default_library_location: Option<String>,
    activity_jobs: Entity<LibraryActivity>,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    flow: AccountFlow,
    /// Whether the account form is showing. It is opened by a header button and
    /// by nothing else: signed out, the group is the two ways in until one of
    /// them is pressed.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    access_open: bool,
    /// Whether the password field on the current flow is showing its text. One
    /// flag, because only one flow is on screen and each carries one password.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    password_revealed: bool,
    activity: AccountActivity,
    /// Password recovery is offered only after an actual sign-in request has
    /// failed, rather than as a permanent third authentication mode.
    login_failed: bool,
    message: Option<SharedString>,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    refresh_scheduled: bool,
    refresh_pending: bool,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    authentication: AuthenticationInputs,
    #[cfg(target_arch = "wasm32")]
    web_authentication: Option<web_authentication::WebAuthentication>,
    #[cfg(target_os = "android")]
    android_authentication: Option<android_authentication::AndroidAuthentication>,
    _active_library_subscription: Subscription,
    _activity_subscription: Subscription,
}

impl SyncPage {
    fn is_busy(&self) -> bool {
        self.activity != AccountActivity::Idle
    }

    pub(crate) fn new(services: AppServices, active_library: Entity<Libraries>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let account_status = services.startup.account_status.clone();
        let page_entity = cx.entity();
        let identity = cx.new(|_| view::AccountIdentityView::new(page_entity, account_status.clone()));
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        let authentication = AuthenticationInputs::new(window, cx);
        let active_library_subscription = cx.observe(&active_library, |page, _, cx| {
            page.schedule_refresh(cx);
            cx.notify();
        });
        // The queue lives in an entity of its own so its own refreshes stay
        // cheap, but the library rows read from it, so this page redraws with
        // it. Only the account section is rebuilt: the settings controls around
        // it belong to the page above and are untouched.
        let activity_jobs = cx.new(|cx| LibraryActivity::new(services.backend.clone(), cx));
        let activity_subscription = cx.observe(&activity_jobs, |_, _, cx| cx.notify());
        let mut page = Self {
            services,
            active_library,
            account_status,
            identity,
            storage_usage: None,
            sync_statuses: Arc::new(Vec::new()),
            asset_storage_pending: std::collections::HashSet::new(),
            local_storage_usage: Arc::new(Vec::new()),
            server_storage_usage: None,
            #[cfg(feature = "filesystem-libraries")]
            default_library_location: None,
            activity_jobs,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            flow: AccountFlow::SignIn,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            access_open: false,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            password_revealed: false,
            activity: AccountActivity::Idle,
            login_failed: false,
            message: None,
            error: None,
            task: None,
            refresh_scheduled: false,
            refresh_pending: false,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            authentication,
            #[cfg(target_arch = "wasm32")]
            web_authentication: None,
            #[cfg(target_os = "android")]
            android_authentication: None,
            _active_library_subscription: active_library_subscription,
            _activity_subscription: activity_subscription,
        };
        page.refresh(cx);
        #[cfg(feature = "filesystem-libraries")]
        {
            let backend = page.services.backend.clone();
            cx.spawn(async move |page, cx| {
                let result = backend.default_library_save_location().await;
                let _ = page.update(cx, |page, cx| {
                    match result {
                        Ok(path) => page.default_library_location = path,
                        Err(error) => page.error = Some(error.into()),
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        page
    }
}

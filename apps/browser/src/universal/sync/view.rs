//! Sync and account presentation, rendered as the Account and Libraries
//! sections of Settings.
//!
//! The account leads with who is signed in, then the quota as a shape with
//! what each library holds of it; the libraries follow as a table sorted by
//! what they occupy.
//!
//! What the device is uploading and downloading is reported on the library it
//! belongs to rather than in a queue of its own: a job is only ever interesting
//! as something *a library* is doing, and a list beneath the libraries made the
//! reader match names between two lists to find that out.

use ui_text::text;

use super::*;
use crate::services::format_storage_bytes;
use gpui::ScrollHandle;
use gpui_component::Disableable;
use gpui_component::checkbox::Checkbox;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use sync_common::{MutationIssue, MutationRejectionReason};
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use ui_components::Button;

const LEGEND_LIBRARY_LIMIT: usize = 4;

/// The single character the identity avatar carries. Uppercased from the
/// address because that is the only name the account has.
fn account_initial(email: &str) -> String {
    email.chars().next().map(|first| first.to_uppercase().to_string()).unwrap_or_else(|| "?".to_owned())
}

/// The identity row has its own render boundary. Sync, storage, and transfer
/// updates can refresh the page around it without making the row flash.
pub(super) struct AccountIdentityView {
    page: Entity<SyncPage>,
    account_status: AccountStatus,
}

impl AccountIdentityView {
    pub(super) fn new(page: Entity<SyncPage>, account_status: AccountStatus) -> Self {
        Self { page, account_status }
    }

    pub(super) fn set_account_status(&mut self, account_status: AccountStatus, cx: &mut Context<Self>) -> bool {
        if self.account_status == account_status {
            return false;
        }
        self.account_status = account_status;
        cx.notify();
        true
    }
}

impl Render for AccountIdentityView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let email = self.account_status.email().unwrap_or_default().to_owned();
        let mut actions = Vec::with_capacity(3);
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            let password = self.page.clone();
            actions.push(components::link_button("account-change-password", text!(account::change_password), false).on_click(move |_, _, cx| {
                password.update(cx, |page, cx| page.open_password_change(cx));
            }));
        }
        let logout = self.page.clone();
        actions.push(components::link_button("account-log-out", text!(account::log_out), false).on_click(move |_, _, cx| {
            logout.update(cx, |page, cx| page.start_logout(cx));
        }));
        let delete = self.page.clone();
        actions.push(components::link_button("account-delete", text!(account::delete_account), false).on_click(move |_, window, cx| {
            delete.update(cx, |page, cx| page.open_account_deletion(window, cx));
        }));
        components::account_identity(account_initial(&email), email, actions, theme)
    }
}

/// The libraries, as a view of their own.
///
/// Settings draws Account, then this, as two sections — so the two halves of
/// what [`SyncPage`] knows are rendered in two places. A view of its own is
/// also the right boundary for the content: transfer progress reaches these
/// rows several times a second, and nothing above them should be rebuilt at
/// that rate.
pub(crate) struct LibrariesView {
    page: Entity<SyncPage>,
    compact: bool,
    _updates: gpui::Subscription,
}

impl LibrariesView {
    pub(crate) fn new(page: Entity<SyncPage>, cx: &mut Context<Self>) -> Self {
        let updates = cx.observe(&page, |_, _, cx| cx.notify());
        Self { page, compact: true, _updates: updates }
    }
}

impl Render for LibrariesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let compact = self.compact;
        let page = self.page.clone();
        let rows = page.read(cx).library_rows(cx);
        let view = cx.weak_entity();
        div().w_full().min_w_0().child(SyncPage::libraries_section(page.read(cx), &page, &rows, compact, theme, cx)).on_children_prepainted(move |children, window, cx| {
            let Some(bounds) = children.first() else { return };
            let next_compact = bounds.size.width < components::library_table_min_width(window);
            if next_compact != compact {
                let view = view.clone();
                window.defer(cx, move |_, cx| {
                    let _ = view.update(cx, |view, cx| {
                        if view.compact != next_compact {
                            view.compact = next_compact;
                            cx.notify();
                        }
                    });
                });
            }
        })
    }
}

/// One library as the page shows it: the sync status joined to the two
/// quantities the page reports for it.
///
/// `server_bytes` is what the account is charged for and is what the quota
/// meter is built from; `local_bytes` is what has been downloaded here. They
/// answer different questions and are never mixed.
struct LibraryRow {
    #[cfg(all(feature = "filesystem-libraries", any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    local_path: Option<String>,
    name: String,
    status: LibrarySyncStatus,
    server_bytes: Option<u64>,
    local_bytes: Option<u64>,
}

impl LibraryRow {
    /// Server bytes rank the list once the server can report them; until then
    /// the local sizes are the only sizes there are.
    const fn sort_bytes(&self) -> u64 {
        match self.server_bytes {
            Some(bytes) => bytes,
            None => match self.local_bytes {
                Some(bytes) => bytes,
                None => 0,
            },
        }
    }
}

impl SyncPage {
    /// Joins the device's libraries to their sync status and local size.
    ///
    /// Libraries the backend has no status for are given the status they would
    /// have, so the verdict above the list is computed from exactly the rows the
    /// list renders rather than from a different set.
    fn library_rows(&self, cx: &gpui::App) -> Vec<LibraryRow> {
        let state = self.state.read(cx);
        // Signed out there is no verdict to wait for, so a library the backend
        // has no status for is what it plainly is — local — rather than a row
        // that says it is checking something that will never be checked.
        let unknown_state = if state.account_status.signed_in() { LibrarySyncState::Checking } else { LibrarySyncState::LocalOnly };
        let mut rows: Vec<LibraryRow> = self
            .active_library
            .read(cx)
            .entries()
            .iter()
            .map(|library| {
                let status = state.sync_statuses.iter().find(|status| status.library_id == *library.library_id()).cloned().unwrap_or_else(|| LibrarySyncStatus::placeholder(*library.library_id(), unknown_state));
                let local_bytes = state.local_storage_usage.iter().find(|usage| usage.library_id == *library.library_id()).map(|usage| usage.used_bytes);
                // A supported server that lists no share for a library is
                // reporting zero; an unsupported server reports nothing at all.
                let server_bytes = state.server_storage_usage.as_ref().map(|usage| usage.iter().find(|entry| entry.library_id == *library.library_id()).map(|entry| entry.used_bytes).unwrap_or_default());
                LibraryRow {
                    name: library.library_name().to_owned(),
                    status,
                    server_bytes,
                    local_bytes,
                    #[cfg(all(feature = "filesystem-libraries", any(target_os = "linux", target_os = "macos", target_os = "windows")))]
                    local_path: Some(library.storage_locator().to_owned()),
                }
            })
            .collect();
        // Sorted by the quantity the page leads with, falling back to the local
        // sizes while no server breakdown is available.
        rows.sort_by(|left, right| right.sort_bytes().cmp(&left.sort_bytes()).then_with(|| left.name.cmp(&right.name)));
        rows
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn access_action(&self, entity: &Entity<Self>, id: &'static str, label: &'static str, icon: IconName, action: fn(&mut Self, &mut Context<Self>)) -> Button {
        let page = entity.clone();
        components::primary_action_button(id, label, icon, self.is_busy()).on_click(move |_, _, cx| page.update(cx, action))
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn access_link(&self, entity: &Entity<Self>, id: &'static str, label: &'static str, flow: AccountFlow) -> Button {
        let page = entity.clone();
        components::link_button(id, label, self.is_busy()).on_click(move |_, _, cx| page.update(cx, |page, cx| page.set_flow(flow, cx)))
    }

    /// The sign-in, registration, verification and password-reset forms.
    ///
    /// Only reachable while signed out, so it is kept apart from the page the
    /// account renders once there is an account to report on.
    ///
    /// Every flow has the same shape: what this screen is, the inputs it needs,
    /// one action that goes forward, and — quieter — the way to another flow.
    /// The inputs carry their own placeholders and are never labelled as well.
    ///
    /// The heading is the whole of the explanation. Text under it survives only
    /// where it says something the screen cannot show — a minimum length, or
    /// that the reset token is not the PIN.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn account_access_form(&self, entity: &Entity<Self>, feedback: Option<gpui::AnyElement>, theme: components::BrowserTheme) -> Div {
        let form = components::access_form();
        match self.flow {
            AccountFlow::SignIn => {
                let register = entity.clone();
                let mut actions = components::action_row().child(self.access_action(entity, "account-sign-in", text!(common::sign_in), IconName::Globe, Self::start_login));
                actions = actions.child(components::secondary_action_button("account-register", text!(account::create_account), IconName::User, self.is_busy()).on_click(move |_, _, cx| {
                    register.update(cx, |page, cx| page.set_flow(AccountFlow::Register, cx));
                }));
                form.child(components::section_header(text!(common::sign_in), "", theme))
                    .children(feedback)
                    .child(Input::new(&self.authentication.login_email))
                    .child(Input::new(&self.authentication.password).mask_toggle())
                    .child(actions)
                    .child(self.access_link(entity, "account-request-reset", text!(account::forgot_your_password), AccountFlow::RequestPasswordReset))
            }
            AccountFlow::Register => form
                .child(components::section_header(text!(account::create_account), "", theme))
                .children(feedback)
                .child(Input::new(&self.authentication.email))
                .child(Input::new(&self.authentication.registration_password).mask_toggle())
                .child(components::form_hint(format!(text!(account::at_least_characters), account_contract::PASSWORD_MIN_CHARACTERS), theme))
                .child(components::action_row().child(self.access_action(entity, "account-register", text!(account::create_account), IconName::User, Self::start_registration)))
                .child(self.access_link(entity, "account-sign-in", text!(account::already_have_an_account_sign_in), AccountFlow::SignIn)),
            AccountFlow::VerifyEmail => {
                let resend = entity.clone();
                let mut actions = components::action_row().child(self.access_action(entity, "account-verify", text!(account::verify_email), IconName::ArrowRight, Self::start_verify_email));
                let seconds = self.authentication.resend_seconds_remaining;
                let resend_label = if seconds > 0 { format!(text!(account::resend_pin_s), seconds = seconds) } else { text!(account::resend_pin).to_owned() };
                actions = actions.child(components::secondary_action_button("account-resend", resend_label, IconName::Redo2, self.is_busy() || seconds > 0).on_click(move |_, _, cx| {
                    resend.update(cx, |page, cx| page.start_resend_verification(cx));
                }));
                // The existing signup inputs supply the email and password.
                form.child(components::section_header(text!(account::check_your_email), "", theme)).children(feedback).child(OtpInput::new(&self.authentication.verification_pin)).child(actions).child(self.access_link(
                    entity,
                    "account-restart-registration",
                    text!(account::wrong_address_start_over),
                    AccountFlow::Register,
                ))
            }
            AccountFlow::RequestPasswordReset => form
                .child(components::section_header(text!(account::reset_your_password), "", theme))
                .children(feedback)
                .child(Input::new(&self.authentication.email))
                .child(components::action_row().child(self.access_action(entity, "account-request-reset", text!(account::email_me_a_reset_code), IconName::Redo2, Self::start_password_reset_request)))
                .child(self.access_link(entity, "account-sign-in", text!(account::back_to_sign_in), AccountFlow::SignIn)),
            AccountFlow::ResetPassword => {
                // The token is a pasted string, not the six-digit PIN, so it is
                // a plain input rather than the OTP boxes — drawing them alike
                // invites people to type six of its characters and give up.
                form.child(components::section_header(text!(account::set_a_new_password), text!(account::paste_the_full_reset_code_from_your_email_this_is_different_from), theme))
                    .children(feedback)
                    .child(Input::new(&self.authentication.reset_token))
                    .child(Input::new(&self.authentication.new_password).mask_toggle())
                    .child(components::form_hint(format!(text!(account::at_least_characters), account_contract::PASSWORD_MIN_CHARACTERS), theme))
                    .child(components::action_row().child(self.access_action(entity, "account-reset-password", text!(account::set_new_password), IconName::Redo2, Self::start_password_reset)))
                    .child(self.access_link(entity, "account-sign-in", text!(account::back_to_sign_in), AccountFlow::SignIn))
            }
        }
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    fn account_access_form(&self, entity: &Entity<Self>, feedback: Option<gpui::AnyElement>, theme: components::BrowserTheme) -> Div {
        let page = entity.clone();
        components::access_form().child(components::section_header(text!(account::bokheim_account), text!(account::sign_in_to_sync_your_library_across_devices), theme)).children(feedback).child(components::action_row().child(
            components::primary_action_button("account-sign-in", text!(account::sign_in_or_create_account), IconName::Globe, self.is_busy()).on_click(move |_, _, cx| {
                #[cfg(target_arch = "wasm32")]
                page.update(cx, |page, cx| page.open_web_authentication(cx));
                #[cfg(target_os = "android")]
                page.update(cx, |page, cx| page.open_android_authentication(cx));
            }),
        ))
    }

    /// Show the account's largest library shares, group smaller libraries, and
    /// include space reserved by uploads. Sizes come from the cloud account.
    fn storage_segments(&self, usage: StorageUsageResponse, theme: components::BrowserTheme, state: &SyncState) -> Vec<(SharedString, u64, gpui::Hsla)> {
        let mut segments: Vec<(SharedString, u64, gpui::Hsla)> = Vec::new();
        let mut other_bytes = 0_u64;
        for (index, (name, bytes)) in state.server_storage_usage.as_ref().map(|usage| usage.iter()).into_iter().flatten().filter(|library| library.used_bytes > 0).map(|library| (library.library_name.clone(), library.used_bytes)).enumerate()
        {
            if index < LEGEND_LIBRARY_LIMIT {
                segments.push((name.into(), bytes, components::usage_segment_color(index, theme)));
            } else {
                other_bytes += bytes;
            }
        }
        if other_bytes > 0 {
            segments.push((text!(account::other_libraries).into(), other_bytes, components::usage_segment_color(LEGEND_LIBRARY_LIMIT, theme)));
        }
        if usage.reserved_bytes > 0 {
            segments.push((text!(account::reserved_by_uploads).into(), usage.reserved_bytes, theme.warning));
        }
        segments
    }
}

impl SyncPage {
    /// The account group: who is signed in, and what that account holds.
    ///
    /// Cloud storage used to be a group of its own, one gap below the identity
    /// that owns it. A quota is not a second subject: it is the answer to "what
    /// does this account get me", so it sits under the address it belongs to.
    /// Signed out there is no quota, and the group is its header, with the two
    /// ways in beside the title.
    pub(crate) fn settings_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        #[cfg(feature = "kobo")]
        self.update_account_overlay_focus(window, cx);
        let theme = components::browser_theme(cx);
        let entity = cx.entity();
        // Whichever of the two the last attempt produced. They are never both
        // set, and one slot means a form cannot grow two lines of chrome.
        let feedback = match (&self.error, &self.message) {
            (Some(error), _) => Some(Alert::error("account-error", error.clone()).with_size(Size::Small).into_any_element()),
            (None, Some(message)) => Some(Alert::new("account-message", message.clone()).with_size(Size::Small).into_any_element()),
            (None, None) => None,
        };

        let signed_in = self.state.read(cx).account_status.signed_in();
        if signed_in && let Some(form) = self.deletion.as_ref() {
            return components::browser_settings_fieldset(text!(account::account), theme).child(components::browser_settings_block().child(self.account_deletion_form(form, &entity, feedback, theme)));
        }
        let account = if signed_in {
            components::browser_settings_fieldset(text!(account::account), theme).child(components::browser_settings_block().child(self.identity.clone()).child(self.storage_body(theme, window, cx)))
        } else {
            components::browser_settings_fieldset_with_actions(text!(account::account), self.account_access_actions(&entity, theme), theme)
        };

        // The form is not the group's resting state on any platform any more:
        // it appears because a header button asked for it, and it is the only
        // thing in the group while it is open.
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        if self.access_open {
            // Every form here can be abandoned: the header opened it, and until
            // it is finished with there is no other way back to the account the
            // group is about.
            let close = entity.clone();
            let form = self.account_access_form(&entity, feedback, theme).child(components::link_button("account-close", text!(common::close), self.is_busy()).on_click(move |_, _, cx| {
                close.update(cx, |page, cx| page.close_account_access(cx));
            }));
            #[cfg(feature = "kobo")]
            return account.child(self.account_overlay(form, window, cx));
            #[cfg(not(feature = "kobo"))]
            return account.child(components::browser_settings_block().child(form));
        }

        account.children(feedback.map(|feedback| components::browser_settings_block().child(feedback)))
    }

    /// The quota as a line and a shape, and what each library holds of it.
    fn storage_body(&self, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let state = self.state.read(cx);
        let Some(usage) = state.storage_usage else {
            return components::storage_block().child(text!(account::loading_cloud_storage));
        };
        let mut storage = components::storage_block().child(components::storage_summary(
            format_storage_bytes(usage.used_bytes),
            format!(text!(account::of_used), format_storage_bytes(usage.quota_bytes)),
            format!(text!(account::available), format_storage_bytes(usage.available_bytes())),
            theme,
        ));
        if let Some(pending) = state.pending_cloud_bytes.filter(|bytes| *bytes > 0) {
            storage = storage.child(components::form_hint(format!(text!(account::waiting_to_upload), format_storage_bytes(pending)), theme));
        }
        if usage.quota_bytes > 0 {
            let segments = self.storage_segments(usage, theme, state);
            let share = |bytes: u64| bytes as f32 / usage.quota_bytes as f32;
            // Five greys and the warning cannot tell libraries apart on e-ink;
            // there the breakdown's figures stand alone.
            if !cfg!(feature = "kobo") {
                storage = storage.child(components::storage_meter(segments.iter().map(|(_, bytes, color)| (share(*bytes), *color)), theme));
            }
        }
        let tabs = components::settings_choice_button_group(theme)
            .self_start()
            .flex_nowrap()
            .child(components::settings_choice_button("account-storage-libraries-tab", text!(account::largest_libraries), self.storage_tab == StorageBreakdownTab::Libraries, theme).flex_none().whitespace_nowrap().on_click(cx.listener(
                |page, _, _, cx| {
                    page.storage_tab = StorageBreakdownTab::Libraries;
                    cx.notify();
                },
            )))
            .child(components::settings_choice_button("account-storage-books-tab", text!(account::largest_books), self.storage_tab == StorageBreakdownTab::Books, theme).flex_none().whitespace_nowrap().on_click(cx.listener(
                |page, _, _, cx| {
                    page.storage_tab = StorageBreakdownTab::Books;
                    cx.notify();
                },
            )));
        let mut breakdown = components::storage_breakdown(std::iter::empty());
        match self.storage_tab {
            StorageBreakdownTab::Libraries => match &state.server_storage_usage {
                None => breakdown = breakdown.child(components::form_hint(text!(account::library_storage_details_are_unavailable), theme)),
                Some(libraries) => {
                    let mut has_rows = false;
                    for (index, library) in libraries.iter().filter(|library| library.used_bytes > 0).enumerate() {
                        has_rows = true;
                        let share = if usage.quota_bytes == 0 { 0.0 } else { library.used_bytes as f64 * 100.0 / usage.quota_bytes as f64 };
                        breakdown = breakdown.child(components::storage_breakdown_row(
                            components::usage_segment_color(index.min(LEGEND_LIBRARY_LIMIT), theme),
                            library.library_name.clone(),
                            format!(text!(account::share_percent), share = share),
                            format_storage_bytes(library.used_bytes),
                            theme,
                        ));
                    }
                    if !has_rows {
                        breakdown = breakdown.child(components::form_hint(text!(account::no_libraries_use_cloud_storage_yet), theme));
                    }
                }
            },
            StorageBreakdownTab::Books => match &state.server_book_storage_usage {
                None => breakdown = breakdown.child(components::form_hint(text!(account::book_storage_details_are_unavailable_try_refreshing_your_account), theme)),
                Some(books) if books.is_empty() => breakdown = breakdown.child(components::form_hint(text!(account::no_books_use_cloud_storage_yet), theme)),
                Some(books) => {
                    for book in books {
                        breakdown = breakdown.child(components::storage_book_row(book.title.clone(), format_storage_bytes(book.used_bytes), theme));
                    }
                    if books.len() == account_contract::MAX_ACCOUNT_STORAGE_BOOKS {
                        breakdown = breakdown.child(components::form_hint(format!(text!(account::showing_the_largest_books), account_contract::MAX_ACCOUNT_STORAGE_BOOKS), theme));
                    }
                }
            },
        }
        let (list_id, scrollbar_id) = match self.storage_tab {
            StorageBreakdownTab::Libraries => ("account-storage-libraries", "account-storage-libraries-scrollbar"),
            StorageBreakdownTab::Books => ("account-storage-books", "account-storage-books-scrollbar"),
        };
        let scroll = window.use_keyed_state(list_id, cx, |_, _| ScrollHandle::default()).read(cx).clone();
        let desktop = !components::touch_ui();
        let list = div()
            .relative()
            .w_full()
            .overflow_hidden()
            // Hidden overflow keeps the scroll handle usable by the desktop
            // scrollbar without consuming wheel input from the settings page.
            .child(breakdown.id(list_id).max_h(px(280.0)).overflow_hidden().track_scroll(&scroll).when(!desktop, |list| list.overflow_y_scroll()).when(desktop, |list| list.pr(px(components::SPACE_MD))))
            .when(desktop, |list| list.child(div().absolute().inset_0().child(Scrollbar::vertical(&scroll).id(scrollbar_id).scrollbar_show(ScrollbarShow::Always))));
        storage.child(tabs).child(list)
    }

    /// The account group's header control while signed out: the two ways in.
    ///
    /// Signing in leads with `Log in` rather than with `Create account`: a
    /// person who opens Settings and finds themselves signed out is far more
    /// often someone whose session lapsed than someone who has never had one.
    fn account_access_actions(&self, entity: &Entity<Self>, theme: components::BrowserTheme) -> Div {
        let register = entity.clone();
        let login = entity.clone();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(components::SPACE_SM))
            .child(components::browser_settings_header_button("account-open-register", text!(account::create_account), false, theme).on_click(move |_, _, cx| {
                register.update(cx, |page, cx| page.open_account_access(true, cx));
            }))
            .child(components::browser_settings_header_button("account-open-sign-in", text!(account::log_in), true, theme).on_click(move |_, _, cx| {
                login.update(cx, |page, cx| page.open_account_access(false, cx));
            }))
    }

    /// Opens the account form, floating above Settings on Kobo.
    ///
    /// On the platforms that authenticate through a browser or the system
    /// account picker there is no form to reveal, so the same press opens that
    /// instead — the header reads the same everywhere, and what it opens is the
    /// platform's business.
    pub(super) fn open_account_access(&mut self, register: bool, cx: &mut Context<Self>) {
        let _ = register;
        #[cfg(target_arch = "wasm32")]
        self.open_web_authentication(cx);
        #[cfg(target_os = "android")]
        self.open_android_authentication(cx);
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            self.access_open = true;
            self.set_flow(if register { AccountFlow::Register } else { AccountFlow::SignIn }, cx);
        }
        cx.notify();
    }

    /// Opens the password-reset flow for someone who is already signed in.
    ///
    /// There is no separate "change password while signed in" exchange — the
    /// reset is the only one the server offers — so this is that flow, reached
    /// from the account menu instead of from a failed sign-in.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub(super) fn open_password_change(&mut self, cx: &mut Context<Self>) {
        self.access_open = true;
        self.set_flow(AccountFlow::RequestPasswordReset, cx);
        cx.notify();
    }

    /// Closes the form and returns the group to reporting the account.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub(super) fn close_account_access(&mut self, cx: &mut Context<Self>) {
        self.access_open = false;
        self.message = None;
        self.error = None;
        cx.notify();
    }

    /// The libraries this device holds: what each occupies, what it is doing,
    /// and where it stands with the account.
    ///
    /// The section opens with where a new library is put, because that setting
    /// is about this list — it decides where the next row in it will live — and
    /// a "Library storage" group of its own separated it from the only thing it
    /// refers to.
    fn libraries_section(&self, page: &Entity<Self>, rows: &[LibraryRow], compact: bool, theme: components::BrowserTheme, cx: &gpui::App) -> Div {
        let mut libraries = components::browser_settings_fieldset(text!(common::libraries), theme);
        let mut has_rows = false;

        #[cfg(all(feature = "filesystem-libraries", not(target_arch = "wasm32")))]
        if let Some(path) = self.default_library_location.clone() {
            let page = page.clone();
            let choose = components::browser_settings_path_button("default-library-location", path, theme).on_click(move |_, _, cx| page.update(cx, |page, cx| page.choose_library_location(cx)));
            libraries = libraries.child(components::browser_settings_compact_row(text!(account::default_location_for_new_libraries), choose, theme).border_t_0());
            has_rows = true;
        }

        if rows.is_empty() {
            return libraries.child(components::library_usage_empty(text!(account::no_local_libraries), theme).when(!has_rows, |row| row.border_t_0()));
        }
        if !compact {
            let header = components::library_table_header(theme);
            let header = header.child(div().w(gpui::rems(components::topbar_action_height_rem())).flex_none());
            libraries = libraries.child(header.when(!has_rows, |row| row.border_t_0()));
            has_rows = true;
        }
        for row in rows {
            let local_bytes = row.local_bytes.map(format_storage_bytes).unwrap_or_else(|| if row.status.state == LibrarySyncState::Unavailable { text!(common::unavailable).to_owned() } else { text!(account::calculating).to_owned() });
            let (status, status_color) = if row.status.issues.rejected != 0 {
                (text!(account::some_changes_were_rejected), theme.warning)
            } else if row.status.issues.untransmittable != 0 {
                (text!(account::some_changes_are_too_large_to_sync), theme.warning)
            } else if row.status.issues.retryable != 0 {
                (text!(account::some_changes_are_pending), theme.warning)
            } else {
                match row.status.state {
                    LibrarySyncState::Checking => (text!(account::loading_status), theme.text_muted),
                    LibrarySyncState::Unavailable => (text!(common::unavailable), theme.warning),
                    LibrarySyncState::Receiving => (text!(account::receiving_library), theme.accent),
                    LibrarySyncState::Synced => (text!(account::up_to_date), theme.accent),
                    LibrarySyncState::SyncPending if row.status.last_synced_at.is_none() => (text!(account::sync_pending), theme.warning),
                    LibrarySyncState::SyncPending => (text!(account::changes_pending), theme.warning),
                    LibrarySyncState::RemoteMissing => (text!(account::not_on_server), theme.warning),
                    LibrarySyncState::AccountRequired => (text!(account::sign_in_to_library_account), theme.warning),
                    LibrarySyncState::CloudStorageFull => (text!(account::cloud_storage_full_books_on_disk_were_not_added), theme.warning),
                    LibrarySyncState::LocalOnly => (text!(account::on_this_device), theme.text_muted),
                }
            };
            let (status, status_color) = (status.to_owned(), status_color);
            let library_id = row.status.library_id;
            // The folder is the button that opens it, labelled with its path.
            #[cfg(all(feature = "filesystem-libraries", any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            let path = row.local_path.as_ref().map(|path| {
                let destination = std::path::PathBuf::from(path);
                components::browser_settings_path_button(SharedString::from(format!("library-path-{library_id}")), path.clone(), theme)
                    .max_w_full()
                    .overflow_hidden()
                    .on_click(move |_, _, cx| cx.open_with_system(&destination))
                    .into_any_element()
            });
            #[cfg(not(all(feature = "filesystem-libraries", any(target_os = "linux", target_os = "macos", target_os = "windows"))))]
            let path: Option<gpui::AnyElement> = None;
            // The column heading names the switch; stacked, with no heading,
            // the switch names itself.
            let cloud_page = page.clone();
            let cloud_blocker = self.state.read(cx).library_cloud_storage_blocker(library_id);
            let cloud = {
                Checkbox::new(SharedString::from(format!("asset-storage-{library_id}")))
                    .when(compact, |checkbox| checkbox.label(text!(account::cloud_books)))
                    .checked(row.status.asset_storage_enabled)
                    .disabled(row.status.book_file_activity.blocks_file_changes() || self.asset_storage_pending.contains(&library_id) || (!row.status.asset_storage_enabled && cloud_blocker.is_some()))
                    .on_click(move |enabled, _, cx| cloud_page.update(cx, |page, cx| page.change_asset_storage(library_id, *enabled, cx)))
                    .into_any_element()
            };
            let cloud = gpui::div()
                .flex()
                .flex_col()
                .gap(gpui::px(components::SPACE_XS))
                .child(cloud)
                .children((self.state.read(cx).account_status.signed_in() && !row.status.asset_storage_enabled).then_some(cloud_blocker).flatten().map(|message| components::form_hint(message, theme)))
                .into_any_element();
            let library_row = components::library_table_row(row.name.clone(), path, local_bytes, (status, status_color), cloud, compact, theme).when(!has_rows, |row| row.border_t_0());
            let library_row = {
                let libraries = self.active_library.clone();
                let backend = libraries.read(cx).backend_client();
                let name = row.name.clone();
                let cloud_enabled = row.status.asset_storage_enabled;
                let files_busy = row.status.book_file_activity.blocks_file_changes();
                let has_actions = cloud_enabled || libraries.read(cx).annotation_export_available();
                library_row.when(has_actions, |library_row| {
                    let more = components::library_switcher_menu_button(format!("settings-library-menu-{library_id}"), row.name.clone(), false, theme).dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| {
                        let menu = crate::universal::library_book_menu(menu, libraries.clone(), backend.clone(), library_id, name.clone(), cloud_enabled, files_busy).touch_friendly(compact || components::touch_ui());
                        Libraries::annotation_export_menu(libraries.clone(), library_id, menu, cx)
                    });
                    library_row.child(more)
                })
            };
            libraries = libraries.child(library_row);
            for detail in &row.status.issue_details {
                let kind = match detail.kind.as_str() {
                    "reading_position" => text!(account::reading_position),
                    "chapter_read_status" => text!(account::chapter_read_status),
                    "annotation" => text!(common::annotation),
                    "directory_name" => text!(common::folder_name),
                    "directory_parent" => text!(account::folder_location),
                    "directory_lifecycle" => text!(account::folder_addition_or_removal),
                    "placement" => text!(account::book_location),
                    "book_lifecycle" => text!(account::book_addition_or_removal),
                    "metadata" | "enrichment" | "book_facts" | "description" => text!(account::book_details),
                    "book_toc" | "pdf_reader_metadata" => text!(account::book_navigation),
                    _ => text!(account::library_change),
                };
                let subject = detail.item_name.as_deref().filter(|name| !name.trim().is_empty()).map(|name| format!(text!(account::change_subject), kind = kind, name = name)).unwrap_or_else(|| kind.to_owned());
                let recovery = match &detail.issue {
                    MutationIssue::Retryable(rejection) | MutationIssue::Rejected(rejection) => match rejection.reason {
                        MutationRejectionReason::TimestampTooFarFuture => text!(account::this_change_is_too_far_ahead_of_the_server_s_clock_check),
                        MutationRejectionReason::InvalidPayload => text!(account::inconsistent_book_information_prevented_syncing_report_the_problem),
                    },
                    MutationIssue::Untransmittable(_) => text!(account::this_change_is_too_large_to_sync_report_the_problem),
                };
                libraries = libraries.child(components::form_hint(format!(text!(account::change_recovery), recovery = recovery, subject = subject), theme));
            }
            let issue_count = row.status.issues.rejected.saturating_add(row.status.issues.retryable).saturating_add(row.status.issues.untransmittable);
            let remaining = issue_count.saturating_sub(row.status.issue_details.len() as u64);
            if remaining > 0 {
                libraries = libraries.child(components::form_hint(format!(text!(account::more_changes_need_attention), remaining = remaining), theme));
            }
            has_rows = true;
        }
        libraries
    }

    /// Chooses the folder new libraries are created in.
    #[cfg(all(feature = "filesystem-libraries", not(target_arch = "wasm32")))]
    fn choose_library_location(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(text!(account::choose_default_library_location).into()) });
        let preferences = self.services.preferences.clone();
        cx.spawn(async move |page, cx| {
            let Ok(Ok(Some(paths))) = selection.await else { return };
            let Some(path) = paths.first() else { return };
            let result = preferences.set_default_library_save_location(path.to_string_lossy().into_owned()).await.map_err(app::BackendError::operation);
            let _ = page.update(cx, |page, cx| {
                match result {
                    Ok(path) => {
                        page.default_library_location = Some(path);
                        page.error = None;
                    }
                    Err(error) => page.error = Some(user_facing_error(&error, text!(account::could_not_save_the_library_location_choose_an_accessible_folder_and_try)).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for SyncPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settings_section(window, cx)
    }
}

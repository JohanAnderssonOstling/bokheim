//! Sync and account presentation, rendered as the first section of Settings.
//!
//! The section leads with identity — who is signed in, with the sync verdict as
//! one line inside that block — then the quota as a shape, then the libraries
//! sorted by what they occupy, and last the activity: what the device has been
//! uploading and downloading, which is the only part that reports events rather
//! than state.

use super::*;

const LEGEND_LIBRARY_LIMIT: usize = 4;

fn format_last_sync(timestamp: u64) -> String {
    let now = u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(timestamp);
    let elapsed_seconds = now.saturating_sub(timestamp) / 1_000;
    if elapsed_seconds < 60 {
        "now".to_owned()
    } else if elapsed_seconds < 60 * 60 {
        format!("{}m", elapsed_seconds / 60)
    } else if elapsed_seconds < 24 * 60 * 60 {
        format!("{}h", elapsed_seconds / (60 * 60))
    } else {
        let days = elapsed_seconds / (24 * 60 * 60);
        if days == 1 { "1 day".to_owned() } else { format!("{days} days") }
    }
}

/// The single character the identity avatar carries. Uppercased from the
/// address because that is the only name the account has.
fn account_initial(email: &str) -> String {
    email.chars().next().map(|first| first.to_uppercase().to_string()).unwrap_or_else(|| "?".to_owned())
}

fn format_storage_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1_000.0 && unit < UNITS.len() - 1 {
        value /= 1_000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.2} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// One library as the page shows it: the sync status joined to the two
/// quantities the page reports for it.
///
/// `server_bytes` is what the account is charged for and is what the quota
/// meter is built from; `local_bytes` is what has been downloaded here. They
/// answer different questions and are never mixed.
struct LibraryRow {
    status: LibrarySyncStatus,
    server_bytes: Option<u64>,
    local_bytes: u64,
}

impl LibraryRow {
    /// Server bytes rank the list once the server can report them; until then
    /// the local sizes are the only sizes there are.
    const fn sort_bytes(&self) -> u64 {
        match self.server_bytes {
            Some(bytes) => bytes,
            None => self.local_bytes,
        }
    }
}

impl SyncPage {
    /// Joins the device's libraries to their sync status and local size.
    ///
    /// Libraries the backend has no status for are given the status they would
    /// have, so the verdict above the list is computed from exactly the rows the
    /// list renders rather than from a different set.
    fn library_rows(&self, cx: &Context<Self>) -> Vec<LibraryRow> {
        let signed_in = self.account_status.signed_in();
        let mut rows: Vec<LibraryRow> = self
            .active_library
            .read(cx)
            .entries()
            .iter()
            .map(|library| {
                let status = self.sync_statuses.iter().find(|status| status.library_id == *library.library_id()).cloned().unwrap_or_else(|| LibrarySyncStatus {
                    library_id: *library.library_id(),
                    library_name: library.library_name().to_owned(),
                    state: if signed_in { LibrarySyncState::RemoteMissing } else { LibrarySyncState::LocalOnly },
                    server_url: None,
                    account_id: None,
                    has_unsynced_changes: false,
                    last_synced_at: None,
                });
                let local_bytes = self.local_storage_usage.iter().find(|usage| usage.library_id == *library.library_id()).map(|usage| usage.used_bytes).unwrap_or_default();
                // A supported server that lists no share for a library is
                // reporting zero; an unsupported server reports nothing at all.
                let server_bytes = self.server_storage_usage.as_ref().map(|usage| usage.iter().find(|entry| entry.library_id == *library.library_id()).map(|entry| entry.used_bytes).unwrap_or_default());
                LibraryRow { status, server_bytes, local_bytes }
            })
            .collect();
        // Sorted by the quantity the page leads with, falling back to the local
        // sizes while no server breakdown is available.
        rows.sort_by(|left, right| right.sort_bytes().cmp(&left.sort_bytes()).then_with(|| left.status.library_name.cmp(&right.status.library_name)));
        rows
    }

    /// The rest of the status line: how many libraries, and how recently they
    /// synced. It does not name the account — the block it sits in already does.
    fn identity_detail(&self, summary: LibrarySyncSummary) -> String {
        let libraries = if summary.library_count == 1 { "1 library".to_owned() } else { format!("{} libraries", summary.library_count) };
        let last_sync = match summary.last_synced_at {
            Some(timestamp) => format!("last sync {}", format_last_sync(timestamp)),
            None => "never synced".to_owned(),
        };
        format!("{libraries} · {last_sync}")
    }

    /// A password field whose reveal stays on until it is pressed again.
    ///
    /// The input's own `mask_toggle` unmasks only while the mouse button is
    /// held, which cannot be used to check a password as it is typed and does
    /// not survive a touch at all.
    fn password_input(&self, state: &Entity<InputState>, id: &'static str, entity: &Entity<Self>) -> Input {
        let revealed = self.password_revealed;
        let toggle = entity.clone();
        let toggled_state = state.clone();
        Input::new(state).suffix(Button::new(id).icon(if revealed { IconName::EyeOff } else { IconName::Eye }).xsmall().ghost().tooltip(if revealed { "Hide password" } else { "Show password" }).on_click(move |_, window, cx| {
            toggled_state.update(cx, |state, cx| state.set_masked(revealed, window, cx));
            toggle.update(cx, |page, cx| {
                page.password_revealed = !revealed;
                cx.notify();
            });
        }))
    }

    /// The sign-in, registration, verification and password-reset forms.
    ///
    /// Only reachable while signed out, so it is kept apart from the page the
    /// account renders once there is an account to report on.
    ///
    /// Every flow has the same shape: what this screen is, the inputs it needs,
    /// one action that goes forward, and — quieter — the way to another flow.
    /// The inputs carry their own placeholders and are never labelled as well.
    fn account_access_form(&self, entity: &Entity<Self>, feedback: Option<gpui::AnyElement>, theme: components::BrowserTheme) -> Div {
        let form = components::access_form();
        match self.flow {
            AccountFlow::SignIn => {
                let login = entity.clone();
                let register = entity.clone();
                let reset = entity.clone();
                let mut actions = components::action_row().child(components::primary_action_button("account-sign-in", "Sign in", IconName::Globe, self.busy).on_click(move |_, _, cx| {
                    login.update(cx, |page, cx| page.start_login(cx));
                }));
                actions = actions.child(components::secondary_action_button("account-register", "Create account", IconName::User, self.busy).on_click(move |_, window, cx| {
                    register.update(cx, |page, cx| page.set_flow(AccountFlow::Register, window, cx));
                }));
                let mut form = form
                    .child(components::section_header("Sign in", "Your libraries, reading positions and purchases, on every device you use.", theme))
                    .children(feedback)
                    .child(Input::new(&self.authentication.login_email))
                    .child(self.password_input(&self.authentication.password, "toggle-sign-in-password", entity))
                    .child(actions);
                // Recovery is offered once a sign-in has actually failed, so
                // the screen a returning user sees carries one action, not
                // three. It stays a link: it is the way out, not the way on.
                if self.login_failed {
                    form = form.child(components::link_button("account-request-reset", "Forgot your password?", theme, self.busy).on_click(move |_, window, cx| {
                        reset.update(cx, |page, cx| page.set_flow(AccountFlow::RequestPasswordReset, window, cx));
                    }));
                }
                form
            }
            AccountFlow::Register => {
                let register = entity.clone();
                let sign_in = entity.clone();
                form.child(components::section_header("Create account", "Only needed to keep more than one device in step. Reading works without one.", theme))
                    .children(feedback)
                    .child(Input::new(&self.authentication.email))
                    .child(self.password_input(&self.authentication.registration_password, "toggle-registration-password", entity))
                    .child(components::form_hint(format!("At least {} characters.", account_contract::PASSWORD_MIN_CHARACTERS), theme))
                    .child(components::action_row().child(components::primary_action_button("account-register", "Create account", IconName::User, self.busy).on_click(move |_, _, cx| {
                        register.update(cx, |page, cx| page.start_registration(cx));
                    })))
                    .child(components::link_button("account-sign-in", "Already have an account? Sign in", theme, self.busy).on_click(move |_, window, cx| {
                        sign_in.update(cx, |page, cx| page.set_flow(AccountFlow::SignIn, window, cx));
                    }))
            }
            AccountFlow::VerifyEmail => {
                let verify = entity.clone();
                let resend = entity.clone();
                let start_over = entity.clone();
                let mut actions = components::action_row().child(components::primary_action_button("account-verify", "Verify email", IconName::ArrowRight, self.busy).on_click(move |_, _, cx| {
                    verify.update(cx, |page, cx| page.start_verify_email(cx));
                }));
                actions = actions.child(components::secondary_action_button("account-resend", "Resend PIN", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                    resend.update(cx, |page, cx| page.start_resend_verification(cx));
                }));
                form.child(components::section_header("Check your email", format!("Enter the {}-digit PIN we sent you.", account_contract::EMAIL_VERIFICATION_PIN_DIGITS), theme))
                    .children(feedback)
                    .child(OtpInput::new(&self.authentication.verification_pin))
                    .child(actions)
                    .child(components::link_button("account-restart-registration", "Wrong address? Start over", theme, self.busy).on_click(move |_, window, cx| {
                        start_over.update(cx, |page, cx| page.set_flow(AccountFlow::Register, window, cx));
                    }))
            }
            AccountFlow::RequestPasswordReset => {
                let request = entity.clone();
                let sign_in = entity.clone();
                form.child(components::section_header("Reset your password", "We will email a token you can use to set a new one.", theme))
                    .children(feedback)
                    .child(Input::new(&self.authentication.email))
                    .child(components::action_row().child(components::primary_action_button("account-request-reset", "Email me a reset token", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                        request.update(cx, |page, cx| page.start_password_reset_request(cx));
                    })))
                    .child(components::link_button("account-sign-in", "Back to sign in", theme, self.busy).on_click(move |_, window, cx| {
                        sign_in.update(cx, |page, cx| page.set_flow(AccountFlow::SignIn, window, cx));
                    }))
            }
            AccountFlow::ResetPassword => {
                let reset = entity.clone();
                let sign_in = entity.clone();
                // The token is a pasted string, not the six-digit PIN, so it is
                // a plain input rather than the OTP boxes — drawing them alike
                // invites people to type six of its characters and give up.
                form.child(components::section_header("Set a new password", "Paste the token from the email. It is not the six-digit PIN.", theme))
                    .children(feedback)
                    .child(Input::new(&self.authentication.reset_token))
                    .child(self.password_input(&self.authentication.new_password, "toggle-new-password", entity))
                    .child(components::form_hint(format!("At least {} characters.", account_contract::PASSWORD_MIN_CHARACTERS), theme))
                    .child(components::action_row().child(components::primary_action_button("account-reset-password", "Set new password", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                        reset.update(cx, |page, cx| page.start_password_reset(cx));
                    })))
                    .child(components::link_button("account-sign-in", "Back to sign in", theme, self.busy).on_click(move |_, window, cx| {
                        sign_in.update(cx, |page, cx| page.set_flow(AccountFlow::SignIn, window, cx));
                    }))
            }
        }
    }

    /// Splits the quota into the parts that explain it: what each library holds
    /// on this device, what the account holds that this device does not, and
    /// what uploads have reserved.
    ///
    /// Local copies can exceed what the server stores — books imported here but
    /// never uploaded — so library shares are taken from the used bytes in
    /// descending order and stop once those are accounted for. The remainder of
    /// the track is free space.
    fn storage_segments(&self, usage: AccountStorageUsage, rows: &[LibraryRow], theme: components::BrowserTheme) -> Vec<(SharedString, u64, gpui::Hsla)> {
        let mut segments: Vec<(SharedString, u64, gpui::Hsla)> = Vec::new();
        let mut attributed = 0_u64;
        let mut other_bytes = 0_u64;
        for (index, (name, bytes)) in rows.iter().filter_map(|row| row.server_bytes.filter(|bytes| *bytes > 0).map(|bytes| (row.status.library_name.clone(), bytes))).enumerate() {
            attributed = attributed.saturating_add(bytes);
            if index < LEGEND_LIBRARY_LIMIT {
                segments.push((name.into(), bytes, components::usage_segment_color(index, theme)));
            } else {
                other_bytes += bytes;
            }
        }
        if other_bytes > 0 {
            segments.push(("Other libraries".into(), other_bytes, components::usage_segment_color(LEGEND_LIBRARY_LIMIT, theme)));
        }
        // Charged bytes belonging to no library the device knows about: a
        // library not enrolled here, or books released everywhere but not yet
        // reclaimed. Never negative, whatever the two sources disagree about.
        let unattributed = usage.used_bytes.saturating_sub(attributed);
        if unattributed > 0 {
            segments.push(("Elsewhere on the account".into(), unattributed, theme.text_muted));
        }
        if usage.reserved_bytes > 0 {
            segments.push(("Reserved by uploads".into(), usage.reserved_bytes, theme.warning));
        }
        segments
    }

    fn activity_section(&self, theme: components::BrowserTheme) -> gpui::AnyElement {
        let mut events = components::activity_list();
        let mut event_count = 0_usize;
        for group in self.transfers.groups.iter().filter(|group| group.scanner_running) {
            events = events.child(components::activity_event(theme.accent, format!("Scanning library · {}", group.library_name), "Checking local files · running", None, theme));
            event_count += 1;
        }
        for group in self.transfers.groups.iter() {
            for transfer in &group.transfers {
                let (color, progress) = match transfer.state {
                    bokheim_backend::TransferState::Running => (theme.accent, Some(transfer.completed_items as f32 / transfer.total_items.max(1) as f32)),
                    bokheim_backend::TransferState::Failed => (theme.danger, None),
                    _ => (theme.warning, None),
                };
                let detail = match transfer.last_error.as_deref() {
                    Some(error) => format!("{} of {} · {} · attempt {} · {error}", transfer.completed_items, transfer.total_items, group.library_name, transfer.attempts),
                    None => format!("{} of {} · {} · {}", transfer.completed_items, transfer.total_items, group.library_name, transfer.state.label().to_lowercase()),
                };
                events = events.child(components::activity_event(color, format!("{} · {}", transfer.kind.batch_label(), group.library_name), detail, progress, theme));
                event_count += 1;
            }
        }
        for group in self.transfers.groups.iter() {
            for transfer in &group.history {
                let completed = matches!(transfer.state, bokheim_backend::TransferState::Completed);
                let color = if completed { theme.text_muted } else { theme.danger };
                let title = match transfer.kind {
                    bokheim_backend::TransferJobKind::UploadBook => "Uploaded books",
                    bokheim_backend::TransferJobKind::UploadThumbnail => "Uploaded thumbnails",
                    bokheim_backend::TransferJobKind::DownloadBook => "Downloaded books",
                    bokheim_backend::TransferJobKind::DownloadThumbnail => "Downloaded thumbnails",
                };
                let detail = match transfer.last_error.as_deref() {
                    Some(error) => format!("{} of {} · {} · {error}", transfer.completed_items, transfer.total_items, group.library_name),
                    None => format!("{} of {} · {}", transfer.completed_items, transfer.total_items, group.library_name),
                };
                events = events.child(components::activity_event(color, format!("{title} · {}", group.library_name), detail, None, theme));
                event_count += 1;
            }
        }

        let activity = GroupBox::new().fill().child(components::section_header("Batch jobs", "", theme));
        if event_count == 0 { activity.child(Label::new("Nothing queued").min_h(px(64.0)).flex().items_center().text_color(theme.text_muted)).into_any_element() } else { activity.child(events).into_any_element() }
    }
}

impl SyncPage {
    /// The account as it appears inside Settings: the same content the page
    /// rendered when it was a destination of its own, without the scroll
    /// container and page width that Settings now provides.
    pub(crate) fn settings_section(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = components::browser_theme(cx);
        let entity = cx.entity();
        let mut content = components::browser_settings_sections();
        // Whichever of the two the last attempt produced. They are never both
        // set, and one slot means a form cannot grow two lines of chrome.
        let feedback = match (&self.error, &self.message) {
            (Some(error), _) => Some(Alert::error("account-error", error.clone()).with_size(Size::Small).into_any_element()),
            (None, Some(message)) => Some(Alert::info("account-message", message.clone()).with_size(Size::Small).into_any_element()),
            (None, None) => None,
        };

        if !self.account_status.signed_in() {
            return content.child(GroupBox::new().fill().child(self.account_access_form(&entity, feedback, theme))).child(self.activity_section(theme));
        }

        let rows = self.library_rows(cx);
        let summary = LibrarySyncSummary::from_statuses(&rows.iter().map(|row| row.status.clone()).collect::<Vec<_>>());
        let settled = summary.all_synced();
        let headline = if self.busy {
            "Syncing…"
        } else if settled {
            "Everything is synced"
        } else if summary.library_count == 0 {
            "No local libraries"
        } else {
            "Needs attention"
        };

        let sync_now = entity.clone();
        let sign_out = entity.clone();
        let actions = components::action_row()
            .child(components::primary_action_button("account-sync-now", "Sync now", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                sync_now.update(cx, |page, cx| page.start_sync(cx));
            }))
            .child(components::secondary_action_button("cloud-logout", "Sign out", IconName::Close, self.account_action_busy).on_click(move |_, _, cx| {
                sign_out.update(cx, |page, cx| page.start_logout(cx));
            }));
        let email = self.account_status.email().unwrap_or_default();
        // While a sync is running the status line keeps the last known verdict
        // rather than going amber: "Syncing…" is not itself a problem to report.
        content = content.child(components::account_identity(account_initial(email), email.to_owned(), format!("{headline} · {}", self.identity_detail(summary)), settled || self.busy, actions, theme)).children(feedback);

        if let Some(usage) = self.storage_usage {
            let mut storage = components::storage_block().child(components::storage_figures(
                format_storage_bytes(usage.used_bytes),
                format!("of {} used", format_storage_bytes(usage.quota_bytes)),
                format!("{} available", format_storage_bytes(usage.available_bytes())),
                theme,
            ));
            if usage.quota_bytes > 0 {
                let segments = self.storage_segments(usage, &rows, theme).into_iter().map(|(label, bytes, color)| (bytes as f32 / usage.quota_bytes as f32, color, SharedString::from(format!("{label} {}", format_storage_bytes(bytes)))));
                storage = storage.child(components::storage_meter(segments, theme));
            }
            content = content.child(GroupBox::new().fill().child(components::section_header("Server storage", "", theme)).child(storage));
        }

        let breakdown_supported = self.server_storage_usage.is_some();
        let libraries_detail = if breakdown_supported { String::new() } else { "This server does not report storage per library.".to_owned() };
        let mut libraries = GroupBox::new().fill().child(components::section_header("Libraries on this device", libraries_detail, theme));
        if !rows.is_empty() {
            libraries = libraries.child(components::usage_column_headings(breakdown_supported.then(|| SharedString::from("On the server")), "On this device", theme));
        }
        if rows.is_empty() {
            libraries = libraries.child(Label::new("No local libraries").min_h(px(64.0)).flex().items_center().text_color(theme.text_muted));
        } else {
            let largest = rows.iter().map(LibraryRow::sort_bytes).max().unwrap_or_default();
            let mut usage_rows = Vec::with_capacity(rows.len());
            for row in &rows {
                let detail = row.status.last_synced_at.map(|timestamp| format!("Last sync {}", format_last_sync(timestamp))).unwrap_or_default();
                let (state_label, state_tag) = match row.status.state {
                    LibrarySyncState::Synced if row.status.last_synced_at.is_none() => ("Sync pending", Tag::secondary()),
                    LibrarySyncState::Synced if row.status.has_unsynced_changes => ("Changes pending", Tag::warning()),
                    LibrarySyncState::Synced => ("Synced", Tag::primary()),
                    LibrarySyncState::SignInRequired => ("Sign in required", Tag::warning()),
                    LibrarySyncState::RemoteMissing => ("Upload pending", Tag::warning()),
                    LibrarySyncState::DifferentAccount => ("Unavailable", Tag::danger()),
                    LibrarySyncState::LocalOnly => ("Local", Tag::secondary()),
                };
                let fraction = if largest == 0 { 0.0 } else { row.sort_bytes() as f32 / largest as f32 };
                usage_rows.push(components::library_usage_row(
                    row.status.library_name.clone(),
                    detail,
                    row.server_bytes.map(|bytes| SharedString::from(format_storage_bytes(bytes))),
                    format_storage_bytes(row.local_bytes),
                    fraction,
                    state_tag.rounded(px(components::RADIUS_SM)).with_size(Size::Small).child(state_label.to_owned()),
                    theme,
                ));
            }
            libraries = libraries.child(components::library_usage_list(usage_rows));
        }
        content = content.child(libraries);

        content = content.child(self.activity_section(theme));

        content
    }
}

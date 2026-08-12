//! Sync and account presentation.
//!
//! The page states its conclusion first — one line answering "is everything
//! synced?" — and everything below it is the evidence for that line: the quota
//! as a shape, the libraries sorted by what they occupy, and the batch jobs
//! reduced to a footer until they have something to report.

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

    /// The line under the headline: how many libraries, how recently they
    /// synced, and who the account belongs to.
    fn verdict_detail(&self, summary: LibrarySyncSummary, contract: &app_contract::AccountContract) -> String {
        let libraries = if summary.library_count == 1 { "1 library".to_owned() } else { format!("{} libraries", summary.library_count) };
        let mut parts = vec![if summary.attention_count == 0 { libraries } else { format!("{} of {} {}", summary.attention_count, libraries, contract.summary.needs_attention_suffix) }];
        match summary.last_synced_at {
            Some(timestamp) => parts.push(format!("last sync {}", format_last_sync(timestamp))),
            None => parts.push("never synced".to_owned()),
        }
        if let Some(email) = self.account_status.email() {
            parts.push(email.to_owned());
        }
        parts.join(" · ")
    }

    /// The sign-in, registration, verification and password-reset forms.
    ///
    /// Only reachable while signed out, so it is kept apart from the page the
    /// account renders once there is an account to report on.
    fn account_access_form(&self, contract: &app_contract::AccountContract, entity: &Entity<Self>) -> Div {
        match self.flow {
            AccountFlow::SignIn => {
                let login = entity.clone();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(11.0))
                    .child(Form::vertical().child(Field::new().label("Email").child(Input::new(&self.authentication.login_email))).child(Field::new().label("Password").child(Input::new(&self.authentication.password).mask_toggle())))
                    .child(components::action_row().child(components::primary_action_button("account-sign-in", "Sign in", IconName::Globe, self.busy).on_click(move |_, _, cx| {
                        login.update(cx, |page, cx| page.start_login(cx));
                    })))
            }
            AccountFlow::Register => {
                let register = entity.clone();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(11.0))
                    .child(
                        Form::vertical().child(Field::new().label("Email").child(Input::new(&self.authentication.email))).child(
                            Field::new()
                                .label("Password")
                                .description(format!("Passwords require at least {} characters.", contract.validation.password_min_characters))
                                .child(Input::new(&self.authentication.registration_password).mask_toggle()),
                        ),
                    )
                    .child(components::action_row().child(components::primary_action_button("account-register", "Create account", IconName::User, self.busy).on_click(move |_, _, cx| {
                        register.update(cx, |page, cx| page.start_registration(cx));
                    })))
            }
            AccountFlow::VerifyEmail => {
                let verify = entity.clone();
                let resend = entity.clone();
                div().w_full().flex().flex_col().gap(px(11.0)).child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(components::SPACE_SM))
                        .child(div().flex_1().min_w_0().child(OtpInput::new(&self.authentication.verification_pin).groups(1).with_size(px(components::TOPBAR_ACTION_HEIGHT))))
                        .child(components::primary_action_button("account-verify", "Verify email", IconName::ArrowRight, self.busy).w(px(144.0)).flex_none().on_click(move |_, _, cx| {
                            verify.update(cx, |page, cx| page.start_verify_email(cx));
                        }))
                        .child(components::secondary_action_button("account-resend", "Resend PIN", IconName::Redo2, self.busy).w(px(144.0)).flex_none().on_click(move |_, _, cx| {
                            resend.update(cx, |page, cx| page.start_resend_verification(cx));
                        })),
                )
            }
            AccountFlow::RequestPasswordReset => {
                let request = entity.clone();
                let token = entity.clone();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(11.0))
                    .child(Form::vertical().child(Field::new().label("Email").description("Request a reset token by email. The response does not reveal whether an address exists.").child(Input::new(&self.authentication.email))))
                    .child(
                        components::action_row()
                            .child(components::primary_action_button("account-request-reset", "Send reset token", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                                request.update(cx, |page, cx| page.start_password_reset_request(cx));
                            }))
                            .child(components::secondary_action_button("account-have-reset", "Enter reset token", IconName::ArrowRight, self.busy).on_click(move |_, _, cx| {
                                token.update(cx, |page, cx| page.set_flow(AccountFlow::ResetPassword, cx));
                            })),
                    )
            }
            AccountFlow::ResetPassword => {
                let reset = entity.clone();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(11.0))
                    .child(
                        Form::vertical().child(Field::new().label("Reset token").child(Input::new(&self.authentication.reset_token))).child(
                            Field::new()
                                .label("New password")
                                .description(format!("Choose a new password of at least {} characters.", contract.validation.password_min_characters))
                                .child(Input::new(&self.authentication.new_password).mask_toggle()),
                        ),
                    )
                    .child(components::action_row().child(components::primary_action_button("account-reset-password", "Set new password", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                        reset.update(cx, |page, cx| page.start_password_reset(cx));
                    })))
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
    fn storage_segments(&self, usage: AccountStorageUsage, rows: &[LibraryRow], theme: components::BrowserTheme, contract: &app_contract::AccountContract) -> Vec<(SharedString, u64, gpui::Hsla)> {
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
            segments.push((contract.summary.other_libraries_label.clone().into(), other_bytes, components::usage_segment_color(LEGEND_LIBRARY_LIMIT, theme)));
        }
        // Charged bytes belonging to no library the device knows about: a
        // library not enrolled here, or books released everywhere but not yet
        // reclaimed. Never negative, whatever the two sources disagree about.
        let unattributed = usage.used_bytes.saturating_sub(attributed);
        if unattributed > 0 {
            segments.push((contract.summary.unattributed_label.clone().into(), unattributed, theme.text_muted));
        }
        if usage.reserved_bytes > 0 {
            segments.push((contract.summary.reserved_label.clone().into(), usage.reserved_bytes, theme.warning));
        }
        segments
    }
}

impl Render for SyncPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let contract = self.services.contracts.account.clone();
        let entity = cx.entity();
        let mut content = components::centered_page_content();

        if !self.account_status.signed_in() {
            let mut account = GroupBox::new().fill();
            let flows = [
                ("account-flow-sign-in", "Sign in", IconName::Globe, AccountFlow::SignIn),
                ("account-flow-register", "Create account", IconName::User, AccountFlow::Register),
                ("account-flow-reset", "Reset password", IconName::Redo2, AccountFlow::RequestPasswordReset),
            ];
            let mut actions = components::action_row();
            for (id, label, icon, flow) in flows {
                let entity = entity.clone();
                actions = actions.child(components::secondary_action_button(id, label, icon, self.account_action_busy).on_click(move |_, _, cx| {
                    entity.update(cx, |page, cx| page.set_flow(flow, cx));
                }));
            }
            account = account.child(actions).child(self.account_access_form(&contract, &entity));
            if let Some(message) = &self.message {
                account = account.child(Alert::info("account-message", message.clone()).with_size(Size::Small));
            }
            if let Some(error) = &self.error {
                account = account.child(Alert::error("account-error", error.clone()).with_size(Size::Small));
            }
            return components::scrollable_page().child(content.child(account)).into_any_element();
        }

        let rows = self.library_rows(cx);
        let summary = LibrarySyncSummary::from_statuses(&rows.iter().map(|row| row.status.clone()).collect::<Vec<_>>());
        let settled = summary.all_synced();
        let headline = if self.busy {
            contract.summary.syncing_label.clone()
        } else if settled {
            contract.summary.all_synced_label.clone()
        } else if summary.library_count == 0 {
            contract.no_local_libraries_label.clone()
        } else {
            contract.summary.attention_label.clone()
        };

        let sync_now = entity.clone();
        let sign_out = entity.clone();
        let banner_actions = components::action_row()
            .child(components::primary_action_button("account-sync-now", contract.summary.sync_now_label.clone(), IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                sync_now.update(cx, |page, cx| page.refresh(cx));
            }))
            .child(components::secondary_action_button("cloud-logout", contract.sign_out_label.clone(), IconName::Close, self.account_action_busy).on_click(move |_, _, cx| {
                sign_out.update(cx, |page, cx| page.start_logout(cx));
            }));
        // While a sync is running the stripe keeps the last known verdict rather
        // than going amber: "Syncing…" is not itself a problem to report.
        content = content.child(components::verdict_banner(headline, self.verdict_detail(summary, &contract), settled, banner_actions, theme));

        if let Some(message) = &self.message {
            content = content.child(Alert::info("account-message", message.clone()).with_size(Size::Small));
        }
        if let Some(error) = &self.error {
            content = content.child(Alert::error("account-error", error.clone()).with_size(Size::Small));
        }

        if let Some(usage) = self.storage_usage {
            let mut storage = components::storage_block().child(components::storage_figures(
                format_storage_bytes(usage.used_bytes),
                format!("of {} used", format_storage_bytes(usage.quota_bytes)),
                format!("{} available", format_storage_bytes(usage.available_bytes())),
                theme,
            ));
            if usage.quota_bytes > 0 {
                let segments = self.storage_segments(usage, &rows, theme, &contract);
                let mut track = components::meter_track(theme);
                let mut legend = components::meter_legend();
                for (label, bytes, color) in segments {
                    track = track.child(components::meter_segment(bytes as f32 / usage.quota_bytes as f32, color));
                    legend = legend.child(components::meter_legend_entry(color, format!("{label} {}", format_storage_bytes(bytes)), theme));
                }
                storage = storage.child(track).child(legend);
            }
            content = content.child(GroupBox::new().fill().child(components::section_header(contract.summary.storage_section_label.clone(), "", theme)).child(storage));
        }

        let breakdown_supported = self.server_storage_usage.is_some();
        let libraries_detail = if breakdown_supported { String::new() } else { contract.summary.breakdown_unavailable_label.clone() };
        let mut libraries = GroupBox::new().fill().child(components::section_header(contract.libraries_section_label.clone(), libraries_detail, theme));
        if !rows.is_empty() {
            libraries = libraries.child(components::usage_column_headings(breakdown_supported.then(|| SharedString::from(contract.summary.server_usage_label.clone())), contract.summary.device_usage_label.clone(), theme));
        }
        if rows.is_empty() {
            libraries = libraries.child(Label::new(contract.no_local_libraries_label.clone()).min_h(px(64.0)).flex().items_center().text_color(theme.text_muted));
        } else {
            let largest = rows.iter().map(LibraryRow::sort_bytes).max().unwrap_or_default();
            let mut list = components::item_list();
            for row in &rows {
                let definition = contract.sync_states.iter().find(|definition| definition.state == row.status.state);
                let detail = row.status.last_synced_at.map(|timestamp| format!("Last sync {}", format_last_sync(timestamp))).unwrap_or_default();
                let (state_label, state_tag) = match row.status.state {
                    LibrarySyncState::Synced if row.status.last_synced_at.is_none() => ("Not synced", Tag::secondary()),
                    LibrarySyncState::Synced if row.status.has_unsynced_changes => ("Changes pending", Tag::warning()),
                    LibrarySyncState::Synced => ("Synced", Tag::primary()),
                    LibrarySyncState::SignInRequired | LibrarySyncState::RemoteMissing => (definition.map(|definition| definition.label.as_str()).unwrap_or("Unknown"), Tag::warning()),
                    LibrarySyncState::DifferentAccount => (definition.map(|definition| definition.label.as_str()).unwrap_or("Unknown"), Tag::danger()),
                    LibrarySyncState::LocalOnly => (definition.map(|definition| definition.label.as_str()).unwrap_or("Unknown"), Tag::secondary()),
                };
                let fraction = if largest == 0 { 0.0 } else { row.sort_bytes() as f32 / largest as f32 };
                let mut item = components::item_row(theme).child(components::item_identity(row.status.library_name.clone(), detail, theme));
                if let Some(server_bytes) = row.server_bytes {
                    item = item.child(components::usage_amount(format_storage_bytes(server_bytes), theme));
                }
                list = list.child(
                    item.child(components::usage_amount_secondary(format_storage_bytes(row.local_bytes), theme))
                        .child(components::usage_bar(fraction, theme))
                        .child(state_tag.rounded_full().with_size(Size::Small).child(state_label.to_owned())),
                );
            }
            libraries = libraries.child(list);
        }
        content = content.child(libraries);

        let active = self.transfers.groups.iter().map(|group| group.transfers.len()).sum::<usize>();
        if active > 0 {
            let transfers = GroupBox::new().fill().child(components::section_header(contract.transfers_section_label.clone(), "Queued, running, or retrying transfers.", theme));
            let mut list = components::item_list();
            for group in self.transfers.groups.iter() {
                for transfer in &group.transfers {
                    let detail = match transfer.last_error.as_deref() {
                        Some(error) => format!("{}/{} · {} · attempt {} · {}", transfer.completed_items, transfer.total_items, group.library_name, transfer.attempts, error),
                        None => format!("{}/{} · {}", transfer.completed_items, transfer.total_items, group.library_name),
                    };
                    let status = if matches!(transfer.state, epub_svelte_lib::TransferState::Running) { Tag::primary() } else { Tag::secondary() };
                    list = list.child(components::item_row(theme).child(components::item_identity(transfer.kind.batch_label(), detail, theme)).child(status.rounded_full().with_size(Size::Small).child(transfer.state.label())));
                }
            }
            content = content.child(transfers.child(list));
        }

        let history_count = self.transfers.groups.iter().map(|group| group.history.len()).sum::<usize>();
        let footer_summary = if history_count == 0 { contract.summary.activity_idle_label.clone() } else { format!("{history_count} {}", contract.summary.finished_suffix) };
        let toggle = entity.clone();
        let toggle_label = if self.activity_expanded { contract.summary.hide_activity_label.clone() } else { contract.summary.show_activity_label.clone() };
        let footer_action = components::secondary_action_button("account-activity-toggle", toggle_label, IconName::ArrowRight, history_count == 0).on_click(move |_, _, cx| {
            toggle.update(cx, |page, cx| {
                page.activity_expanded = !page.activity_expanded;
                cx.notify();
            });
        });
        content = content.child(components::activity_footer(footer_summary, footer_action, theme));

        if self.activity_expanded && history_count > 0 {
            let history = GroupBox::new().fill().child(components::section_header(contract.transfer_history_section_label.clone(), "The latest completed batch jobs on this device.", theme));
            let mut list = components::item_list();
            for group in self.transfers.groups.iter() {
                for transfer in &group.history {
                    let direction = match transfer.kind {
                        epub_svelte_lib::TransferJobKind::UploadBook => "Uploaded books",
                        epub_svelte_lib::TransferJobKind::UploadThumbnail => "Uploaded thumbnails",
                        epub_svelte_lib::TransferJobKind::DownloadBook => "Downloaded books",
                        epub_svelte_lib::TransferJobKind::DownloadThumbnail => "Downloaded thumbnails",
                    };
                    let detail = match transfer.last_error.as_deref() {
                        Some(error) => format!("{}/{} · {} · {}", transfer.completed_items, transfer.total_items, group.library_name, error),
                        None => format!("{}/{} · {}", transfer.completed_items, transfer.total_items, group.library_name),
                    };
                    let status = if matches!(transfer.state, epub_svelte_lib::TransferState::Completed) { Tag::primary() } else { Tag::secondary() };
                    list = list.child(components::item_row(theme).child(components::item_identity(direction, detail, theme)).child(status.rounded_full().with_size(Size::Small).child(transfer.state.label())));
                }
            }
            content = content.child(history.child(list));
        }

        components::scrollable_page().child(content).into_any_element()
    }
}

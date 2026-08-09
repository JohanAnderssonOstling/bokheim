//! Sync and account presentation.

use super::*;

impl Render for SyncPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let heading_font = crate::stores::heading_font(cx);
        let contract = self.services.contracts.account.clone();
        let entity = cx.entity();
        let mut content = components::cloud_page_content();

        let mut account = components::cloud_section(theme);
        if self.account_status.signed_in() {
            let identity = format!("{} · {}", self.account_status.email().unwrap_or("Unknown user"), self.account_status.server_url().unwrap_or("Unknown server"));
            let logout = entity.clone();
            account = account.child(components::cloud_row(theme).child(components::cloud_row_identity("Signed in", identity, theme)).child(
                components::cloud_secondary_button("cloud-logout", contract.sign_out_label.clone(), IconName::Close, self.busy).on_click(move |_, _, cx| {
                    logout.update(cx, |page, cx| page.start_logout(cx));
                }),
            ));
        } else {
            let flows = [
                ("account-flow-sign-in", "Sign in", IconName::Globe, AccountFlow::SignIn),
                ("account-flow-register", "Create account", IconName::User, AccountFlow::Register),
                ("account-flow-reset", "Reset password", IconName::Redo2, AccountFlow::RequestPasswordReset),
            ];
            let mut actions = components::cloud_actions();
            for (id, label, icon, flow) in flows {
                let entity = entity.clone();
                actions = actions.child(components::cloud_secondary_button(id, label, icon, self.busy).on_click(move |_, _, cx| {
                    entity.update(cx, |page, cx| page.set_flow(flow, cx));
                }));
            }
            account = account.child(actions);

            let form = match self.flow {
                AccountFlow::SignIn => {
                    let login = entity.clone();
                    components::cloud_form().child(components::cloud_field("Email", Input::new(&self.authentication.login_email), theme)).child(components::cloud_field("Password", Input::new(&self.authentication.password), theme)).child(
                        components::cloud_actions().child(components::cloud_primary_button("account-sign-in", "Sign in", IconName::Globe, self.busy).on_click(move |_, _, cx| {
                            login.update(cx, |page, cx| page.start_login(cx));
                        })),
                    )
                }
                AccountFlow::Register => {
                    let register = entity.clone();
                    let verify = entity.clone();
                    components::cloud_form()
                        .child(components::cloud_message(format!("Passwords require at least {} characters.", contract.validation.password_min_characters), theme))
                        .child(components::cloud_field("Email", Input::new(&self.authentication.email), theme))
                        .child(components::cloud_field("Password", Input::new(&self.authentication.registration_password), theme))
                        .child(
                            components::cloud_actions()
                                .child(components::cloud_primary_button("account-register", "Create account", IconName::User, self.busy).on_click(move |_, _, cx| {
                                    register.update(cx, |page, cx| page.start_registration(cx));
                                }))
                                .child(components::cloud_secondary_button("account-have-verification", "Enter verification token", IconName::ArrowRight, self.busy).on_click(move |_, _, cx| {
                                    verify.update(cx, |page, cx| page.set_flow(AccountFlow::VerifyEmail, cx));
                                })),
                        )
                }
                AccountFlow::VerifyEmail => {
                    let verify = entity.clone();
                    let resend = entity.clone();
                    components::cloud_form()
                        .child(components::cloud_message("Enter the token from your verification email. Verification signs you in.", theme))
                        .child(components::cloud_field("Verification token", Input::new(&self.authentication.token), theme))
                        .child(components::cloud_field("Email for resend", Input::new(&self.authentication.email), theme))
                        .child(
                            components::cloud_actions()
                                .child(components::cloud_primary_button("account-verify", "Verify email", IconName::ArrowRight, self.busy).on_click(move |_, _, cx| {
                                    verify.update(cx, |page, cx| page.start_verify_email(cx));
                                }))
                                .child(components::cloud_secondary_button("account-resend", "Resend token", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                                    resend.update(cx, |page, cx| page.start_resend_verification(cx));
                                })),
                        )
                }
                AccountFlow::RequestPasswordReset => {
                    let request = entity.clone();
                    let token = entity.clone();
                    components::cloud_form()
                        .child(components::cloud_message("Request a reset token by email. The response does not reveal whether an address exists.", theme))
                        .child(components::cloud_field("Email", Input::new(&self.authentication.email), theme))
                        .child(
                            components::cloud_actions()
                                .child(components::cloud_primary_button("account-request-reset", "Send reset token", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                                    request.update(cx, |page, cx| page.start_password_reset_request(cx));
                                }))
                                .child(components::cloud_secondary_button("account-have-reset", "Enter reset token", IconName::ArrowRight, self.busy).on_click(move |_, _, cx| {
                                    token.update(cx, |page, cx| page.set_flow(AccountFlow::ResetPassword, cx));
                                })),
                        )
                }
                AccountFlow::ResetPassword => {
                    let reset = entity.clone();
                    components::cloud_form()
                        .child(components::cloud_message(format!("Choose a new password of at least {} characters.", contract.validation.password_min_characters), theme))
                        .child(components::cloud_field("Reset token", Input::new(&self.authentication.token), theme))
                        .child(components::cloud_field("New password", Input::new(&self.authentication.new_password), theme))
                        .child(components::cloud_actions().child(components::cloud_primary_button("account-reset-password", "Set new password", IconName::Redo2, self.busy).on_click(move |_, _, cx| {
                            reset.update(cx, |page, cx| page.start_password_reset(cx));
                        })))
                }
            };
            account = account.child(form);
        }
        if let Some(message) = &self.message {
            account = account.child(components::cloud_message(message.clone(), theme));
        }
        if let Some(error) = &self.error {
            account = account.child(components::cloud_error(error.clone(), theme));
        }
        content = content.child(account);

        if self.account_status.signed_in() {
            let mut sessions = components::cloud_section(theme).child(components::cloud_section_header("Sessions", "Review this login and revoke sessions you no longer recognize.", heading_font.clone(), theme));
            if self.sessions.items.len() > 1 {
                let revoke = entity.clone();
                sessions = sessions.child(components::cloud_actions().child(components::cloud_secondary_button("cloud-revoke-other-sessions", "Sign out others", IconName::Close, self.busy).on_click(move |_, _, cx| {
                    revoke.update(cx, |page, cx| page.start_revoke_other_sessions(cx));
                })));
            }
            if self.sessions.items.is_empty() {
                sessions = sessions.child(components::cloud_empty("No active sessions", theme));
            } else {
                let mut list = components::cloud_list();
                for session in self.sessions.items.iter() {
                    let session_id = session.session_id.clone();
                    let revoke = entity.clone();
                    let label = if session.current { "Sign out" } else { "Revoke" };
                    let title = if session.current { "Current session" } else { "Other session" };
                    list = list.child(components::cloud_row(theme).child(components::cloud_row_identity(title, sessions::detail(session), theme)).child(
                        components::cloud_secondary_button(format!("cloud-revoke-session-{session_id}"), label, IconName::Close, self.busy).on_click(move |_, _, cx| {
                            let session_id = session_id.clone();
                            revoke.update(cx, |page, cx| page.start_revoke_session(session_id, cx));
                        }),
                    ));
                }
                sessions = sessions.child(list);
            }
            content = content.child(sessions);
        }

        let entries = self.active_library.read(cx).entries().to_vec();
        let mut libraries = components::cloud_section(theme).child(components::cloud_section_header(contract.libraries_section_label.clone(), "Every library synchronizes automatically while signed in.", heading_font.clone(), theme));
        if entries.is_empty() {
            libraries = libraries.child(components::cloud_empty(contract.no_local_libraries_label.clone(), theme));
        } else {
            let mut list = components::cloud_list();
            for library in &entries {
                let library_id = library.library_id().as_str().to_owned();
                let status = self.sync_statuses.iter().find(|status| status.library_id == library_id);
                let state = status.map(|status| status.state).unwrap_or(if self.account_status.signed_in() { LibrarySyncState::RemoteMissing } else { LibrarySyncState::LocalOnly });
                let server = status.and_then(|status| status.server_url.as_deref()).unwrap_or("server");
                let definition = contract.sync_states.iter().find(|definition| definition.state == state);
                let state_label = definition.map(|definition| definition.label.as_str()).unwrap_or("Unknown");
                let detail = definition.map(|definition| definition.description.clone()).unwrap_or_else(|| "Synchronization state unavailable".to_owned());
                let detail = if matches!(state, LibrarySyncState::Synced | LibrarySyncState::SignInRequired | LibrarySyncState::DifferentAccount | LibrarySyncState::RemoteMissing) { format!("{detail} · {server}") } else { detail };
                list = list.child(components::cloud_row(theme).child(components::cloud_row_identity(library.library_name(), detail, theme)).child(components::cloud_badge(state_label, state == LibrarySyncState::Synced, theme)));
            }
            libraries = libraries.child(list);
        }
        content = content.child(libraries);

        let mut transfers = components::cloud_section(theme).child(components::cloud_section_header(contract.transfers_section_label.clone(), "Durable queued, running, or retrying transfers.", heading_font, theme));
        let active = self.transfers.groups.iter().map(|group| group.transfers.len()).sum::<usize>();
        if active == 0 {
            transfers = transfers.child(components::cloud_empty(contract.no_active_transfers_label.clone(), theme));
        } else {
            let mut list = components::cloud_list();
            for group in self.transfers.groups.iter() {
                for transfer in &group.transfers {
                    let path = transfer.path.as_deref().unwrap_or("Content-addressed asset");
                    let detail = match transfer.last_error.as_deref() {
                        Some(error) => format!("{} · {} · attempt {} · {}", group.library_name, transfer.kind.label(), transfer.attempts, error),
                        None => format!("{} · {} · {}", group.library_name, transfer.kind.label(), path),
                    };
                    list = list.child(components::cloud_row(theme).child(components::cloud_row_identity(transfer.book_content_hash.as_deref().unwrap_or("Library asset"), detail, theme)).child(components::cloud_badge(
                        transfer.state.label(),
                        matches!(transfer.state, epub_svelte_lib::TransferState::Running),
                        theme,
                    )));
                }
            }
            transfers = transfers.child(list);
        }
        content = content.child(transfers);

        components::cloud_page().child(content).into_any_element()
    }
}

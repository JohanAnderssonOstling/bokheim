use std::sync::Arc;
use std::time::Duration;

use bokheim_ui_components as components;
use epub_svelte_lib::APP_NAME;
use epub_svelte_lib::LibraryTransfers;
use epub_svelte_lib::app_core::{AccountStatus, LibrarySyncState, LibrarySyncStatus};
use epub_svelte_lib::home::LibraryEntry;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity};
use gpui_component::IconName;
use gpui_component::input::Input;

use crate::BrowserView;

struct CloudSnapshot {
    account: AccountStatus,
    libraries: Vec<LibraryEntry>,
    sync_statuses: Vec<LibrarySyncStatus>,
    transfers: Vec<LibraryTransfers>,
}

fn load_cloud_snapshot(core: &epub_svelte_lib::app_core::AppCore) -> Result<CloudSnapshot, String> {
    let catalogue = core.reconcile_library_catalogue_blocking().map_err(|error| error.to_string())?;
    let account = core.account_status().map_err(|error| error.to_string())?;
    let transfers = core.transfer_statuses().map_err(|error| error.to_string())?;
    Ok(CloudSnapshot { account, libraries: catalogue.libraries, sync_statuses: catalogue.sync_statuses, transfers })
}

impl BrowserView {
    fn apply_cloud_snapshot(&mut self, snapshot: CloudSnapshot, message: Option<&str>, cx: &mut Context<Self>) {
        self.account_status = snapshot.account;
        self.libraries = snapshot.libraries;
        self.sync_statuses = Arc::new(snapshot.sync_statuses);
        self.transfer_groups = Arc::new(snapshot.transfers);
        self.cloud_busy = false;
        self.cloud_error = None;
        self.cloud_message = message.map(Into::into);
        if self.active_library_id.as_deref().is_none_or(|active| !self.libraries.iter().any(|library| library.library_id().as_str() == active)) {
            self.active_library_id = self.libraries.first().map(|library| library.library_id().as_str().to_owned());
        }
        self.active_library = self.active_library_id.as_deref().and_then(|id| (self.resolve_library)(id));
        self.schedule_refresh(cx);
        cx.notify();
    }

    fn apply_cloud_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.cloud_busy = false;
        self.cloud_message = None;
        self.cloud_error = Some(error.into());
        cx.notify();
    }

    pub(crate) fn refresh_cloud(&mut self, cx: &mut Context<Self>) {
        if self.cloud_busy {
            return;
        }
        self.cloud_busy = true;
        self.cloud_error = None;
        self.cloud_message = Some("Refreshing account and transfer state…".into());
        cx.notify();

        let core = self.app_core.clone();
        let worker = self.blocking_worker.clone();
        let operation = worker.dispatch(move || load_cloud_snapshot(&core));
        self.cloud_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = operation.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => this.apply_cloud_snapshot(snapshot, None, cx),
                Err(error) => this.apply_cloud_error(error, cx),
            });
        }));
    }

    fn start_login(&mut self, cx: &mut Context<Self>) {
        if self.cloud_busy {
            return;
        }
        let username = self.account_username_input.read(cx).value().trim().to_owned();
        let password = self.account_password_input.read(cx).value().to_string();
        if username.is_empty() || password.is_empty() {
            self.cloud_error = Some("Username and password are required".into());
            cx.notify();
            return;
        }
        self.cloud_busy = true;
        self.cloud_error = None;
        self.cloud_message = Some("Signing in…".into());
        cx.notify();

        let core = self.app_core.clone();
        let worker = self.blocking_worker.clone();
        let operation = worker.dispatch(move || {
            core.login_blocking(&username, &password).map_err(|error| error.to_string())?;
            load_cloud_snapshot(&core)
        });
        self.cloud_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = operation.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => this.apply_cloud_snapshot(snapshot, Some("Signed in"), cx),
                Err(error) => this.apply_cloud_error(error, cx),
            });
        }));
    }

    fn start_logout(&mut self, cx: &mut Context<Self>) {
        if self.cloud_busy {
            return;
        }
        self.cloud_busy = true;
        self.cloud_error = None;
        self.cloud_message = Some("Signing out…".into());
        cx.notify();

        let core = self.app_core.clone();
        let worker = self.blocking_worker.clone();
        let operation = worker.dispatch(move || {
            core.logout_blocking().map_err(|error| error.to_string())?;
            load_cloud_snapshot(&core)
        });
        self.cloud_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = operation.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => this.apply_cloud_snapshot(snapshot, Some("Signed out"), cx),
                Err(error) => this.apply_cloud_error(error, cx),
            });
        }));
    }

    fn start_upload_and_sync(&mut self, library_id: String, cx: &mut Context<Self>) {
        if self.cloud_busy {
            return;
        }
        self.cloud_busy = true;
        self.cloud_error = None;
        self.cloud_message = Some("Uploading and synchronizing library…".into());
        cx.notify();
        self.schedule_transfer_poll(cx);

        let core = self.app_core.clone();
        let worker = self.blocking_worker.clone();
        let operation = worker.dispatch(move || {
            core.upload_and_sync_library_blocking(&library_id).map_err(|error| error.to_string())?;
            load_cloud_snapshot(&core)
        });
        self.cloud_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = operation.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => this.apply_cloud_snapshot(snapshot, Some("Library is up to date"), cx),
                Err(error) => this.apply_cloud_error(error, cx),
            });
        }));
    }

    fn start_disable_sync(&mut self, library_id: String, cx: &mut Context<Self>) {
        if self.cloud_busy {
            return;
        }
        self.cloud_busy = true;
        self.cloud_error = None;
        self.cloud_message = Some("Keeping library local only…".into());
        cx.notify();

        let core = self.app_core.clone();
        let worker = self.blocking_worker.clone();
        let operation = worker.dispatch(move || {
            core.disable_library_sync_blocking(&library_id).map_err(|error| error.to_string())?;
            load_cloud_snapshot(&core)
        });
        self.cloud_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = operation.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(snapshot) => this.apply_cloud_snapshot(snapshot, Some("Library is now local only"), cx),
                Err(error) => this.apply_cloud_error(error, cx),
            });
        }));
    }

    fn schedule_transfer_poll(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(Duration::from_millis(250));
        let core = self.app_core.clone();
        let worker = self.blocking_worker.clone();
        cx.spawn(async move |this, cx| {
            timer.await;
            let operation = worker.dispatch(move || core.transfer_statuses().map_err(|error| error.to_string()));
            let Ok(result) = operation.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if !this.cloud_busy {
                    return;
                }
                match result {
                    Ok(transfers) => {
                        this.transfer_groups = Arc::new(transfers);
                        cx.notify();
                    }
                    Err(error) => log::warn!("failed to poll transfer status: {error}"),
                }
                this.schedule_transfer_poll(cx);
            });
        })
        .detach();
    }

    pub(crate) fn render_cloud(&self, target: Entity<Self>, theme: components::BrowserTheme) -> AnyElement {
        let heading_font = self.heading_font_name();
        let mut content = components::cloud_page_content().child(components::cloud_page_title("Cloud", "Sign in, publish local libraries, and inspect active transfers.", heading_font.clone(), theme));

        let mut account = components::cloud_section(theme).child(components::cloud_section_header(
            "Account",
            if self.account_status.signed_in() { format!("Connected to the {APP_NAME} server") } else { "Sign in to upload and synchronize libraries".to_string() },
            heading_font.clone(),
            theme,
        ));
        if self.account_status.signed_in() {
            let identity = format!("{} · {}", self.account_status.username().unwrap_or("Unknown user"), self.account_status.server_url().unwrap_or("Unknown server"),);
            let logout_target = target.clone();
            account = account.child(components::cloud_row(theme).child(components::cloud_row_identity("Signed in", identity, theme)).child(
                components::cloud_secondary_button("cloud-logout", "Sign out", IconName::Close, self.cloud_busy).on_click(move |_, _, cx| {
                    logout_target.update(cx, |this, cx| this.start_logout(cx));
                }),
            ));
        } else {
            let login_target = target.clone();
            account = account.child(
                components::cloud_form()
                    .child(components::cloud_message(format!("Server: {}", epub_svelte_lib::API_SERVER_ORIGIN), theme))
                    .child(components::cloud_field("Username", Input::new(&self.account_username_input), theme))
                    .child(components::cloud_field("Password", Input::new(&self.account_password_input), theme))
                    .child(components::cloud_actions().child(components::cloud_primary_button("cloud-login", "Sign in", IconName::Globe, self.cloud_busy).on_click(move |_, _, cx| {
                        login_target.update(cx, |this, cx| this.start_login(cx));
                    }))),
            );
        }
        if let Some(message) = &self.cloud_message {
            account = account.child(components::cloud_message(message.clone(), theme));
        }
        if let Some(error) = &self.cloud_error {
            account = account.child(components::cloud_error(error.clone(), theme));
        }
        content = content.child(account);

        let refresh_target = target.clone();
        let mut libraries = components::cloud_section(theme).child(components::cloud_section_header("Libraries", "Upload a local library and synchronize its catalogue, books, and thumbnails.", heading_font.clone(), theme)).child(
            components::cloud_actions().child(components::cloud_secondary_button("cloud-refresh", "Refresh", IconName::Redo2, self.cloud_busy).on_click(move |_, _, cx| {
                refresh_target.update(cx, |this, cx| this.refresh_cloud(cx));
            })),
        );
        if self.libraries.is_empty() {
            libraries = libraries.child(components::cloud_empty("No local libraries", theme));
        } else {
            let mut list = components::cloud_list();
            for library in &self.libraries {
                let library_id = library.library_id().as_str().to_owned();
                let status = self.sync_statuses.iter().find(|status| status.library_id == library_id);
                let state = status.map(|status| status.state).unwrap_or_else(|| if library.sync_binding().is_some() { LibrarySyncState::SignInRequired } else { LibrarySyncState::LocalOnly });
                let server = status.and_then(|status| status.server_url.as_deref()).unwrap_or("server");
                let (state_label, detail) = match state {
                    LibrarySyncState::LocalOnly => ("Local", "Stored only on this device".to_owned()),
                    LibrarySyncState::Synced => ("Synced", format!("Sync enabled with {server}")),
                    LibrarySyncState::SignInRequired => ("Sign in required", format!("Linked to {server}")),
                    LibrarySyncState::DifferentAccount => ("Different account", format!("Linked to another account on {server}")),
                    LibrarySyncState::RemoteMissing => ("Upload pending", format!("Ready to create on {server}")),
                };
                let mut actions = components::cloud_actions();
                if matches!(state, LibrarySyncState::LocalOnly | LibrarySyncState::Synced | LibrarySyncState::RemoteMissing) {
                    let enable_target = target.clone();
                    let enable_library_id = library_id.clone();
                    let label = if state == LibrarySyncState::Synced { "Sync now" } else { "Upload and sync" };
                    actions = actions.child(components::cloud_primary_button(format!("enable-sync-{library_id}"), label, IconName::ArrowUp, self.cloud_busy || !self.account_status.signed_in()).on_click(move |_, _, cx| {
                        enable_target.update(cx, |this, cx| this.start_upload_and_sync(enable_library_id.clone(), cx));
                    }));
                }
                if state != LibrarySyncState::LocalOnly {
                    let disable_target = target.clone();
                    let disable_library_id = library_id.clone();
                    actions = actions.child(components::cloud_secondary_button(format!("disable-sync-{library_id}"), "Keep local only", IconName::Close, self.cloud_busy).on_click(move |_, _, cx| {
                        disable_target.update(cx, |this, cx| this.start_disable_sync(disable_library_id.clone(), cx));
                    }));
                }
                list =
                    list.child(components::cloud_row(theme).child(components::cloud_row_identity(library.library_name(), detail, theme)).child(components::cloud_badge(state_label, state == LibrarySyncState::Synced, theme)).child(actions));
            }
            libraries = libraries.child(list);
        }
        content = content.child(libraries);

        let mut transfers = components::cloud_section(theme).child(components::cloud_section_header("Transfers", "Durable queued, running, and retrying transfers across every local library.", heading_font, theme));
        let active_transfer_count = self.transfer_groups.iter().map(|group| group.transfers.len()).sum::<usize>();
        if active_transfer_count == 0 {
            transfers = transfers.child(components::cloud_empty("No active transfers", theme));
        } else {
            let mut list = components::cloud_list();
            for group in self.transfer_groups.iter() {
                for transfer in &group.transfers {
                    let path = transfer.path.as_deref().unwrap_or("Content-addressed asset");
                    let detail = match transfer.last_error.as_deref() {
                        Some(error) => format!("{} · {} · attempt {} · {}", group.library_name, transfer.kind.label(), transfer.attempts, error),
                        None => format!("{} · {} · {}", group.library_name, transfer.kind.label(), path),
                    };
                    list = list.child(components::cloud_row(theme).child(components::cloud_row_identity(transfer.book_id.as_deref().unwrap_or("Library asset"), detail, theme)).child(components::cloud_badge(
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

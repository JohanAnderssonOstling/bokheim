use crate::components::{create_compact_button, create_dialog_button, create_dialog_tab_button};
use crate::components::page_views::{
    account_button_area, account_status, centered_modal, create_overlay_scrim, danger_message, dialog_action_button, form_actions, form_field_row,
    form_tab_row, modal_panel, modal_title,
};
use crate::page_navigation::Signals;
use epub_svelte_lib::auth;
use epub_svelte_lib::library::sync_server_url;
use floem::event::EventPropagation;
use floem::prelude::{
    container, create_rw_signal, dyn_view, label, text_input, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith,
};
use floem::View;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthTab {
    Login,
    Signup,
}

/// A persistent top-right button. Shows "Sign in" when signed-out, or username
/// + "Log out" when signed-in.
pub fn account_button(signals: Signals, show_modal: RwSignal<bool>) -> impl View {
    let session_signal = signals.session;
    account_button_area(dyn_view(move || -> Box<dyn View> {
        match session_signal.get() {
            None => Box::new(
                create_compact_button("Sign in")
                    .on_click(move |_| {
                        show_modal.set(true);
                        EventPropagation::Stop
                    }),
            ),
            Some(s) => {
                let name = s.username.clone();
                let signals_for_click = signals.clone();
                Box::new(
                    account_status(
                        name,
                        create_compact_button("Log out").on_click(move |_| {
                            on_logout(signals_for_click.clone());
                            EventPropagation::Stop
                        }),
                    ),
                )
            }
        }
    }))
}

/// Login / signup modal overlay. Rendered alongside the home view; gated by
/// `show_modal`.
pub fn auth_modal(signals: Signals, show_modal: RwSignal<bool>) -> impl View {
    let tab = create_rw_signal(AuthTab::Login);
    let username_buf = create_rw_signal(String::new());
    let password_buf = create_rw_signal(String::new());
    let invite_buf = create_rw_signal(String::new());
    let error: RwSignal<Option<String>> = create_rw_signal(None);

    let login_tab_button = create_dialog_tab_button("Log in").on_click(move |_| {
        tab.set(AuthTab::Login);
        error.set(None);
        EventPropagation::Stop
    });
    let signup_tab_button = create_dialog_tab_button("I have an invite").on_click(move |_| {
        tab.set(AuthTab::Signup);
        error.set(None);
        EventPropagation::Stop
    });

    let invite_field = dyn_view(move || -> Box<dyn View> {
        if tab.get() == AuthTab::Signup {
            Box::new(form_field_row("Invite token", text_input(invite_buf)))
        } else {
            Box::new(container(label(|| String::new())))
        }
    });

    let signals_for_submit = signals.clone();
    let submit_button = dialog_action_button(label(move || match tab.get() {
        AuthTab::Login => "Log in".to_string(),
        AuthTab::Signup => "Sign up".to_string(),
    }))
    .on_click(move |_| {
        let mode = tab.get();
        let username = username_buf.get();
        let password = password_buf.get();
        let invite = invite_buf.get();
        let server_url = sync_server_url();
        error.set(None);

        let result = match mode {
            AuthTab::Login => auth::login_blocking(&server_url, &username, &password),
            AuthTab::Signup => auth::signup_blocking(&server_url, &invite, &username, &password),
        };

        match result {
            Ok(session) => {
                // Bind every already-open library to this account first; if any
                // is bound to a different account, abort without persisting.
                if let Err(msg) = propagate_session(&signals_for_submit, &session) {
                    error.set(Some(msg));
                    return EventPropagation::Stop;
                }
                if let Err(e) = session.save() {
                    error.set(Some(format!("could not save session: {e}")));
                    return EventPropagation::Stop;
                }
                signals_for_submit.session.set(Some(session));
                username_buf.set(String::new());
                password_buf.set(String::new());
                invite_buf.set(String::new());
                show_modal.set(false);
            }
            Err(e) => error.set(Some(format!("{e}"))),
        }
        EventPropagation::Stop
    });

    let cancel_button = create_dialog_button("Cancel").on_click(move |_| {
        show_modal.set(false);
        EventPropagation::Stop
    });

    let error_label = dyn_view(move || -> Box<dyn View> {
        match error.get() {
            Some(msg) => Box::new(danger_message(msg)),
            None => Box::new(container(label(|| String::new()))),
        }
    });

    let panel = modal_panel((
        modal_title("Account"),
        form_tab_row(login_tab_button, signup_tab_button),
        invite_field,
        form_field_row("Username", text_input(username_buf)),
        form_field_row("Password", text_input(password_buf)),
        error_label,
        form_actions(submit_button, cancel_button),
    ))
    .on_click(|_| EventPropagation::Stop);

    let centered = centered_modal(panel);

    create_overlay_scrim(
        show_modal,
        centered.on_click(move |_| {
            show_modal.set(false);
            EventPropagation::Stop
        }),
    )
}

fn on_logout(signals: Signals) {
    if let Some(session) = signals.session.get_untracked() {
        auth::logout_blocking(&session);
    }
    auth::Session::clear();
    signals.libraries.with_untracked(|libs| {
        for lib in libs {
            if let Some(handle) = lib.loaded_handle() {
                handle.clear_session();
            }
        }
    });
    signals.session.set(None);
}

fn propagate_session(signals: &Signals, session: &auth::Session) -> Result<(), String> {
    signals.libraries.with_untracked(|libs| {
        for lib in libs {
            if let Some(handle) = lib.loaded_handle() {
                handle.adopt_session(session.clone())?;
            }
        }
        Ok(())
    })
}

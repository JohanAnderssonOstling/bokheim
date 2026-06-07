use crate::components::create_button;
use crate::components::page_views::create_overlay_scrim;
use crate::page_navigation::Signals;
use crate::style::{BORDER_COLOR, INK_TEXT, SUBTLE_TEXT, WHITE_SURFACE};
use epub_svelte_lib::auth;
use epub_svelte_lib::library::sync_server_url;
use floem::event::EventPropagation;
use floem::prelude::{
    button, container, create_rw_signal, dyn_view, h_stack, label, text_input, v_stack, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith,
};
use floem::{IntoView, View};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthTab {
    Login,
    Signup,
}

/// A persistent top-right button. Shows "Sign in" when signed-out, or username
/// + "Log out" when signed-in.
pub fn account_button(signals: Signals, show_modal: RwSignal<bool>) -> impl View {
    let session_signal = signals.session;
    dyn_view(move || -> Box<dyn View> {
        match session_signal.get() {
            None => Box::new(
                button(label(|| "Sign in".to_string()))
                    .style(|s| s.height(32))
                    .on_click(move |_| {
                        show_modal.set(true);
                        EventPropagation::Stop
                    }),
            ),
            Some(s) => {
                let name = s.username.clone();
                let signals_for_click = signals.clone();
                Box::new(
                    h_stack((
                        label(move || name.clone()).style(|s| s.color(SUBTLE_TEXT).padding_right(8)),
                        button(label(|| "Log out".to_string())).style(|s| s.height(32)).on_click(move |_| {
                            on_logout(signals_for_click.clone());
                            EventPropagation::Stop
                        }),
                    ))
                    .style(|s| s.gap(8).items_center()),
                )
            }
        }
    })
    .style(|s| s.padding(8))
}

/// Login / signup modal overlay. Rendered alongside the home view; gated by
/// `show_modal`.
pub fn auth_modal(signals: Signals, show_modal: RwSignal<bool>) -> impl View {
    let tab = create_rw_signal(AuthTab::Login);
    let username_buf = create_rw_signal(String::new());
    let password_buf = create_rw_signal(String::new());
    let invite_buf = create_rw_signal(String::new());
    let error: RwSignal<Option<String>> = create_rw_signal(None);

    let login_tab_button = button(label(|| "Log in".to_string())).style(|s| s.height(30)).on_click(move |_| {
        tab.set(AuthTab::Login);
        error.set(None);
        EventPropagation::Stop
    });
    let signup_tab_button = button(label(|| "I have an invite".to_string())).style(|s| s.height(30)).on_click(move |_| {
        tab.set(AuthTab::Signup);
        error.set(None);
        EventPropagation::Stop
    });

    let invite_field = dyn_view(move || -> Box<dyn View> {
        if tab.get() == AuthTab::Signup {
            Box::new(field_row("Invite token", text_input(invite_buf)))
        } else {
            Box::new(container(label(|| String::new())))
        }
    });

    let signals_for_submit = signals.clone();
    let submit_button = button(label(move || match tab.get() {
        AuthTab::Login => "Log in".to_string(),
        AuthTab::Signup => "Sign up".to_string(),
    }))
    .style(|s| s.height(36))
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
                if let Err(e) = session.save() {
                    error.set(Some(format!("could not save session: {e}")));
                    return EventPropagation::Stop;
                }
                propagate_session(&signals_for_submit, &session);
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

    let cancel_button = button(label(|| "Cancel".to_string())).style(|s| s.height(36)).on_click(move |_| {
        show_modal.set(false);
        EventPropagation::Stop
    });

    let error_label = dyn_view(move || -> Box<dyn View> {
        match error.get() {
            Some(msg) => Box::new(label(move || msg.clone()).style(|s| s.color(floem::peniko::Color::rgb8(180, 30, 30)).padding_top(4))),
            None => Box::new(container(label(|| String::new()))),
        }
    });

    let panel = v_stack((
        label(|| "Account".to_string()).style(|s| s.font_size(20).color(INK_TEXT).padding_bottom(8)),
        h_stack((login_tab_button, signup_tab_button)).style(|s| s.gap(8).padding_bottom(12)),
        invite_field,
        field_row("Username", text_input(username_buf)),
        field_row("Password", text_input(password_buf)),
        error_label,
        h_stack((submit_button, cancel_button)).style(|s| s.gap(8).padding_top(12).justify_end()),
    ))
    .style(|s| {
        s.width(360)
            .padding(20)
            .gap(8)
            .background(WHITE_SURFACE)
            .border(1)
            .border_color(BORDER_COLOR)
            .border_radius(12)
    })
    .on_click(|_| EventPropagation::Stop);

    let centered = container(panel).style(|s| {
        s.absolute().inset(0).size_full().items_center().justify_center().z_index(101)
    });

    create_overlay_scrim(
        show_modal,
        centered.on_click(move |_| {
            show_modal.set(false);
            EventPropagation::Stop
        }),
    )
}

fn field_row(label_text: &'static str, input: impl View + 'static) -> impl View {
    v_stack((
        label(move || label_text.to_string()).style(|s| s.color(SUBTLE_TEXT).font_size(13)),
        input,
    ))
    .style(|s| s.gap(4).padding_vert(4))
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

fn propagate_session(signals: &Signals, session: &auth::Session) {
    signals.libraries.with_untracked(|libs| {
        for lib in libs {
            if let Some(handle) = lib.loaded_handle() {
                handle.set_session(session.clone());
            }
        }
    });
}

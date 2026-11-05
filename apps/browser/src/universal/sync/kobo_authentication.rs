//! Kobo account forms stay in a window overlay above the virtual keyboard.

use gpui::prelude::*;
use gpui::{Context, Deferred, Div, FocusHandle, Focusable, KeyDownEvent, MouseButton, Window, deferred, div, px};
use ui_components as components;

use super::SyncPage;
use crate::model::AccountFlow;
use crate::{FocusNextSetting, FocusPreviousSetting};

pub(super) struct AccountOverlay {
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    flow: Option<AccountFlow>,
}

impl AccountOverlay {
    pub(super) fn new(cx: &mut Context<SyncPage>) -> Self {
        Self { focus: cx.focus_handle(), previous_focus: None, flow: None }
    }
}

impl SyncPage {
    pub(super) fn update_account_overlay_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.access_open {
            if self.access_overlay.flow.is_none() {
                self.access_overlay.previous_focus = window.focused(cx);
            }
            if self.access_overlay.flow != Some(self.flow) {
                self.access_overlay.flow = Some(self.flow);
                self.account_input_focus(cx).focus(window, cx);
            }
        } else if self.access_overlay.flow.take().is_some() {
            // Successful authentication also closes the form asynchronously.
            // Do not steal focus if the user has already moved elsewhere.
            if self.access_overlay.focus.contains_focused(window, cx) {
                if let Some(previous) = self.access_overlay.previous_focus.take() {
                    previous.focus(window, cx);
                } else {
                    window.blur();
                }
            }
            self.access_overlay.previous_focus = None;
        }
    }

    fn account_input_focus(&self, cx: &gpui::App) -> FocusHandle {
        match self.flow {
            AccountFlow::SignIn => self.authentication.login_email.read(cx).focus_handle(cx),
            AccountFlow::Register | AccountFlow::RequestPasswordReset => self.authentication.email.read(cx).focus_handle(cx),
            AccountFlow::VerifyEmail => self.authentication.verification_pin.read(cx).focus_handle(cx),
            AccountFlow::ResetPassword => self.authentication.reset_token.read(cx).focus_handle(cx),
        }
    }

    fn focus_account_control(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if !self.access_overlay.focus.contains_focused(window, cx) {
            self.account_input_focus(cx).focus(window, cx);
            if backwards {
                // Walk this small form to its last available tab stop.
                let mut last = window.focused(cx);
                for _ in 0..32 {
                    window.focus_next(cx);
                    let next = window.focused(cx);
                    if !self.access_overlay.focus.contains_focused(window, cx) || next == last {
                        break;
                    }
                    last = next;
                }
                if let Some(last) = last {
                    last.focus(window, cx);
                }
            }
        }
    }

    pub(super) fn account_overlay(&self, form: Div, window: &Window, cx: &mut Context<Self>) -> Deferred {
        let theme = components::browser_theme(cx);
        let panel = components::modal_form_surface(theme)
            .id("kobo-account-overlay")
            .key_context("Dialog")
            .track_focus(&self.access_overlay.focus)
            .tab_group()
            .w(px(480.0))
            .max_h_full()
            .child(div().id("kobo-account-form").min_h_0().overflow_y_scroll().child(form));
        deferred(components::anchor_to_window(
            components::modal_overlay()
                .occlude()
                .on_action(cx.listener(|page, _: &FocusNextSetting, window, cx| page.focus_account_control(false, window, cx)))
                .on_action(cx.listener(|page, _: &FocusPreviousSetting, window, cx| page.focus_account_control(true, window, cx)))
                .on_key_down(cx.listener(|page, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        if !page.is_busy() {
                            page.close_account_access(cx);
                        }
                        cx.stop_propagation();
                    }
                }))
                .child(components::modal_scrim("kobo-account-scrim", theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(cx.listener(|page, _, _, cx| {
                    if !page.is_busy() {
                        page.close_account_access(cx);
                    }
                    cx.stop_propagation();
                })))
                .child(panel),
            window,
        ))
        .with_priority(3)
    }
}

//! Reader appearance and shortcuts share the reader's own Settings panel.

use ui_text::text;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Focusable, IntoElement, Window, div};
use ui_components::{self as components, KeyboardShortcutEditor, ShortcutPage};

struct SettingsTabs {
    keyboard: bool,
    editor: Entity<KeyboardShortcutEditor>,
}

pub(crate) fn with_keyboard_settings<C: 'static>(general: AnyElement, window: &mut Window, cx: &mut Context<C>) -> AnyElement {
    if components::uses_mobile_reader_controls(window) {
        return general;
    }
    let state = settings_state(window, cx);
    let keyboard = state.read(cx).keyboard;
    let editor = state.read(cx).editor.clone();
    let theme = components::browser_theme(cx);
    div().size_full().min_h_0().min_w_0().flex().flex_col()
        .when(keyboard, |panel| panel.key_context("ShortcutSettings"))
        .child(if keyboard { components::reader_settings_tab_body(theme).child(components::reader_settings_group(theme).child(editor)).into_any_element() } else { general })
        .into_any_element()
}

fn settings_state<C: 'static>(window: &mut Window, cx: &mut Context<C>) -> Entity<SettingsTabs> {
    window
        .use_keyed_state(("reader-settings-tabs", cx.entity_id()), cx, |_, cx| {
            let editor = cx.new(|cx| KeyboardShortcutEditor::new(ShortcutPage::Reader, cx));
            let state = cx.new(|_| SettingsTabs { keyboard: false, editor });
            let changes = cx.observe(&state, |_, _, cx| cx.notify());
            (state, changes)
        })
        .read(cx)
        .0
        .clone()
}

pub(crate) fn header<C: 'static>(window: &mut Window, cx: &mut Context<C>) -> AnyElement {
    let state = settings_state(window, cx);
    let keyboard = state.read(cx).keyboard;
    let theme = components::browser_theme(cx);
    let general_target = state.clone();
    let keyboard_target = state;
    components::row(0.0).flex_1().min_w_0().h_full()
        .child(components::base_button("reader-settings-general-tab").label(text!(common::general)).ghost().no_fill().flex_1().h_full().border_b_2().border_color(if !keyboard { theme.accent } else { theme.rule }).on_click(move |_, window, cx| {
            general_target.update(cx, |state, cx| {
                state.keyboard = false;
                state.editor.update(cx, |editor, cx| editor.cancel_capture(cx));
                cx.notify();
            });
            window.focus_next(cx);
        }))
        .child(components::base_button("reader-settings-keyboard-tab").label(text!(common::keyboard)).ghost().no_fill().flex_1().h_full().border_b_2().border_color(if keyboard { theme.accent } else { theme.rule }).on_click(move |_, window, cx| {
            keyboard_target.update(cx, |state, cx| {
                state.keyboard = true;
                state.editor.update(cx, |editor, cx| editor.cancel_capture(cx));
                state.editor.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            });
        }))
        .into_any_element()
}

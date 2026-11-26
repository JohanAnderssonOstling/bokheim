use super::*;
use gpui::{Div, Point};

impl SettingsPage {
    pub(crate) fn open_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.select_settings_tab(false, cx);
        self.selected_section.set(None);
        self.keyboard_visible.set(false);
        self.focus.focus(window, cx);
    }

    pub(super) fn settings_container(&self, content: impl IntoElement, desktop: bool, cx: &Context<Self>) -> Div {
        let theme = components::browser_theme(cx);
        let target = cx.entity();
        let tabs = components::settings_tabs("settings", self.keyboard_tab, theme, move |keyboard, _, cx| {
            target.update(cx, |page, cx| page.select_settings_tab(keyboard, cx));
        })
        .max_w(px(1080.0))
        .key_context("BrowserSettingsHeader");
        div()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .key_context(SETTINGS_KEY_CONTEXT)
            .track_focus(&self.focus)
            .tab_group()
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_action(cx.listener(Self::move_setting_row))
            .on_action(cx.listener(Self::adjust_setting_value))
            .on_key_down(cx.listener(Self::on_setting_row_key))
            .when(desktop, |page| page.child(div().flex_none().w_full().flex().justify_center().child(tabs)))
            .child(div().flex_1().min_h_0().child(components::browser_settings_page_with_scroll(&self.scroll).child(content)))
    }

    fn select_settings_tab(&mut self, keyboard: bool, cx: &mut Context<Self>) {
        self.keyboard_tab = keyboard;
        self.selected_section.set(None);
        self.keyboard_visible.set(false);
        self.keyboard.update(cx, |editor, cx| editor.cancel_capture(cx));
        self.scroll.set_offset(Point::default());
        cx.notify();
    }

    pub(super) fn keyboard_panel(&self) -> Div {
        components::browser_settings_content().child(self.keyboard.clone())
    }
}

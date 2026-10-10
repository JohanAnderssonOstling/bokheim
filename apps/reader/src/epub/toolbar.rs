//! EPUB-specific appearance controls in the shared sidebar.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Window};
use ui_components as components;

use crate::epub::ReaderView;
use crate::invalidation::{Component, notify};
use crate::settings::{ReaderSettings, SidebarTab};
use crate::shell::settings_navigation::{self, SettingsRow, adjacent_value};
use crate::shell::state_panel;
use crate::shell::toolbar::SidebarTarget;
use crate::{ReaderFontFamily, ReaderTextAlignment};

impl SidebarTarget for ReaderView {
    #[cfg(feature = "kobo")]
    fn brightness_controls(&self) -> gpui::Entity<crate::shell::brightness::BrightnessControls> {
        self.brightness.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.close_reader)(window, cx);
    }

    fn hide_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_navigation(window) {
            if self.sidebar.close() {
                notify(cx, Component::ReaderShell, "sidebar_closed");
            }
        } else {
            self.chrome.hide(cx);
        }
        self.focus_document(window, cx);
    }

    fn sidebar_focus(&self) -> gpui::FocusHandle {
        self.sidebar_focus.clone()
    }

    fn sidebar_tab(&self) -> SidebarTab {
        self.sidebar.effective_or_default(self.contents_available())
    }

    fn contents_available(&self) -> bool {
        true
    }

    fn select_sidebar_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        state_panel::select_tab(&mut self.sidebar, tab, cx);
        self.sidebar_focus.focus(window, cx);
        cx.notify();
    }

    fn toggle_sidebar_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        state_panel::toggle_tab(&mut self.sidebar, tab, true, cx);
        self.chrome.show(cx);
        if self.sidebar.is_visible() {
            self.focus_document(window, cx);
        }
        notify(cx, Component::ReaderShell, "mobile_sidebar_tab_changed");
    }

    fn focus_sidebar_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_tab() == SidebarTab::Contents {
            self.toc_tree.update(cx, |tree, cx| {
                if tree.selected_index().is_none() {
                    tree.set_selected_index(Some(0), cx);
                }
                tree.focus(window, cx);
            });
        } else {
            window.focus_next(cx);
        }
    }

    fn settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.handle_settings_key(key, window, cx)
    }

    fn search_is_open(&self) -> bool {
        self.search.query.is_some()
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::shell::input::CommandTarget::execute(self, &crate::ToggleSearch, window, cx);
    }
}

impl ReaderView {
    fn settings_rows() -> &'static [SettingsRow] {
        if cfg!(feature = "kobo") {
            &[SettingsRow::Typeface, SettingsRow::Alignment, SettingsRow::FontSize, SettingsRow::LineHeight, SettingsRow::SideMargins, SettingsRow::VerticalMargins]
        } else {
            &[
                SettingsRow::Theme,
                SettingsRow::Typeface,
                SettingsRow::Alignment,
                SettingsRow::FontSize,
                SettingsRow::LineHeight,
                SettingsRow::SideMargins,
                SettingsRow::VerticalMargins,
                SettingsRow::ColumnGap,
                SettingsRow::MinLineWidth,
                SettingsRow::MaxLineWidth,
            ]
        }
    }

    fn handle_settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let direction = match key {
            "up" | "left" => -1,
            "down" | "right" => 1,
            _ => return false,
        };
        if matches!(key, "up" | "down") {
            self.settings_navigation.move_row(Self::settings_rows(), direction);
        } else {
            let current = ReaderSettings::preferences(cx).clone();
            match self.settings_navigation.selected(Self::settings_rows()) {
                SettingsRow::Theme => settings_navigation::adjust_theme(direction, cx),
                SettingsRow::Typeface => self.set_reader_font_family(adjacent_value(&[ReaderFontFamily::Publisher, ReaderFontFamily::Serif], current.font_family, direction), cx),
                SettingsRow::Alignment => self.set_reader_text_alignment(adjacent_value(&[ReaderTextAlignment::Publisher, ReaderTextAlignment::Start, ReaderTextAlignment::Justify], current.text_alignment, direction), cx),
                SettingsRow::FontSize => self.adjust_font_size(direction as f32, cx),
                SettingsRow::LineHeight => self.adjust_line_height(direction as f32 * 0.1, cx),
                SettingsRow::SideMargins => self.adjust_horizontal_margin(direction as f32 * 8.0, cx),
                SettingsRow::VerticalMargins => self.adjust_vertical_margin(direction as f32 * 8.0, cx),
                SettingsRow::ColumnGap => self.adjust_min_column_gap(direction as f32 * 8.0, cx),
                SettingsRow::MinLineWidth => self.adjust_min_line_width_ch(direction as f64 * 5.0, window, cx),
                SettingsRow::MaxLineWidth => self.adjust_max_line_width_ch(direction as f64 * 5.0, window, cx),
                _ => return false,
            }
        }
        self.sidebar_focus.focus(window, cx);
        cx.notify();
        true
    }

    pub(super) fn render_settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let current = ReaderSettings::preferences(cx).clone();
        components::reader_settings_tab_body(theme)
            .when(!cfg!(feature = "kobo"), |body| body.child(self.settings_navigation.row(SettingsRow::Theme, Self::settings_rows(), crate::shell::theme_picker::reader_theme_picker(cx, theme), theme)))
            .child(
                components::reader_settings_group(theme)
                    .child(
                        components::reader_setting_row_smallcaps("Typeface", theme).map(|row| self.settings_navigation.row(SettingsRow::Typeface, Self::settings_rows(), row, theme)).child(components::reader_choice_button_group(
                            "reader-font-group",
                            [
                                components::reader_choice_button("reader-font-publisher", "Book", current.font_family == ReaderFontFamily::Publisher, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_font_family(ReaderFontFamily::Publisher, cx))),
                                components::reader_choice_button("reader-font-serif", "Serif", current.font_family == ReaderFontFamily::Serif, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_font_family(ReaderFontFamily::Serif, cx))),
                            ],
                        )),
                    )
                    .child(
                        components::reader_setting_row_smallcaps("Alignment", theme).map(|row| self.settings_navigation.row(SettingsRow::Alignment, Self::settings_rows(), row, theme)).child(components::reader_choice_button_group(
                            "reader-alignment-group",
                            [
                                components::reader_choice_button("reader-alignment-publisher", "Book", current.text_alignment == ReaderTextAlignment::Publisher, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_text_alignment(ReaderTextAlignment::Publisher, cx))),
                                components::reader_choice_button("reader-alignment-start", "Left", current.text_alignment == ReaderTextAlignment::Start, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_text_alignment(ReaderTextAlignment::Start, cx))),
                                components::reader_choice_button("reader-alignment-justify", "Justify", current.text_alignment == ReaderTextAlignment::Justify, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_text_alignment(ReaderTextAlignment::Justify, cx))),
                            ],
                        )),
                    ),
            )
            .child(
                components::reader_settings_group(theme)
                    .child(components::reader_setting_row("Font size").map(|row| self.settings_navigation.row(SettingsRow::FontSize, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("font-size-smaller", "−", false, theme).tooltip("Decrease font size (Alt+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(-1.0, cx))),
                        format!("{:.0}px", current.font_size),
                        components::settings_stepper_button("font-size-larger", "+", false, theme).tooltip("Increase font size (Alt++)").on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(1.0, cx))),
                        theme,
                    )))
                    .child(components::reader_setting_row("Line height").map(|row| self.settings_navigation.row(SettingsRow::LineHeight, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("line-height-smaller", "−", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_line_height(-0.1, cx))),
                        format!("{:.1}", current.line_height),
                        components::settings_stepper_button("line-height-larger", "+", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_line_height(0.1, cx))),
                        theme,
                    ))),
            )
            .child(
                components::reader_settings_group(theme)
                    .child(components::reader_setting_row("Side margins").map(|row| self.settings_navigation.row(SettingsRow::SideMargins, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("side-margins-smaller", "−", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_horizontal_margin(-8.0, cx))),
                        format!("{:.0}px", current.horizontal_margin_px),
                        components::settings_stepper_button("side-margins-larger", "+", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_horizontal_margin(8.0, cx))),
                        theme,
                    )))
                    .child(components::reader_setting_row("Top/bottom margins").map(|row| self.settings_navigation.row(SettingsRow::VerticalMargins, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("vertical-margins-smaller", "−", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_vertical_margin(-8.0, cx))),
                        format!("{:.0}px", current.vertical_margin_px),
                        components::settings_stepper_button("vertical-margins-larger", "+", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_vertical_margin(8.0, cx))),
                        theme,
                    )))
                    .when(!cfg!(feature = "kobo"), |section| {
                        section.child(components::reader_setting_row("Column gap").map(|row| self.settings_navigation.row(SettingsRow::ColumnGap, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                            components::settings_stepper_button("column-gap-smaller", "−", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_min_column_gap(-8.0, cx))),
                            format!("{:.0}px", current.min_column_gap_px),
                            components::settings_stepper_button("column-gap-larger", "+", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_min_column_gap(8.0, cx))),
                            theme,
                        )))
                    }),
            )
            .child(components::reader_settings_group(theme).when(!cfg!(feature = "kobo"), |section| {
                section
                    .child(components::reader_setting_row("Line width min").map(|row| self.settings_navigation.row(SettingsRow::MinLineWidth, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("line-width-min-smaller", "−", false, theme).on_click(cx.listener(|this, _, window, cx| this.adjust_min_line_width_ch(-5.0, window, cx))),
                        format!("{:.0}ch", current.min_line_width_ch),
                        components::settings_stepper_button("line-width-min-larger", "+", false, theme).on_click(cx.listener(|this, _, window, cx| this.adjust_min_line_width_ch(5.0, window, cx))),
                        theme,
                    )))
                    .child(components::reader_setting_row("Line width max").map(|row| self.settings_navigation.row(SettingsRow::MaxLineWidth, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("line-width-max-smaller", "−", false, theme).on_click(cx.listener(|this, _, window, cx| this.adjust_max_line_width_ch(-5.0, window, cx))),
                        format!("{:.0}ch", current.max_line_width_ch),
                        components::settings_stepper_button("line-width-max-larger", "+", false, theme).on_click(cx.listener(|this, _, window, cx| this.adjust_max_line_width_ch(5.0, window, cx))),
                        theme,
                    )))
            }))
            .into_any_element()
    }
}

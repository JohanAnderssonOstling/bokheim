//! EPUB-specific appearance controls in the shared toolbar.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Window};
use ui_components as components;

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::settings::ReaderSettings;
use crate::shell::toolbar::{self as shared_toolbar, ToolbarContent, ToolbarTarget};
use crate::{ReaderFontFamily, ReaderTextAlignment};

impl ToolbarTarget for ReaderView {
    #[cfg(feature = "kobo")]
    fn brightness_controls(&self) -> gpui::Entity<crate::shell::brightness::BrightnessControls> {
        self.brightness.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.close_reader)(window, cx);
    }

    fn toggle_sidebar(&mut self, _: bool, cx: &mut Context<Self>) {
        crate::shell::state_panel::toggle_sidebar(&mut self.sidebar, true, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "sidebar_visibility_changed");
    }

    fn search_is_open(&self) -> bool {
        self.search.query.is_some()
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::shell::input::CommandTarget::execute(self, &crate::ToggleSearch, window, cx);
    }

    fn has_selected_annotation(&self) -> bool {
        self.marks.selected().is_some()
    }

    fn delete_selected_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.marks.selected().map(|annotation| annotation.id.clone()) else { return };
        self.delete_annotation(id, cx);
        self.focus_document(window, cx);
    }
}

impl ReaderView {
    fn render_settings_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let current = ReaderSettings::preferences(cx).clone();
        components::reader_settings_dropdown(window.viewport_size().height, theme)
            .child(
                components::reader_settings_section("Typography", theme)
                    .child(
                        components::reader_setting_row("Typeface").child(components::reader_choice_button_group(
                            "reader-font-group",
                            [
                                components::reader_choice_button("reader-font-publisher", "Book", current.font_family == ReaderFontFamily::Publisher)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_font_family(ReaderFontFamily::Publisher, cx))),
                                components::reader_choice_button("reader-font-serif", "Serif", current.font_family == ReaderFontFamily::Serif).on_click(cx.listener(|this, _, _, cx| this.set_reader_font_family(ReaderFontFamily::Serif, cx))),
                            ],
                        )),
                    )
                    .child(
                        components::reader_setting_row("Alignment").child(components::reader_choice_button_group(
                            "reader-alignment-group",
                            [
                                components::reader_choice_button("reader-alignment-publisher", "Book", current.text_alignment == ReaderTextAlignment::Publisher)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_text_alignment(ReaderTextAlignment::Publisher, cx))),
                                components::reader_choice_button("reader-alignment-start", "Left", current.text_alignment == ReaderTextAlignment::Start)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_text_alignment(ReaderTextAlignment::Start, cx))),
                                components::reader_choice_button("reader-alignment-justify", "Justify", current.text_alignment == ReaderTextAlignment::Justify)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_reader_text_alignment(ReaderTextAlignment::Justify, cx))),
                            ],
                        )),
                    )
                    .child(components::reader_setting_row("Font size").child(components::reader_stepper_group(
                        components::reader_stepper_button("font-size-smaller", "−", theme).tooltip("Decrease font size (Alt+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(-1.0, cx))),
                        format!("{:.0}px", current.font_size),
                        components::reader_stepper_button("font-size-larger", "+", theme).tooltip("Increase font size (Alt++)").on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(1.0, cx))),
                        theme,
                    )))
                    .child(components::reader_setting_row("Line height").child(components::reader_stepper_group(
                        components::reader_stepper_button("line-height-smaller", "−", theme).on_click(cx.listener(|this, _, _, cx| this.adjust_line_height(-0.1, cx))),
                        format!("{:.1}", current.line_height),
                        components::reader_stepper_button("line-height-larger", "+", theme).on_click(cx.listener(|this, _, _, cx| this.adjust_line_height(0.1, cx))),
                        theme,
                    ))),
            )
            .child(
                components::reader_settings_section("Page layout", theme)
                    .when(!cfg!(feature = "kobo"), |section| {
                        section
                            .child(components::reader_setting_row("Column width").child(components::reader_stepper_group(
                                components::reader_stepper_button("column-width-smaller", "−", theme).tooltip("Decrease column width (Ctrl+Alt+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_column_width(-40.0, cx))),
                                format!("{:.0}px", current.column_width),
                                components::reader_stepper_button("column-width-larger", "+", theme).tooltip("Increase column width (Ctrl+Alt++)").on_click(cx.listener(|this, _, _, cx| this.adjust_column_width(40.0, cx))),
                                theme,
                            )))
                            .child(components::reader_setting_row("Fit columns").child(components::reader_stepper_group(
                                components::reader_stepper_button("fit-columns-fewer", "−", theme).tooltip("Fit fewer columns in the current reader width").on_click(cx.listener(|this, _, window, cx| {
                                    this.adjust_desired_column_count(-1, window, cx);
                                })),
                                self.desired_column_count.to_string(),
                                components::reader_stepper_button("fit-columns-more", "+", theme).tooltip("Fit more columns in the current reader width").on_click(cx.listener(|this, _, window, cx| {
                                    this.adjust_desired_column_count(1, window, cx);
                                })),
                                theme,
                            )))
                    })
                    .child(components::reader_setting_row("Zoom").child(components::reader_stepper_group(
                        components::reader_stepper_button("reader-zoom-out", "−", theme).tooltip("Zoom out (Ctrl+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_scale(-0.1, cx))),
                        format!("{:.1}x", current.scale),
                        components::reader_stepper_button("reader-zoom-in", "+", theme).tooltip("Zoom in (Ctrl++)").on_click(cx.listener(|this, _, _, cx| this.adjust_scale(0.1, cx))),
                        theme,
                    ))),
            )
            .into_any_element()
    }

    pub(super) fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let progress = self.progress.summary();
        let content = ToolbarContent::new(self.spine_title(), progress.label(), progress.tooltip(), self.document_focus(cx), self.render_settings_panel(window, cx));
        shared_toolbar::reader_toolbar(self, &cx.entity(), content, theme, window)
    }
}

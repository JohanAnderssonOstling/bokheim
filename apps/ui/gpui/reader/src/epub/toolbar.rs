//! The EPUB toolbar: navigation, sidebar, progress, and appearance controls.
//!
//! The toolbar owns a small projection of reader state and reports typed events
//! back to the reader. Keeping that projection in an entity gives it a clear
//! state and event boundary; GPUI may still dirty the ancestor render path when
//! the child changes.
//!
//! It owns everything it draws. The reader pushes a [`ToolbarModel`] into it
//! when something it displays changes, and the toolbar reports interaction
//! back as a [`ToolbarEvent`]. It deliberately holds no handle to the reader.

use gpui::prelude::*;
use gpui::{AnyElement, Context, EventEmitter, IntoElement, Render, Window};
use ui_components as components;

use crate::invalidation::{self, Component};
use crate::settings::ReaderPreferences;
use crate::{ReaderFontFamily, ReaderTextAlignment};

/// Everything the toolbar draws, pushed in by the reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ToolbarModel {
    /// Current spine/TOC item title; empty when the publication provides none.
    pub(crate) title: String,
    pub(crate) can_history_back: bool,
    pub(crate) can_history_forward: bool,
    /// How far through the book, for the toolbar's compact readout.
    pub(crate) progress: crate::epub::progress::ProgressSummary,
    /// How many columns the reader is currently fitting.
    pub(crate) column_count: u8,
    pub(crate) preferences: ReaderPreferences,
    /// Where the appearance popover should leave keyboard focus.
    ///
    /// The toolbar holds no handle on the reader by design, but a popover
    /// always focuses something when it opens, and the reader's navigation keys
    /// only fire while the document has focus. Carrying the handle here lets
    /// the popover keep focus on the document instead of taking it.
    pub(crate) document_focus: Option<gpui::FocusHandle>,
}

/// An interaction with the toolbar. The reader decides what each one means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ToolbarEvent {
    ReturnToLibrary,
    /// `-1` for back, `1` for forward.
    NavigateHistory(i8),
    ToggleSidebar,
    AdjustColumnWidth(f64),
    /// Fit one more or one fewer column in the current width.
    AdjustColumnCount(i8),
    AdjustFontSize(f32),
    AdjustScale(f64),
    AdjustLineHeight(f32),
    SetFontFamily(ReaderFontFamily),
    SetTextAlignment(ReaderTextAlignment),
}

#[derive(Default)]
pub(crate) struct ReaderToolbar {
    model: ToolbarModel,
}

impl EventEmitter<ToolbarEvent> for ReaderToolbar {}

impl ReaderToolbar {
    /// Replaces what the toolbar draws, repainting only on a real change.
    ///
    /// The reader may call this after any event; an update that changes
    /// nothing visible costs a comparison instead of a frame.
    pub(crate) fn set_model(&mut self, model: ToolbarModel, reason: &'static str, cx: &mut Context<Self>) {
        if self.model == model {
            return;
        }
        self.model = model;
        invalidation::notify(cx, Component::ReaderToolbar, reason);
    }

    fn render_settings_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let current = &self.model.preferences;
        components::reader_settings_dropdown(window.viewport_size().height, theme)
            .child(
                components::reader_settings_section("Typography", theme)
                    .child(
                        components::reader_setting_row("Typeface").child(components::reader_choice_button_group(
                            "reader-font-group",
                            [
                                components::reader_choice_button("reader-font-publisher", "Publisher", current.font_family == ReaderFontFamily::Publisher)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetFontFamily(ReaderFontFamily::Publisher)))),
                                components::reader_choice_button("reader-font-serif", "Serif", current.font_family == ReaderFontFamily::Serif)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetFontFamily(ReaderFontFamily::Serif)))),
                                components::reader_choice_button("reader-font-sans", "Sans serif", current.font_family == ReaderFontFamily::SansSerif)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetFontFamily(ReaderFontFamily::SansSerif)))),
                            ],
                        )),
                    )
                    .child(components::reader_setting_row("Font size").child(components::reader_stepper_group(
                        components::reader_stepper_button("font-size-smaller", "−", theme).tooltip("Decrease font size (Alt+-)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustFontSize(-1.0)))),
                        format!("{:.0}px", current.font_size),
                        components::reader_stepper_button("font-size-larger", "+", theme).tooltip("Increase font size (Alt++)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustFontSize(1.0)))),
                        theme,
                    )))
                    .child(components::reader_setting_row("Line height").child(components::reader_stepper_group(
                        components::reader_stepper_button("line-height-smaller", "−", theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustLineHeight(-0.1)))),
                        format!("{:.1}", current.line_height),
                        components::reader_stepper_button("line-height-larger", "+", theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustLineHeight(0.1)))),
                        theme,
                    )))
                    .child(
                        components::reader_setting_row("Alignment").child(components::reader_choice_button_group(
                            "reader-alignment-group",
                            [
                                components::reader_choice_button("reader-alignment-publisher", "Publisher", current.text_alignment == ReaderTextAlignment::Publisher)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetTextAlignment(ReaderTextAlignment::Publisher)))),
                                components::reader_choice_button("reader-alignment-start", "Left", current.text_alignment == ReaderTextAlignment::Start)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetTextAlignment(ReaderTextAlignment::Start)))),
                                components::reader_choice_button("reader-alignment-justify", "Justified", current.text_alignment == ReaderTextAlignment::Justify)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetTextAlignment(ReaderTextAlignment::Justify)))),
                            ],
                        )),
                    ),
            )
            .child(
                components::reader_settings_section("Page layout", theme)
                    .when(!cfg!(feature = "kobo"), |section| {
                        section
                            .child(components::reader_setting_row("Column width").child(components::reader_stepper_group(
                                components::reader_stepper_button("column-width-smaller", "−", theme).tooltip("Decrease column width (Ctrl+Alt+-)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnWidth(-40.0)))),
                                format!("{:.0}px", current.column_width),
                                components::reader_stepper_button("column-width-larger", "+", theme).tooltip("Increase column width (Ctrl+Alt++)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnWidth(40.0)))),
                                theme,
                            )))
                            .child(components::reader_setting_row("Fit columns").child(components::reader_stepper_group(
                                components::reader_stepper_button("fit-columns-fewer", "−", theme).tooltip("Fit fewer columns in the current reader width").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnCount(-1)))),
                                self.model.column_count.to_string(),
                                components::reader_stepper_button("fit-columns-more", "+", theme).tooltip("Fit more columns in the current reader width").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnCount(1)))),
                                theme,
                            )))
                    })
                    .child(components::reader_setting_row("Zoom").child(components::reader_stepper_group(
                        components::reader_stepper_button("reader-zoom-out", "−", theme).tooltip("Zoom out (Ctrl+-)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustScale(-0.1)))),
                        format!("{:.1}x", current.scale),
                        components::reader_stepper_button("reader-zoom-in", "+", theme).tooltip("Zoom in (Ctrl++)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustScale(0.1)))),
                        theme,
                    ))),
            )
            .into_any_element()
    }
}

impl Render for ReaderToolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        invalidation::record_render(Component::ReaderToolbar);
        let theme = components::browser_theme(cx);
        let settings_panel = self.render_settings_panel(window, cx);

        let left = gpui::div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(gpui::px(components::SPACE_SM))
            .child(components::reader_control_button_icon("reader-library", "Return to library", gpui_component::IconName::BookOpen, theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ReturnToLibrary))))
            .child(components::reader_control_button_icon("reader-sidebar", "Toggle contents and annotations", gpui_component::IconName::PanelLeftOpen, theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleSidebar))))
            // `Undo2`/`Redo2`, not arrows: these retrace footnote and link
            // jumps, and bare arrows among the paging keys imply page turns.
            .child(
                components::reader_control_button_icon_state("reader-history-back", "Back to where the last link was followed (Alt+←)", gpui_component::IconName::Undo2, self.model.can_history_back, theme)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::NavigateHistory(-1)))),
            )
            .child(
                components::reader_control_button_icon_state("reader-history-forward", "Forward again after going back (Alt+→)", gpui_component::IconName::Redo2, self.model.can_history_forward, theme)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::NavigateHistory(1)))),
            );

        let right = gpui::div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap(gpui::px(components::SPACE_SM))
            .child(components::reader_progress(self.model.progress.label(), self.model.progress.tooltip(), theme))
            .child(components::reader_settings_popover(self.model.document_focus.as_ref(), theme).child(settings_panel));

        components::reader_toolbar(theme).child(left).child(components::reader_toolbar_title(self.model.title.clone())).child(right)
    }
}

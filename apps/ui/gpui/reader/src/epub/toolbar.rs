//! The EPUB toolbar: navigation, sidebar, audio, and the appearance panel.
//!
//! The toolbar repaints far more often than the document — a page turn changes
//! the chapter title and the history buttons while the document layout is
//! untouched — so it is its own entity with its own invalidation.
//!
//! It owns everything it draws. The reader pushes a [`ToolbarModel`] into it
//! when something it displays changes, and the toolbar reports interaction
//! back as a [`ToolbarEvent`]. It holds no handle to the reader, so a repaint
//! here can never pull the reader's whole render along with it.

use gpui::prelude::*;
use gpui::{AnyElement, Context, EventEmitter, IntoElement, Render, Window};
use ui_components as components;

use crate::invalidation::{self, Component};
use crate::settings::ReaderPreferences;
use crate::{ReaderFontFamily, ReaderTextAlignment};

/// What the audio button offers for the open book.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AudioAffordance {
    /// The book has no narration and speech synthesis is unavailable.
    #[default]
    Unavailable,
    /// Speech synthesis is idle and can start.
    ReadAloud,
    /// Speech synthesis is running.
    Speaking,
    /// The book has narration, stopped.
    NarrationIdle,
    NarrationPlaying,
    NarrationPaused,
}

impl AudioAffordance {
    fn label(self) -> &'static str {
        match self {
            Self::Unavailable | Self::ReadAloud => "Read",
            Self::Speaking => "Stop",
            Self::NarrationIdle => "Audio",
            Self::NarrationPlaying => "Pause audio",
            Self::NarrationPaused => "Resume audio",
        }
    }

    fn is_available(self) -> bool {
        self != Self::Unavailable
    }

    fn icon(self) -> gpui_component::IconName {
        match self {
            Self::Speaking | Self::NarrationPlaying => gpui_component::IconName::CircleX,
            Self::Unavailable | Self::ReadAloud | Self::NarrationIdle | Self::NarrationPaused => gpui_component::IconName::BookOpen,
        }
    }
}

/// Everything the toolbar draws, pushed in by the reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ToolbarModel {
    /// Current spine/TOC item title; empty when the publication provides none.
    pub(crate) title: String,
    pub(crate) can_history_back: bool,
    pub(crate) can_history_forward: bool,
    pub(crate) audio: AudioAffordance,
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
    ToggleAudio,
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

    fn render_settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let current = &self.model.preferences;
        components::reader_settings_dropdown(theme)
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        invalidation::record_render(Component::ReaderToolbar);
        let theme = components::browser_theme(cx);
        let audio = self.model.audio;
        let settings_panel = self.render_settings_panel(cx);

        let left = gpui::div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(gpui::px(components::SPACE_SM))
            .child(components::reader_control_button_icon("reader-sidebar", "Toggle contents and annotations", gpui_component::IconName::PanelLeftOpen, theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleSidebar))))
            .when(cfg!(feature = "kobo"), |group| {
                group.child(components::reader_control_button_icon("reader-library", "Return to library", gpui_component::IconName::BookOpen, theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ReturnToLibrary))))
            })
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
            // Omitted rather than made transparent: an invisible button still
            // occupies the toolbar and still answers clicks.
            .children(audio.is_available().then(|| components::reader_control_button_icon("reader-audio", audio.label(), audio.icon(), theme).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleAudio)))))
            .child(components::reader_settings_popover(self.model.document_focus.as_ref(), theme).child(settings_panel));

        components::reader_toolbar(theme).child(left).child(components::reader_toolbar_title(self.model.title.clone())).child(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audio_button_names_what_pressing_it_will_do() {
        assert_eq!(AudioAffordance::ReadAloud.label(), "Read");
        assert_eq!(AudioAffordance::Speaking.label(), "Stop");
        assert_eq!(AudioAffordance::NarrationIdle.label(), "Audio");
        assert_eq!(AudioAffordance::NarrationPlaying.label(), "Pause audio");
        assert_eq!(AudioAffordance::NarrationPaused.label(), "Resume audio");
    }

    #[test]
    fn only_the_unavailable_affordance_hides_the_button() {
        assert!(!AudioAffordance::Unavailable.is_available());
        for affordance in [AudioAffordance::ReadAloud, AudioAffordance::Speaking, AudioAffordance::NarrationIdle, AudioAffordance::NarrationPlaying, AudioAffordance::NarrationPaused] {
            assert!(affordance.is_available(), "{affordance:?} must leave the button usable");
        }
    }
}

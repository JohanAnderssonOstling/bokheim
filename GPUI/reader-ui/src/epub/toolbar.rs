//! The EPUB toolbar: title, navigation, audio, and the appearance panel.
//!
//! The toolbar repaints far more often than the document — a page turn changes
//! the chapter title and the history buttons while the document layout is
//! untouched — so it is its own entity with its own invalidation.
//!
//! It owns everything it draws. The reader pushes a [`ToolbarModel`] into it
//! when something it displays changes, and the toolbar reports interaction
//! back as a [`ToolbarEvent`]. It holds no handle to the reader, so a repaint
//! here can never pull the reader's whole render along with it.

use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{AnyElement, Context, EventEmitter, IntoElement, Render, Window};

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
}

/// Everything the toolbar draws, pushed in by the reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ToolbarModel {
    /// Book title, with the current chapter appended when known.
    pub(crate) title: String,
    pub(crate) can_history_back: bool,
    pub(crate) can_history_forward: bool,
    pub(crate) audio: AudioAffordance,
    /// How many columns the reader is currently fitting.
    pub(crate) column_count: u8,
    pub(crate) preferences: ReaderPreferences,
}

/// An interaction with the toolbar. The reader decides what each one means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ToolbarEvent {
    ReturnToLibrary,
    /// `-1` for back, `1` for forward.
    NavigateHistory(i8),
    ToggleToc,
    ToggleAnnotations,
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
        let current = &self.model.preferences;
        components::reader_settings_dropdown()
            .child(components::reader_settings_title())
            .child(
                components::reader_setting_row("Column width")
                    .child(components::reader_square_button("column-width-smaller", "−").tooltip("Decrease column width (Ctrl+Alt+-)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnWidth(-40.0)))))
                    .child(components::reader_setting_value(format!("{:.0}px", current.column_width)))
                    .child(components::reader_square_button("column-width-larger", "+").tooltip("Increase column width (Ctrl+Alt++)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnWidth(40.0))))),
            )
            .child(
                components::reader_setting_row("Fit columns")
                    .child(components::reader_square_button("fit-columns-fewer", "−").tooltip("Fit fewer columns in the current reader width").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnCount(-1)))))
                    .child(components::reader_setting_value(self.model.column_count.to_string()))
                    .child(components::reader_square_button("fit-columns-more", "+").tooltip("Fit more columns in the current reader width").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustColumnCount(1))))),
            )
            .child(
                components::reader_setting_row("Font size")
                    .child(components::reader_square_button("font-size-smaller", "−").tooltip("Decrease font size (Alt+-)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustFontSize(-1.0)))))
                    .child(components::reader_setting_value(format!("{:.0}px", current.font_size)))
                    .child(components::reader_square_button("font-size-larger", "+").tooltip("Increase font size (Alt++)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustFontSize(1.0))))),
            )
            .child(
                components::reader_setting_row("Zoom")
                    .child(components::reader_square_button("reader-zoom-out", "−").tooltip("Zoom out (Ctrl+-)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustScale(-0.1)))))
                    .child(components::reader_setting_value(format!("{:.1}x", current.scale)))
                    .child(components::reader_square_button("reader-zoom-in", "+").tooltip("Zoom in (Ctrl++)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustScale(0.1))))),
            )
            .child(
                components::reader_setting_row("Line height")
                    .child(components::reader_square_button("line-height-smaller", "−").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustLineHeight(-0.1)))))
                    .child(components::reader_setting_value(format!("{:.1}", current.line_height)))
                    .child(components::reader_square_button("line-height-larger", "+").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::AdjustLineHeight(0.1))))),
            )
            .child(
                components::reader_setting_row("Typeface").child(components::reader_choice_button_group(
                    "reader-font-group",
                    [
                        components::reader_choice_button("reader-font-publisher", "Publisher", current.font_family == ReaderFontFamily::Publisher)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetFontFamily(ReaderFontFamily::Publisher)))),
                        components::reader_choice_button("reader-font-serif", "Serif", current.font_family == ReaderFontFamily::Serif).on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetFontFamily(ReaderFontFamily::Serif)))),
                        components::reader_choice_button("reader-font-sans", "Sans serif", current.font_family == ReaderFontFamily::SansSerif)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::SetFontFamily(ReaderFontFamily::SansSerif)))),
                    ],
                )),
            )
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
            )
            .into_any_element()
    }
}

impl Render for ReaderToolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        invalidation::record_render(Component::ReaderToolbar);
        let audio = self.model.audio;
        let settings_panel = self.render_settings_panel(cx);

        components::reader_toolbar()
            .when(cfg!(feature = "kobo"), |toolbar| toolbar.child(components::reader_control_button("reader-library", "Library").tooltip("Return to library").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ReturnToLibrary)))))
            .child(components::reader_control_button_state("reader-history-back", "←", self.model.can_history_back).tooltip("Link history back (Alt+←)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::NavigateHistory(-1)))))
            .child(
                components::reader_control_button_state("reader-history-forward", "→", self.model.can_history_forward).tooltip("Link history forward (Alt+→)").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::NavigateHistory(1)))),
            )
            .child(components::reader_control_button_icon("reader-toc", "Contents", gpui_component::IconName::BookOpen).tooltip("Toggle table of contents").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleToc))))
            .child(components::reader_control_button_icon("reader-state", "Annotations", gpui_component::IconName::File).tooltip("Annotations and bookmarks").on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleAnnotations))))
            .child(components::reader_title(self.model.title.clone()))
            .child(
                components::reader_control_button_state("reader-audio", audio.label(), audio.is_available())
                    .opacity(if audio.is_available() { 1.0 } else { 0.0 })
                    .tooltip("Read aloud or play book audio")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleAudio))),
            )
            .child(components::reader_settings_popover().child(settings_panel))
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

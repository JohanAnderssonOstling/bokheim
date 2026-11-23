//! EPUB-specific appearance controls in the shared sidebar.

use ui_text::text;

use gpui::prelude::*;
use std::{cell::Cell, rc::Rc};

use gpui::{AnyElement, App, Context, Window, div, font, px, rems};
use html_view_gpui::ReadingRulerStyle;
use ui_components as components;

use crate::epub::{DocumentNavigation, LoadState, ReaderView};
use crate::invalidation::{Component, notify};
use crate::settings::{CjkFontChoice, ReaderSettings, SidebarTab};
use crate::shell::reading_ruler::{self, ReadingRulerSettings};
use crate::shell::settings_navigation::{self, SettingsRow, adjacent_step, adjacent_value};
use crate::shell::state_panel::sidebar_tabs;
use crate::shell::toolbar::{SidebarTarget, document_sidebar_methods};
use crate::{ReaderFontFamily, ReaderTextAlignment};
use app_preferences::{
    FIRST_LINE_INDENT_MAX_EM, FONT_SIZE_MAX, FONT_SIZE_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, PARAGRAPH_SPACING_MAX_EM, PARAGRAPH_STEP_EM, READER_SPACING_MAX, READER_SPACING_MIN, ReaderFirstLineIndent, ReaderFontWeight, ReaderHyphenation,
    WORD_SPACING_MAX_PERCENT, WORD_SPACING_MIN_PERCENT, WORD_SPACING_STEP_PERCENT,
};

impl SidebarTarget for ReaderView {
    document_sidebar_methods!();
    fn control_click(&mut self, position: gpui::Point<gpui::Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_link_at_in_new_window(position, window, cx);
    }
    fn close_label(&self) -> &'static str {
        if self.book.is_transient() { text!(common::back) } else { text!(common::return_to_library) }
    }
    fn available_tabs(&self) -> &'static [SidebarTab] {
        if self.book.is_transient() { &[SidebarTab::Contents, SidebarTab::Search, SidebarTab::Settings] } else { sidebar_tabs(self.contents_available()) }
    }
    fn source_url(&self) -> Option<&str> {
        self.book.is_transient().then(|| self.book.document_uris().first().map(String::as_str)).flatten()
    }
    fn history_availability(&self) -> (bool, bool) {
        self.document_history.unwrap_or_else(|| self.history.availability())
    }

    fn navigate_history(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.document_history.is_some() {
            cx.emit(DocumentNavigation::History(forward));
            return;
        }
        let LoadState::Ready(renderer) = &self.load_state else { return };
        let renderer = renderer.clone();
        let Some(current) = renderer.read(cx).current_cfi() else { return };
        if let Some(target) = self.history.navigate(current, forward) {
            self.restore_document_position(target, window, cx);
        }
        cx.notify();
    }

    #[cfg(feature = "kobo")]
    fn device_controls(&self) -> Option<gpui::AnyView> {
        self.device_controls.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        self.tts.close();
        (self.close_reader)(window, cx);
    }

    fn hide_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if components::uses_mobile_reader_controls(window) {
            if self.sidebar.close(cx) {
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

    fn rail_focus(&self) -> gpui::FocusHandle {
        self.rail_focus.clone()
    }

    fn keyboard_visibility(&self) -> Rc<Cell<bool>> {
        self.settings_navigation.keyboard_visibility()
    }

    fn contents_available(&self) -> bool {
        true
    }

    fn settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.handle_settings_key(key, window, cx)
    }

    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    fn tts_mark(&self) -> Option<components::ReaderRailMark> {
        self.tts.available().then(|| self.tts.button_mark())
    }

    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    fn press_tts(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        crate::shell::tts::TtsReader::press_tts(self, cx);
    }
}

/// The stored value, accessible name, and preview for each segmented choice.
type Steps<T> = [(T, &'static str, &'static str); 3];

struct ChoiceRow {
    id: &'static str,
    row: SettingsRow,
    label: &'static str,
}

const TYPEFACES: [(ReaderFontFamily, &str, Option<&str>); 3] =
    [(ReaderFontFamily::Publisher, text!(reader::book), None), (ReaderFontFamily::Serif, text!(common::serif), Some("NV Garamond")), (ReaderFontFamily::SansSerif, text!(common::sans_serif), Some("Raleway"))];
const SYSTEM_FONT_PICKER: bool = !cfg!(target_arch = "wasm32");
const SPACING_STEP_PX: f32 = 8.0;
const ALIGNMENTS: Steps<ReaderTextAlignment> = [
    (ReaderTextAlignment::Publisher, text!(reader::book), components::READER_BOOK_CHOICE_ICON),
    (ReaderTextAlignment::Start, text!(reader::left), "icons/reader-settings/align-left.svg"),
    (ReaderTextAlignment::Justify, text!(reader::justify), "icons/reader-settings/align-justify.svg"),
];

/// Soft line-length targets in characters; columns expand to fill the viewport.
const LINE_LENGTHS: Steps<(f64, f64)> = [
    ((25.0, 50.0), text!(reader::short), "icons/reader-settings/line-length-short.svg"),
    ((30.0, 70.0), text!(reader::medium), "icons/reader-settings/line-length-medium.svg"),
    ((60.0, 85.0), text!(reader::long), "icons/reader-settings/line-length-long.svg"),
];

fn step_values<T: Copy>(steps: &Steps<T>) -> [T; 3] {
    steps.map(|(value, _, _)| value)
}

impl ReaderView {
    fn settings_rows(&self, cx: &App) -> Vec<SettingsRow> {
        let mut rows = vec![];
        if !cfg!(feature = "kobo") {
            rows.push(SettingsRow::Theme);
            if settings_navigation::DESKTOP_FULLSCREEN {
                rows.push(SettingsRow::Fullscreen);
            }
        }
        rows.push(SettingsRow::Typeface);
        if self.cjk_font.is_some() {
            rows.push(SettingsRow::CjkTypeface);
        }
        rows.push(SettingsRow::Alignment);
        rows.extend([SettingsRow::FontWeight, SettingsRow::Hyphenation, SettingsRow::FontSize, SettingsRow::WordSpacing]);
        rows.push(SettingsRow::LineLength);
        rows.extend([SettingsRow::LineHeightMode, SettingsRow::LineHeight, SettingsRow::ParagraphSpacingMode, SettingsRow::ParagraphSpacing, SettingsRow::FirstLineIndentMode, SettingsRow::FirstLineIndent]);
        rows.extend([SettingsRow::SideMargins, SettingsRow::VerticalMargins]);
        rows.push(SettingsRow::ColumnGap);
        reading_ruler::append_rows(&mut rows, ReaderSettings::preferences(cx).reading_ruler != ReadingRulerStyle::Off);
        rows
    }

    fn handle_settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let rows = self.settings_rows(cx);
        let direction = match key {
            "up" | "left" => -1,
            "down" | "right" => 1,
            _ => return false,
        };
        if matches!(key, "up" | "down") {
            self.settings_navigation.move_row(&rows, direction);
        } else {
            let current = ReaderSettings::preferences(cx).clone();
            match self.settings_navigation.selected(&rows) {
                SettingsRow::Theme => settings_navigation::adjust_theme(direction, cx),
                SettingsRow::Fullscreen => settings_navigation::set_fullscreen(direction > 0, window),
                SettingsRow::Typeface => {
                    if SYSTEM_FONT_PICKER && direction > 0 && (current.custom_fonts_active() || current.synchronized.font_family() == ReaderFontFamily::SansSerif) {
                        self.select_custom_fonts(window, cx);
                        return true;
                    }
                    let family = if current.custom_fonts_active() { ReaderFontFamily::SansSerif } else { adjacent_value(&TYPEFACES.map(|(family, _, _)| family), current.synchronized.font_family(), direction) };
                    self.set_reader_font_family(family, cx);
                }
                SettingsRow::CjkTypeface => {
                    if SYSTEM_FONT_PICKER && direction > 0 {
                        self.select_cjk_custom_font(window, cx);
                    } else {
                        self.set_cjk_font_choice(CjkFontChoice::Publisher, cx);
                    }
                }
                SettingsRow::Alignment => self.set_reader_text_alignment(adjacent_value(&step_values(&ALIGNMENTS), current.synchronized.text_alignment(), direction), cx),
                SettingsRow::FontSize => self.adjust_font_size(direction as f32, cx),
                SettingsRow::Hyphenation => self.set_hyphenation(adjacent_value(&ReaderHyphenation::ALL, current.synchronized.hyphenation(), direction), cx),
                SettingsRow::WordSpacing => self.adjust_word_spacing(direction as f32 * WORD_SPACING_STEP_PERCENT, cx),
                SettingsRow::FontWeight => self.set_font_weight(adjacent_value(&ReaderFontWeight::ALL, current.synchronized.font_weight(), direction), cx),
                SettingsRow::LineHeightMode => self.set_override_line_height(direction > 0, cx),
                SettingsRow::LineHeight => self.adjust_line_height(direction as f32 * 0.1, cx),
                SettingsRow::ParagraphSpacingMode => self.set_override_paragraph_spacing(direction > 0, cx),
                SettingsRow::ParagraphSpacing => self.adjust_paragraph_spacing(direction as f32 * PARAGRAPH_STEP_EM, cx),
                SettingsRow::FirstLineIndentMode => self.set_first_line_indent(adjacent_value(&ReaderFirstLineIndent::ALL, current.synchronized.first_line_indent(), direction), cx),
                SettingsRow::FirstLineIndent => self.adjust_first_line_indent(direction as f32 * PARAGRAPH_STEP_EM, cx),
                SettingsRow::ReadingRulerStyle => self.set_reading_ruler(adjacent_value(&[ReadingRulerStyle::Off, ReadingRulerStyle::Band, ReadingRulerStyle::Focus, ReadingRulerStyle::Underline], current.reading_ruler, direction), cx),
                SettingsRow::ReadingRulerLines => self.set_reading_ruler_lines(adjacent_step(&[1, 2, 3, 4, 5], current.reading_ruler_lines, direction), cx),
                SettingsRow::LineLength => self.set_line_width_range(adjacent_step(&step_values(&LINE_LENGTHS), (current.synchronized.min_line_width_ch(), current.synchronized.max_line_width_ch()), direction), cx),
                SettingsRow::SideMargins => self.set_horizontal_margin(current.synchronized.horizontal_margin_px() + direction as f32 * SPACING_STEP_PX, cx),
                SettingsRow::VerticalMargins => self.set_vertical_margin(current.synchronized.vertical_margin_px() + direction as f32 * SPACING_STEP_PX, cx),
                SettingsRow::ColumnGap => self.set_min_column_gap(current.synchronized.min_column_gap_px() + direction as f32 * SPACING_STEP_PX, cx),
                _ => return false,
            }
        }
        self.sidebar_focus.focus(window, cx);
        cx.notify();
        true
    }

    pub(super) fn render_settings_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.settings_rows(cx);
        let theme = components::browser_theme(cx);
        let current = ReaderSettings::preferences(cx).clone();
        let font_size = components::reader_setting_row(text!(reader::font_size), theme).map(|row| self.settings_navigation.row(SettingsRow::FontSize, &rows, row, theme)).child(components::reader_settings_stepper(
            components::settings_stepper_button("font-size-smaller", text!(reader::minus), current.synchronized.font_size_px() <= FONT_SIZE_MIN)
                .tooltip(components::configured_shortcut_tooltip(text!(reader::decrease_font_size), "reader.font_decrease", cx))
                .on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(-1.0, cx))),
            format!("{:.0}px", current.synchronized.font_size_px()),
            components::settings_stepper_button("font-size-larger", "+", current.synchronized.font_size_px() >= FONT_SIZE_MAX)
                .tooltip(components::configured_shortcut_tooltip(text!(reader::increase_font_size), "reader.font_increase", cx))
                .on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(1.0, cx))),
            theme,
        ));
        let line_length = self.choice_row(
            ChoiceRow { id: "reader-line-length", row: SettingsRow::LineLength, label: text!(reader::line_length) },
            &LINE_LENGTHS,
            (current.synchronized.min_line_width_ch(), current.synchronized.max_line_width_ch()),
            theme,
            cx,
            |this, range, _, cx| this.set_line_width_range(range, cx),
        );
        let typography = components::reader_settings_group(theme)
            .child(components::reader_settings_intrinsic_columns([
                div().min_w_0().flex().flex_col().gap(px(components::READER_SETTINGS_GAP)).child(self.typeface_row(current.synchronized.font_family(), theme, cx)).children(self.cjk_typeface_row(theme, cx)),
                self.choice_row(ChoiceRow { id: "reader-alignment", row: SettingsRow::Alignment, label: text!(reader::alignment) }, &ALIGNMENTS, current.synchronized.text_alignment(), theme, cx, |this, alignment, _, cx| {
                    this.set_reader_text_alignment(alignment, cx)
                }),
            ]))
            .child(components::reader_settings_intrinsic_columns([self.font_weight_settings(theme, cx), self.hyphenation_settings(theme, cx)]))
            .child(components::reader_settings_intrinsic_columns([font_size, self.word_spacing_settings(theme, cx), line_length]));
        let spacing = [self.line_height_settings(theme, cx), self.paragraph_spacing_settings(theme, cx), self.first_line_indent_settings(theme, cx)];
        let margins = [
            (SettingsRow::SideMargins, text!(reader::side_margins), "reader-side-margins-decrease", "reader-side-margins-increase", current.synchronized.horizontal_margin_px()),
            (SettingsRow::VerticalMargins, text!(reader::vertical_margins), "reader-vertical-margins-decrease", "reader-vertical-margins-increase", current.synchronized.vertical_margin_px()),
            (SettingsRow::ColumnGap, text!(reader::column_gap), "reader-column-gap-decrease", "reader-column-gap-increase", current.synchronized.min_column_gap_px()),
        ]
        .map(|(row, label, decrease, increase, value)| {
            let change = move |this: &Self, value, cx: &mut Context<Self>| match row {
                SettingsRow::SideMargins => this.set_horizontal_margin(value, cx),
                SettingsRow::VerticalMargins => this.set_vertical_margin(value, cx),
                SettingsRow::ColumnGap => this.set_min_column_gap(value, cx),
                _ => {}
            };
            components::reader_setting_row(label, theme).map(|content| self.settings_navigation.row(row, &rows, content, theme)).child(components::reader_settings_stepper(
                components::settings_stepper_button(decrease, text!(reader::minus), value <= READER_SPACING_MIN).on_click(cx.listener(move |this, _, _, cx| change(this, value - SPACING_STEP_PX, cx))),
                format!("{value:.0}px"),
                components::settings_stepper_button(increase, "+", value >= READER_SPACING_MAX).on_click(cx.listener(move |this, _, _, cx| change(this, value + SPACING_STEP_PX, cx))),
                theme,
            ))
        });
        components::reader_settings_tab_body(theme, window)
            .when(!cfg!(feature = "kobo"), |body| body.child(components::reader_settings_group(theme).child(self.settings_navigation.row(SettingsRow::Theme, &rows, crate::shell::theme_picker::reader_theme_picker(cx, theme), theme))))
            .children(self.settings_navigation.fullscreen_group(&rows, window, theme))
            .child(typography)
            .child(components::reader_settings_group(theme).child(components::reader_settings_intrinsic_columns(spacing.into_iter().chain(margins))))
            .child(self.reading_ruler_settings(current.reading_ruler, current.reading_ruler_lines, theme, cx))
            .into_any_element()
    }

    fn hyphenation_settings(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let selected = ReaderSettings::preferences(cx).synchronized.hyphenation();
        let buttons = ReaderHyphenation::ALL.into_iter().enumerate().map(|(index, mode)| {
            let button = if mode == ReaderHyphenation::Book {
                components::reader_book_choice(("reader-hyphenation", index), mode == selected, theme)
            } else {
                components::settings_choice_button(("reader-hyphenation", index), mode.label(), mode == selected, theme)
            };
            button.min_w(rems(components::settings_control_size_rem())).on_click(cx.listener(move |reader, _, _, cx| reader.set_hyphenation(mode, cx)))
        });
        let group = components::reader_setting_row(text!(reader::hyphenation), theme).child(components::reader_choice_button_group("reader-hyphenation", buttons, theme));
        self.settings_navigation.row(SettingsRow::Hyphenation, &rows, group, theme)
    }

    fn word_spacing_settings(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let percent = ReaderSettings::preferences(cx).synchronized.word_spacing_percent();
        let row = components::reader_setting_row(text!(reader::word_spacing), theme).child(components::reader_settings_stepper(
            components::settings_stepper_button("reader-word-spacing-decrease", text!(reader::minus), percent <= WORD_SPACING_MIN_PERCENT).on_click(cx.listener(|reader, _, _, cx| reader.adjust_word_spacing(-WORD_SPACING_STEP_PERCENT, cx))),
            format!(text!(reader::percent_rounded), percent = percent),
            components::settings_stepper_button("reader-word-spacing-increase", "+", percent >= WORD_SPACING_MAX_PERCENT).on_click(cx.listener(|reader, _, _, cx| reader.adjust_word_spacing(WORD_SPACING_STEP_PERCENT, cx))),
            theme,
        ));
        self.settings_navigation.row(SettingsRow::WordSpacing, &rows, row, theme)
    }

    fn font_weight_settings(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let selected = ReaderSettings::preferences(cx).synchronized.font_weight();
        let buttons = ReaderFontWeight::ALL.into_iter().enumerate().map(|(index, weight)| {
            components::settings_choice_button(("reader-font-weight", index), weight.label(), weight == selected, theme)
                .min_w(rems(components::settings_control_size_rem()))
                .on_click(cx.listener(move |reader, _, _, cx| reader.set_font_weight(weight, cx)))
        });
        let group = components::reader_setting_row(text!(reader::font_weight), theme).child(components::reader_choice_button_group("reader-font-weight", buttons, theme));
        self.settings_navigation.row(SettingsRow::FontWeight, &rows, group, theme)
    }

    fn line_height_settings(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let current = ReaderSettings::preferences(cx).clone();
        let selected = current.synchronized.override_line_height();
        let book = components::reader_book_choice("reader-line-spacing-book", !selected, theme).on_click(cx.listener(|this, _, _, cx| this.set_override_line_height(false, cx)));
        let value = format!(text!(reader::spacing_multiplier), current.synchronized.line_height());
        let custom = components::reader_numeric_tile(
            "reader-line-spacing-custom",
            components::settings_stepper_button("reader-line-height-decrease", text!(reader::minus), current.synchronized.line_height() <= LINE_HEIGHT_MIN).tooltip(text!(reader::decrease_line_spacing)).on_click(cx.listener(
                |this, _, _, cx| {
                    cx.stop_propagation();
                    this.adjust_line_height(-0.1, cx);
                },
            )),
            components::settings_stepper_value_button(("reader-line-spacing-custom", 0usize), value.clone(), theme)
                .debug_selector(|| "reader-line-spacing-custom-value".into())
                .aria_label(format!(text!(common::use_custom_value), value = value))
                .on_click(cx.listener(|this, _, _, cx| this.set_override_line_height(true, cx))),
            components::settings_stepper_button("reader-line-height-increase", "+", current.synchronized.line_height() >= LINE_HEIGHT_MAX).tooltip(text!(reader::increase_line_spacing)).on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.adjust_line_height(0.1, cx);
            })),
            selected,
            theme,
        )
        .debug_selector(|| "reader-line-spacing-custom".into())
        .aria_label(text!(reader::custom_line_spacing));
        let choices = components::reader_settings_control_row(
            "reader-line-spacing-mode",
            self.settings_navigation.row(SettingsRow::LineHeightMode, &rows, div().flex().child(book), theme),
            1,
            self.settings_navigation.row(SettingsRow::LineHeight, &rows, div().flex().child(custom), theme),
            theme,
        );
        components::reader_setting_row(text!(reader::line_spacing), theme).child(choices)
    }

    fn paragraph_spacing_settings(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let current = ReaderSettings::preferences(cx).clone();
        let selected = current.synchronized.override_paragraph_spacing();
        let book = components::reader_book_choice("reader-paragraph-spacing-book", !selected, theme).on_click(cx.listener(|this, _, _, cx| this.set_override_paragraph_spacing(false, cx)));
        let value = format!(text!(reader::em), current.synchronized.paragraph_spacing_em());
        let custom = components::reader_numeric_tile(
            "reader-paragraph-spacing-custom",
            components::settings_stepper_button("reader-paragraph-spacing-decrease", text!(reader::minus), current.synchronized.paragraph_spacing_em() <= 0.0).tooltip(text!(reader::decrease_paragraph_spacing)).on_click(cx.listener(
                |this, _, _, cx| {
                    cx.stop_propagation();
                    this.adjust_paragraph_spacing(-PARAGRAPH_STEP_EM, cx);
                },
            )),
            components::settings_stepper_value_button(("reader-paragraph-spacing-custom", 0usize), value.clone(), theme)
                .debug_selector(|| "reader-paragraph-spacing-custom-value".into())
                .aria_label(format!(text!(common::use_custom_value), value = value))
                .on_click(cx.listener(|this, _, _, cx| this.set_override_paragraph_spacing(true, cx))),
            components::settings_stepper_button("reader-paragraph-spacing-increase", "+", current.synchronized.paragraph_spacing_em() >= PARAGRAPH_SPACING_MAX_EM).tooltip(text!(reader::increase_paragraph_spacing)).on_click(cx.listener(
                |this, _, _, cx| {
                    cx.stop_propagation();
                    this.adjust_paragraph_spacing(PARAGRAPH_STEP_EM, cx);
                },
            )),
            selected,
            theme,
        )
        .debug_selector(|| "reader-paragraph-spacing-custom".into())
        .aria_label(text!(reader::custom_paragraph_spacing));
        let choices = components::reader_settings_control_row(
            "reader-paragraph-spacing-mode",
            self.settings_navigation.row(SettingsRow::ParagraphSpacingMode, &rows, div().flex().child(book), theme),
            1,
            self.settings_navigation.row(SettingsRow::ParagraphSpacing, &rows, div().flex().child(custom), theme),
            theme,
        );
        components::reader_setting_row(text!(reader::paragraph_spacing), theme).child(choices)
    }

    fn first_line_indent_settings(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let current = ReaderSettings::preferences(cx).clone();
        let selected = current.synchronized.first_line_indent() == ReaderFirstLineIndent::Custom;
        let buttons = [ReaderFirstLineIndent::Book, ReaderFirstLineIndent::Off].into_iter().enumerate().map(|(index, mode)| {
            let selected = current.synchronized.first_line_indent() == mode;
            let button = if mode == ReaderFirstLineIndent::Book {
                components::reader_book_choice(("reader-first-line-indent-mode", index), selected, theme)
            } else {
                components::settings_choice_button(("reader-first-line-indent-mode", index), mode.label(), selected, theme)
            };
            button.on_click(cx.listener(move |this, _, _, cx| this.set_first_line_indent(mode, cx)))
        });
        let preset_count = buttons.len() as u16;
        let modes = components::reader_choice_button_group("reader-first-line-indent-presets", buttons, theme).h_full().border_0();
        let value = format!(text!(reader::em), current.synchronized.first_line_indent_em());
        let custom = components::reader_numeric_tile(
            "reader-first-line-indent-custom",
            components::settings_stepper_button("reader-first-line-indent-decrease", text!(reader::minus), current.synchronized.first_line_indent_em() <= 0.0).tooltip(text!(reader::decrease_first_line_indentation)).on_click(cx.listener(
                |this, _, _, cx| {
                    cx.stop_propagation();
                    this.adjust_first_line_indent(-PARAGRAPH_STEP_EM, cx);
                },
            )),
            components::settings_stepper_value_button(("reader-first-line-indent-custom", 0usize), value.clone(), theme)
                .debug_selector(|| "reader-first-line-indent-custom-value".into())
                .aria_label(format!(text!(common::use_custom_value), value = value))
                .on_click(cx.listener(|this, _, _, cx| this.set_first_line_indent(ReaderFirstLineIndent::Custom, cx))),
            components::settings_stepper_button("reader-first-line-indent-increase", "+", current.synchronized.first_line_indent_em() >= FIRST_LINE_INDENT_MAX_EM).tooltip(text!(reader::increase_first_line_indentation)).on_click(
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.adjust_first_line_indent(PARAGRAPH_STEP_EM, cx);
                }),
            ),
            selected,
            theme,
        )
        .debug_selector(|| "reader-first-line-indent-custom".into())
        .aria_label(text!(reader::custom_first_line_indentation));
        let choices = components::reader_settings_control_row(
            "reader-first-line-indent-mode",
            self.settings_navigation.row(SettingsRow::FirstLineIndentMode, &rows, div().flex().child(modes), theme),
            preset_count,
            self.settings_navigation.row(SettingsRow::FirstLineIndent, &rows, div().flex().child(custom), theme),
            theme,
        );
        components::reader_setting_row(text!(reader::first_line_indentation), theme).child(choices)
    }

    fn reading_ruler_settings(&self, current: ReadingRulerStyle, lines: usize, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        reading_ruler::render(
            ReadingRulerSettings {
                current,
                off: ReadingRulerStyle::Off,
                styles: [ReadingRulerStyle::Band, ReadingRulerStyle::Focus, ReadingRulerStyle::Underline],
                lines,
                set_enabled: Self::set_reading_ruler_enabled,
                set_style: Self::set_reading_ruler,
                set_lines: Self::set_reading_ruler_lines,
            },
            &self.settings_navigation,
            &rows,
            theme,
            cx,
        )
    }

    /// A labelled segmented row with a preview for each choice. A stored value
    /// that matches no choice, such as one kept from an older version, simply
    /// selects none until a choice is selected.
    fn choice_row<T: Copy + PartialEq + 'static>(&self, row: ChoiceRow, steps: &Steps<T>, current: T, theme: components::BrowserTheme, cx: &mut Context<Self>, choose: fn(&mut Self, T, &mut Window, &mut Context<Self>)) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let ChoiceRow { id, row, label } = row;
        let buttons = steps.iter().enumerate().map(|(index, &(value, name, picture))| {
            components::reader_picture_choice((id, index), components::reader_tile_picture(picture), name, value == current, theme).on_click(cx.listener(move |this, _, window, cx| choose(this, value, window, cx)))
        });
        components::reader_setting_row(label, theme).map(|element| self.settings_navigation.row(row, &rows, element, theme)).child(components::reader_choice_button_group(id, buttons, theme))
    }

    /// Main typeface choices keep their sample beside the label.
    fn typeface_row(&self, current: ReaderFontFamily, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.settings_rows(cx);
        let preferences = ReaderSettings::preferences(cx).clone();
        let custom = preferences.custom_fonts_active();
        let mut buttons: Vec<_> = TYPEFACES
            .iter()
            .enumerate()
            .map(|(index, &(family, name, face))| {
                if let Some(face) = face {
                    components::ensure_bundled_font(face, cx);
                }
                let button = if family == ReaderFontFamily::Publisher {
                    components::reader_book_choice(("reader-typeface", index), !custom && family == current, theme)
                } else {
                    components::reader_picture_choice(("reader-typeface", index), components::reader_typeface_sample(face), name, !custom && family == current, theme)
                };
                button.on_click(cx.listener(move |this, _, _, cx| this.set_reader_font_family(family, cx)))
            })
            .collect();
        if SYSTEM_FONT_PICKER {
            buttons.push(components::settings_choice_button("reader-custom-fonts", text!(common::custom), custom, theme).on_click(cx.listener(|this, _, window, cx| this.select_custom_fonts(window, cx))));
        }
        components::reader_setting_row(if self.cjk_font.is_some() { text!(reader::latin_typeface) } else { text!(reader::typeface) }, theme)
            .map(|element| self.settings_navigation.row(SettingsRow::Typeface, &rows, element, theme))
            .child(components::reader_choice_button_group("reader-typeface", buttons, theme))
            .when(SYSTEM_FONT_PICKER, |row| row.child(self.font_picker.clone()))
    }
    fn cjk_typeface_row(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let rows = self.settings_rows(cx);
        let preferences = ReaderSettings::preferences(cx).clone();
        let cjk = self.cjk_font?;
        components::ensure_text_fonts(cjk.sample(), cx);
        let sample = || div().flex_none().flex().items_center().whitespace_nowrap().text_size(rems(1.25)).font_family(cjk.family()).child(cjk.sample());
        let system_family = preferences.saved_cjk_font_family().map(str::to_owned);
        let custom_sample = sample().when_some(system_family.clone().filter(|family| cx.text_system().font_id(&font(family.clone())).is_ok()), |sample, family| sample.font_family(family));
        let has_custom = system_family.is_some();
        let custom_label = system_family.unwrap_or_else(|| text!(common::custom).into());
        let mut buttons =
            vec![components::reader_book_choice("reader-cjk-publisher", preferences.cjk_font_choice == CjkFontChoice::Publisher, theme).on_click(cx.listener(|this, _, _, cx| this.set_cjk_font_choice(CjkFontChoice::Publisher, cx)))];
        if SYSTEM_FONT_PICKER {
            buttons.push(
                components::reader_picture_choice("reader-cjk-system", custom_sample, custom_label, preferences.cjk_font_choice == CjkFontChoice::Custom, theme)
                    .track_focus(self.cjk_font_picker.read(cx).cjk_focus())
                    .on_click(cx.listener(|this, _, window, cx| this.select_cjk_custom_font(window, cx)))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.select_cjk_custom_font(window, cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }
        Some(
            components::reader_setting_row(text!(reader::cjk_typeface), theme)
                .map(|element| self.settings_navigation.row(SettingsRow::CjkTypeface, &rows, element, theme))
                .child(components::reader_choice_button_group("reader-cjk-typeface", buttons, theme))
                .when(SYSTEM_FONT_PICKER && has_custom && preferences.cjk_font_choice == CjkFontChoice::Custom, |row| {
                    row.child(components::outlined_text_button("reader-cjk-change-font", text!(reader::change_font)).on_click(cx.listener(|this, _, window, cx| this.open_cjk_font_picker(window, cx))))
                })
                .when(SYSTEM_FONT_PICKER, |row| row.child(self.cjk_font_picker.clone())),
        )
    }
}

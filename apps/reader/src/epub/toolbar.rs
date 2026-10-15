//! EPUB-specific appearance controls in the shared sidebar.

use gpui::prelude::*;
use std::{cell::Cell, rc::Rc};

use gpui::{AnyElement, Context, Focusable, Window};
use ui_components as components;

use crate::epub::ReaderView;
use crate::invalidation::{Component, notify};
use crate::settings::{ReaderSettings, SidebarTab};
use crate::shell::settings_navigation::{self, SettingsRow, adjacent_step, adjacent_value};
use crate::shell::state_panel;
use crate::shell::toolbar::SidebarTarget;
use crate::{ReaderFontFamily, ReaderTextAlignment};
use app_preferences::{DEFAULT_COLUMN_GAP_PX, DEFAULT_READER_MARGIN_PX};

impl SidebarTarget for ReaderView {
    #[cfg(feature = "kobo")]
    fn brightness_controls(&self) -> gpui::Entity<crate::shell::brightness::BrightnessControls> {
        self.brightness.clone()
    }

    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(all(feature = "native-tts", not(feature = "kobo"), not(target_arch = "wasm32")))]
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

    fn sidebar_tab(&self) -> SidebarTab {
        self.sidebar.effective_or_default(self.contents_available())
    }

    fn contents_available(&self) -> bool {
        true
    }

    fn select_sidebar_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        state_panel::select_tab(&mut self.sidebar, tab, cx);
        self.chrome.show(cx);
        if tab == SidebarTab::Search {
            self.open_search(cx);
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.focus_sidebar_body(window, cx);
        cx.notify();
    }

    fn toggle_sidebar_tab(&mut self, tab: SidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        state_panel::toggle_tab(&mut self.sidebar, tab, true, cx);
        self.chrome.show(cx);
        if self.sidebar.is_visible() && tab == SidebarTab::Search {
            self.open_search(cx);
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        } else if self.sidebar.is_visible() {
            self.focus_sidebar_body(window, cx);
        } else {
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
        } else if self.sidebar_tab() == SidebarTab::Search {
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.sidebar_focus.focus(window, cx);
            if self.sidebar_tab() == SidebarTab::Notes {
                window.focus_next(cx);
                if !self.sidebar_focus.contains_focused(window, cx) {
                    self.sidebar_focus.focus(window, cx);
                }
            }
        }
    }

    fn settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.handle_settings_key(key, window, cx)
    }

    #[cfg(all(feature = "native-tts", not(feature = "kobo"), not(target_arch = "wasm32")))]
    fn tts_mark(&self) -> Option<components::ReaderRailMark> {
        Some(self.tts.button_mark())
    }

    #[cfg(all(feature = "native-tts", not(feature = "kobo"), not(target_arch = "wasm32")))]
    fn press_tts(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        crate::shell::tts::TtsReader::press_tts(self, cx);
    }
}

/// The values a settings tile stores, its name, and its picture. The middle
/// tile of every row is the preference's default.
type Steps<T> = [(T, &'static str, &'static str); 3];

const TYPEFACES: [(ReaderFontFamily, &str, Option<&str>); 3] = [(ReaderFontFamily::Publisher, "Book", None), (ReaderFontFamily::Serif, "Serif", Some("NV Garamond")), (ReaderFontFamily::SansSerif, "Sans serif", Some("Raleway"))];
const ALIGNMENTS: Steps<ReaderTextAlignment> = [
    (ReaderTextAlignment::Publisher, "Book", "icons/reader-settings/align-book.svg"),
    (ReaderTextAlignment::Start, "Left", "icons/reader-settings/align-left.svg"),
    (ReaderTextAlignment::Justify, "Justify", "icons/reader-settings/align-justify.svg"),
];
const LINE_SPACINGS: Steps<f32> = [(1.3, "Tight", "icons/reader-settings/line-spacing-tight.svg"), (1.5, "Normal", "icons/reader-settings/line-spacing-normal.svg"), (1.8, "Loose", "icons/reader-settings/line-spacing-loose.svg")];
/// Minimum and maximum line length in characters.
const LINE_LENGTHS: Steps<(f64, f64)> =
    [((35.0, 50.0), "Short", "icons/reader-settings/line-length-short.svg"), ((45.0, 65.0), "Medium", "icons/reader-settings/line-length-medium.svg"), ((60.0, 85.0), "Long", "icons/reader-settings/line-length-long.svg")];
const SIDE_MARGINS: Steps<f32> =
    [(8.0, "Narrow", "icons/reader-settings/side-margins-narrow.svg"), (DEFAULT_READER_MARGIN_PX, "Medium", "icons/reader-settings/side-margins-medium.svg"), (64.0, "Wide", "icons/reader-settings/side-margins-wide.svg")];
const VERTICAL_MARGINS: Steps<f32> =
    [(8.0, "Narrow", "icons/reader-settings/vertical-margins-narrow.svg"), (DEFAULT_READER_MARGIN_PX, "Medium", "icons/reader-settings/vertical-margins-medium.svg"), (48.0, "Wide", "icons/reader-settings/vertical-margins-wide.svg")];
const COLUMN_GAPS: Steps<f32> =
    [(16.0, "Narrow", "icons/reader-settings/column-gap-narrow.svg"), (DEFAULT_COLUMN_GAP_PX, "Medium", "icons/reader-settings/column-gap-medium.svg"), (64.0, "Wide", "icons/reader-settings/column-gap-wide.svg")];

fn step_values<T: Copy>(steps: &Steps<T>) -> [T; 3] {
    steps.map(|(value, _, _)| value)
}

impl ReaderView {
    fn settings_rows() -> &'static [SettingsRow] {
        if cfg!(feature = "kobo") {
            &[SettingsRow::FontSize, SettingsRow::Typeface, SettingsRow::Alignment, SettingsRow::LineHeight, SettingsRow::SideMargins, SettingsRow::VerticalMargins]
        } else {
            &[SettingsRow::Theme, SettingsRow::FontSize, SettingsRow::Typeface, SettingsRow::Alignment, SettingsRow::LineHeight, SettingsRow::LineLength, SettingsRow::SideMargins, SettingsRow::VerticalMargins, SettingsRow::ColumnGap]
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
                SettingsRow::Typeface => self.set_reader_font_family(adjacent_value(&TYPEFACES.map(|(family, _, _)| family), current.font_family, direction), cx),
                SettingsRow::Alignment => self.set_reader_text_alignment(adjacent_value(&step_values(&ALIGNMENTS), current.text_alignment, direction), cx),
                SettingsRow::FontSize => self.adjust_font_size(direction as f32, cx),
                SettingsRow::LineHeight => self.set_line_height(adjacent_step(&step_values(&LINE_SPACINGS), current.line_height, direction), cx),
                SettingsRow::LineLength => self.set_line_width_range(adjacent_step(&step_values(&LINE_LENGTHS), (current.min_line_width_ch, current.max_line_width_ch), direction), window, cx),
                SettingsRow::SideMargins => self.set_horizontal_margin(adjacent_step(&step_values(&SIDE_MARGINS), current.horizontal_margin_px, direction), cx),
                SettingsRow::VerticalMargins => self.set_vertical_margin(adjacent_step(&step_values(&VERTICAL_MARGINS), current.vertical_margin_px, direction), cx),
                SettingsRow::ColumnGap => self.set_min_column_gap(adjacent_step(&step_values(&COLUMN_GAPS), current.min_column_gap_px, direction), cx),
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
            .when(!cfg!(feature = "kobo"), |body| {
                body.child(components::reader_settings_group(theme).child(self.settings_navigation.row(SettingsRow::Theme, Self::settings_rows(), crate::shell::theme_picker::reader_theme_picker(cx, theme), theme)))
            })
            .child(
                components::reader_settings_group(theme)
                    .child(components::reader_setting_row("Font size").map(|row| self.settings_navigation.row(SettingsRow::FontSize, Self::settings_rows(), row, theme)).child(components::settings_stepper(
                        components::settings_stepper_button("font-size-smaller", "−", false, theme).tooltip("Decrease font size (Alt+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(-1.0, cx))),
                        format!("{:.0}px", current.font_size),
                        components::settings_stepper_button("font-size-larger", "+", false, theme).tooltip("Increase font size (Alt++)").on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(1.0, cx))),
                        theme,
                    )))
                    .child(self.typeface_row(current.font_family, theme, cx))
                    .child(self.tile_row("reader-alignment", SettingsRow::Alignment, "Alignment", &ALIGNMENTS, current.text_alignment, theme, cx, |this, alignment, _, cx| this.set_reader_text_alignment(alignment, cx))),
            )
            .child(
                components::reader_settings_group(theme)
                    .child(self.tile_row("reader-line-spacing", SettingsRow::LineHeight, "Line spacing", &LINE_SPACINGS, current.line_height, theme, cx, |this, line_height, _, cx| this.set_line_height(line_height, cx)))
                    .when(!cfg!(feature = "kobo"), |group| {
                        group.child(self.tile_row("reader-line-length", SettingsRow::LineLength, "Line length", &LINE_LENGTHS, (current.min_line_width_ch, current.max_line_width_ch), theme, cx, |this, range, window, cx| {
                            this.set_line_width_range(range, window, cx)
                        }))
                    }),
            )
            .child(
                components::reader_settings_group(theme)
                    .child(self.tile_row("reader-side-margins", SettingsRow::SideMargins, "Side margins", &SIDE_MARGINS, current.horizontal_margin_px, theme, cx, |this, margin, _, cx| this.set_horizontal_margin(margin, cx)))
                    .child(self.tile_row("reader-vertical-margins", SettingsRow::VerticalMargins, "Vertical margins", &VERTICAL_MARGINS, current.vertical_margin_px, theme, cx, |this, margin, _, cx| this.set_vertical_margin(margin, cx)))
                    .when(!cfg!(feature = "kobo"), |group| {
                        group.child(self.tile_row("reader-column-gap", SettingsRow::ColumnGap, "Column gap", &COLUMN_GAPS, current.min_column_gap_px, theme, cx, |this, gap, _, cx| this.set_min_column_gap(gap, cx)))
                    }),
            )
            .into_any_element()
    }

    /// A labelled row of three picture tiles for one setting. A stored value
    /// that matches no tile, such as one kept from an older version, simply
    /// selects none until a tile is chosen.
    #[allow(clippy::too_many_arguments)]
    fn tile_row<T: Copy + PartialEq + 'static>(
        &self, id: &'static str, row: SettingsRow, label: &'static str, steps: &Steps<T>, current: T, theme: components::BrowserTheme, cx: &mut Context<Self>, choose: fn(&mut Self, T, &mut Window, &mut Context<Self>),
    ) -> gpui::Div {
        let tiles = steps.iter().enumerate().map(|(index, &(value, name, picture))| {
            components::reader_picture_tile((id, index), components::reader_tile_picture(picture), name, value == current, theme).on_click(cx.listener(move |this, _, window, cx| choose(this, value, window, cx)))
        });
        components::reader_setting_row_smallcaps(label, theme).map(|element| self.settings_navigation.row(row, Self::settings_rows(), element, theme)).child(components::reader_tile_group(id).children(tiles))
    }

    /// Typeface tiles show "Ag" set in the face they select.
    fn typeface_row(&self, current: ReaderFontFamily, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let tiles = TYPEFACES.iter().enumerate().map(|(index, &(family, name, face))| {
            components::reader_picture_tile(("reader-typeface", index), components::reader_typeface_sample(face), name, family == current, theme).on_click(cx.listener(move |this, _, _, cx| this.set_reader_font_family(family, cx)))
        });
        components::reader_setting_row_smallcaps("Typeface", theme)
            .map(|element| self.settings_navigation.row(SettingsRow::Typeface, Self::settings_rows(), element, theme))
            .child(components::reader_tile_group("reader-typeface").children(tiles))
    }
}

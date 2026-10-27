//! PDF appearance controls and their persisted settings.

use crate::invalidation::notify;
use gpui::prelude::*;
use gpui::{Context, Window};
use pdf_view_gpui::{MAX_VISIBLE_PAGE_COUNT, MIN_VISIBLE_PAGE_COUNT, PdfZoomMode};
use ui_components as components;

use super::{PdfReaderView, ZOOM_STEP};
use crate::invalidation::Component;
use crate::pdf::state::store_zoom_mode;
use crate::settings::{PdfZoomModePreference, ReaderSettings};
use crate::shell::settings_navigation::{self, SettingsRow, adjacent_value};

impl PdfReaderView {
    fn settings_rows(&self, cx: &gpui::App) -> Vec<SettingsRow> {
        let mut rows = Vec::new();
        if !cfg!(feature = "kobo") {
            rows.push(SettingsRow::Theme);
        }
        if settings_navigation::DESKTOP_FULLSCREEN {
            rows.push(SettingsRow::Fullscreen);
        }
        rows.extend([SettingsRow::PdfFit, SettingsRow::PdfMargins]);
        match self.pdf.read(cx).zoom_mode() {
            PdfZoomMode::FitWidth { .. } => rows.push(SettingsRow::PdfPages),
            PdfZoomMode::Fixed { .. } => rows.push(SettingsRow::PdfZoom),
            PdfZoomMode::FitHeight => {}
        }
        rows
    }

    pub(super) fn handle_settings_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let direction = match key {
            "up" | "left" => -1,
            "down" | "right" => 1,
            _ => return false,
        };
        let rows = self.settings_rows(cx);
        if matches!(key, "up" | "down") {
            self.settings_navigation.move_row(&rows, direction);
        } else {
            match self.settings_navigation.selected(&rows) {
                SettingsRow::Theme => settings_navigation::adjust_theme(direction, cx),
                SettingsRow::Fullscreen => settings_navigation::set_fullscreen(direction > 0, window),
                SettingsRow::PdfMargins => self.set_trim_margins(direction > 0, cx),
                SettingsRow::PdfFit => {
                    let current = self.pdf.read(cx).zoom_mode();
                    let index = match current {
                        PdfZoomMode::FitWidth { .. } => 0,
                        PdfZoomMode::FitHeight => 1,
                        PdfZoomMode::Fixed { .. } => 2,
                    };
                    let next = adjacent_value(&[0, 1, 2], index, direction);
                    let mode = match next {
                        0 => PdfZoomMode::FitWidth { columns: self.pdf.read(cx).visible_page_count() },
                        1 => PdfZoomMode::FitHeight,
                        _ => PdfZoomMode::Fixed { zoom: self.pdf.read(cx).zoom() },
                    };
                    self.set_zoom_mode(mode, cx);
                }
                SettingsRow::PdfPages => self.adjust_visible_page_count(direction as isize, cx),
                SettingsRow::PdfZoom => self.adjust_zoom(direction as f32 * ZOOM_STEP, cx),
                _ => return false,
            }
        }
        self.sidebar_focus.focus(window, cx);
        cx.notify();
        true
    }

    /// Nudges the zoom, switching to the mode where zoom is held fixed.
    pub(super) fn adjust_zoom(&self, delta: f32, cx: &mut Context<Self>) {
        if self.pdf.read(cx).zoom_mode() == PdfZoomMode::FitHeight {
            return;
        }
        self.pdf.update(cx, |pdf, cx| pdf.change_zoom(delta, cx));
        let zoom = self.pdf.read(cx).zoom();
        ReaderSettings::update(cx, |preferences| {
            preferences.pdf_zoom_mode = PdfZoomModePreference::Fixed;
            preferences.pdf_zoom = zoom;
        });
        notify(cx, Component::PdfShell, "zoom_changed");
    }

    fn set_zoom_mode(&self, zoom_mode: PdfZoomMode, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| pdf.set_zoom_mode(zoom_mode, cx));
        ReaderSettings::update(cx, |preferences| store_zoom_mode(zoom_mode, preferences));
        notify(cx, Component::PdfShell, "zoom_mode_changed");
    }

    /// Sets the page count, switching to the mode where it is held fixed.
    fn adjust_visible_page_count(&self, delta: isize, cx: &mut Context<Self>) {
        let current = self.pdf.read(cx).visible_page_count();
        let next = current.saturating_add_signed(delta).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT);
        if current != next {
            self.set_zoom_mode(PdfZoomMode::FitWidth { columns: next }, cx);
        }
    }

    fn set_trim_margins(&self, trim_margins: bool, cx: &mut Context<Self>) {
        if !ReaderSettings::update(cx, |preferences| preferences.pdf_trim_margins = trim_margins) {
            return;
        }
        self.pdf.update(cx, |pdf, cx| pdf.set_trim_margins(trim_margins, cx));
        notify(cx, Component::PdfShell, "trim_margins_changed");
    }

    pub(super) fn render_settings(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let rows = self.settings_rows(cx);
        let zoom = self.pdf.read(cx).zoom();
        let zoom_mode = self.pdf.read(cx).zoom_mode();
        let visible_page_count = self.pdf.read(cx).visible_page_count();
        let trim_margins = ReaderSettings::preferences(cx).pdf_trim_margins;
        let general = components::reader_settings_tab_body(theme)
            .when(!cfg!(feature = "kobo"), |body| body.child(components::reader_settings_group(theme).child(self.settings_navigation.row(SettingsRow::Theme, &rows, crate::shell::theme_picker::reader_theme_picker(cx, theme), theme))))
            .children(self.settings_navigation.fullscreen_group(&rows, window, theme))
            .child(
                components::reader_settings_group(theme)
                    .child(
                        components::reader_setting_row_smallcaps("Fit", theme).map(|row| self.settings_navigation.row(SettingsRow::PdfFit, &rows, row, theme)).child(
                            components::reader_tile_group("pdf-fit").children([
                                components::reader_picture_tile(("pdf-fit", 0usize), components::reader_tile_picture("icons/reader-settings/pdf-fit-width.svg"), "Fit width", matches!(zoom_mode, PdfZoomMode::FitWidth { .. }), theme)
                                    .on_click(cx.listener(move |this, _, _, cx| this.set_zoom_mode(PdfZoomMode::FitWidth { columns: visible_page_count }, cx))),
                                components::reader_picture_tile(("pdf-fit", 1usize), components::reader_tile_picture("icons/reader-settings/pdf-fit-height.svg"), "Fit height", zoom_mode == PdfZoomMode::FitHeight, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_zoom_mode(PdfZoomMode::FitHeight, cx))),
                                components::reader_picture_tile(("pdf-fit", 2usize), components::reader_tile_picture("icons/reader-settings/pdf-fit-zoom.svg"), "Zoom", matches!(zoom_mode, PdfZoomMode::Fixed { .. }), theme)
                                    .on_click(cx.listener(move |this, _, _, cx| this.set_zoom_mode(PdfZoomMode::Fixed { zoom }, cx))),
                            ]),
                        ),
                    )
                    .child(
                        components::reader_setting_row_smallcaps("Margins", theme).map(|row| self.settings_navigation.row(SettingsRow::PdfMargins, &rows, row, theme)).child(
                            components::reader_tile_group("pdf-margins").children([
                                components::reader_picture_tile(("pdf-margins", 0usize), components::reader_tile_picture("icons/reader-settings/pdf-margins-show.svg"), "Show margins", !trim_margins, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_trim_margins(false, cx))),
                                components::reader_picture_tile(("pdf-margins", 1usize), components::reader_tile_picture("icons/reader-settings/pdf-margins-crop.svg"), "Crop margins", trim_margins, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_trim_margins(true, cx))),
                            ]),
                        ),
                    )
                    .children(matches!(zoom_mode, PdfZoomMode::FitWidth { .. }).then(|| {
                        components::reader_setting_row("Pages").map(|row| self.settings_navigation.row(SettingsRow::PdfPages, &rows, row, theme)).child(components::settings_stepper(
                            components::settings_stepper_button("pdf-pages-decrease", "−", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_visible_page_count(-1, cx))),
                            visible_page_count.to_string(),
                            components::settings_stepper_button("pdf-pages-increase", "+", false, theme).on_click(cx.listener(|this, _, _, cx| this.adjust_visible_page_count(1, cx))),
                            theme,
                        ))
                    }))
                    .children(matches!(zoom_mode, PdfZoomMode::Fixed { .. }).then(|| {
                        components::reader_setting_row("Zoom").map(|row| self.settings_navigation.row(SettingsRow::PdfZoom, &rows, row, theme)).child(components::settings_stepper(
                            components::settings_stepper_button("pdf-zoom-out", "−", false, theme).tooltip("Zoom out").on_click(cx.listener(|this, _, _, cx| this.adjust_zoom(-ZOOM_STEP, cx))),
                            format!("{:.0}%", zoom * 100.0),
                            components::settings_stepper_button("pdf-zoom-in", "+", false, theme).tooltip("Zoom in").on_click(cx.listener(|this, _, _, cx| this.adjust_zoom(ZOOM_STEP, cx))),
                            theme,
                        ))
                    })),
            )
            .into_any_element();
        crate::shell::keyboard_settings::with_keyboard_settings(general, window, cx)
    }
}

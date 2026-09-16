//! EPUB appearance preferences and matching renderer commands.

use app_preferences::{COLUMN_COUNT_MAX, COLUMN_COUNT_MIN, FONT_SIZE_MAX, FONT_SIZE_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, LINE_WIDTH_MAX, LINE_WIDTH_MIN, ReaderFontFamily, ReaderTextAlignment, ZOOM_MAX, ZOOM_MIN};
use gpui::{Context, Window};
use html_view_core::RendererCommand;

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::settings::{ReaderPreferences, ReaderSettings};

/// Narrowest column produced by fit-columns. This deliberately sits below the
/// hand-adjustable minimum so four columns can fit in a narrow window.
const FITTED_COLUMN_WIDTH_MIN: f64 = 80.0;

impl ReaderView {
    /// Persists a preference, applies its renderer command, and refreshes the
    /// reader only when the value actually changed.
    fn apply_appearance(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences), command: impl FnOnce(&ReaderPreferences) -> RendererCommand) {
        if !ReaderSettings::update(cx, update) {
            return;
        }
        let command = command(ReaderSettings::preferences(cx));
        self.with_renderer(cx, |renderer, cx| renderer.apply(command, cx));
        crate::invalidation::notify(cx, Component::ReaderShell, "preferences_changed");
    }

    fn apply_style_override(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences)) {
        // Read before the update: the overrides carry the theme's two lightness
        // ends, and those come from the application rather than from any reader
        // preference.
        let theme = crate::settings::library_theme_adaptation(cx);
        self.apply_appearance(cx, update, move |preferences| RendererCommand::SetReaderStyleOverrides(preferences.style_overrides(Some(theme))));
    }

    pub(in crate::epub) fn adjust_font_size(&self, delta: f32, cx: &mut Context<Self>) {
        let font_size = (ReaderSettings::preferences(cx).font_size + delta).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.apply_appearance(cx, |preferences| preferences.font_size = font_size, |_| RendererCommand::SetFontSize(font_size));
    }

    pub(in crate::epub) fn set_reader_font_family(&self, font_family: ReaderFontFamily, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| preferences.font_family = font_family);
    }

    pub(in crate::epub) fn set_reader_text_alignment(&self, text_alignment: ReaderTextAlignment, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| preferences.text_alignment = text_alignment);
    }

    pub(in crate::epub) fn adjust_line_height(&self, delta: f32, cx: &mut Context<Self>) {
        let line_height = (ReaderSettings::preferences(cx).line_height + delta).clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        self.apply_style_override(cx, |preferences| preferences.line_height = line_height);
    }

    pub(in crate::epub) fn adjust_desired_column_count(&mut self, delta: i8, window: &mut Window, cx: &mut Context<Self>) {
        if cfg!(feature = "kobo") {
            return;
        }
        const MIN_COLUMN_GAP: f64 = 20.0;
        let count = (i16::from(self.desired_column_count) + i16::from(delta)).clamp(i16::from(COLUMN_COUNT_MIN), i16::from(COLUMN_COUNT_MAX)) as u8;
        if count == self.desired_column_count {
            return;
        }
        self.desired_column_count = count;
        let preferences = ReaderSettings::preferences(cx);
        let sidebar_width = if self.sidebar.is_visible() { f64::from(preferences.toc_width) } else { 0.0 };
        let viewport_width = (f64::from(f32::from(window.viewport_size().width)) - sidebar_width).max(1.0) / preferences.scale.max(0.1);
        let width = (((viewport_width + MIN_COLUMN_GAP) / f64::from(count)) - MIN_COLUMN_GAP - 0.01).clamp(FITTED_COLUMN_WIDTH_MIN, LINE_WIDTH_MAX);
        self.apply_appearance(cx, |preferences| preferences.column_width = width, |_| RendererCommand::SetColumnWidth(width));
    }

    pub(in crate::epub) fn adjust_column_width(&self, delta: f64, cx: &mut Context<Self>) {
        if cfg!(feature = "kobo") {
            return;
        }
        let width = (ReaderSettings::preferences(cx).column_width + delta).clamp(LINE_WIDTH_MIN, LINE_WIDTH_MAX);
        self.apply_appearance(cx, |preferences| preferences.column_width = width, |_| RendererCommand::SetColumnWidth(width));
    }

    pub(in crate::epub) fn adjust_scale(&self, delta: f64, cx: &mut Context<Self>) {
        let scale = (ReaderSettings::preferences(cx).scale + delta).clamp(ZOOM_MIN, ZOOM_MAX);
        self.apply_appearance(cx, |preferences| preferences.scale = scale, |_| RendererCommand::SetScale(scale));
    }
}

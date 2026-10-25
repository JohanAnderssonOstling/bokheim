//! EPUB appearance preferences and matching renderer commands.

use crate::invalidation::notify;
use app_preferences::{FONT_SIZE_MAX, FONT_SIZE_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, LINE_WIDTH_MAX, LINE_WIDTH_MIN, READER_SPACING_MAX, READER_SPACING_MIN, ReaderFontFamily, ReaderTextAlignment};
use gpui::{Context, Window};
use html_view_core::RendererCommand;
use html_view_gpui::ReadingRulerStyle;

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::settings::{ReaderPreferences, ReaderSettings, SidebarTab};
use crate::shell::chrome;

/// The layout engine's fallback `ch` metric is half an em. Keeping this
/// conversion here makes the preference typographic rather than tied to a
/// particular screen's pixels, while the layout itself continues to receive
/// its required pixel width.
const CH_ADVANCE_EM: f64 = 0.5;

fn line_width_range_px(preferences: &ReaderPreferences) -> (f64, f64) {
    let ch = f64::from(preferences.font_size) * CH_ADVANCE_EM;
    let minimum = (preferences.min_line_width_ch * ch).clamp(LINE_WIDTH_MIN, LINE_WIDTH_MAX);
    let maximum = (preferences.max_line_width_ch * ch).clamp(minimum, LINE_WIDTH_MAX);
    (minimum, maximum)
}

/// The renderer puts unused width between columns after applying the explicit
/// outer margin. A single column has no gap to constrain.
fn column_count_for_gap(viewport_width: f64, column_width: f64, minimum_gap: f64, desired: u8) -> u8 {
    let count = ((viewport_width + minimum_gap) / (column_width + minimum_gap)).floor();
    (count.max(1.0) as u8).min(desired.max(1))
}

/// Use the integer width the renderer will actually keep. Reserve one layout
/// pixel for split and framebuffer rounding so an exact fit cannot become one
/// column when the final GPUI document area is fractionally narrower.
fn fitted_columns_and_width(viewport_width: f64, minimum_gap: f64, desired: u8, minimum_width: f64, maximum_width: f64) -> (u8, f64) {
    let count = column_count_for_gap(viewport_width, minimum_width, minimum_gap, desired);
    let fit = ((viewport_width - f64::from(count.saturating_sub(1)) * minimum_gap) / f64::from(count) - 1.0).floor().max(1.0);
    // The preferred minimum cannot make a column wider than its actual slot.
    // Otherwise a narrow reader clips the ends of lines at the viewport edge.
    (count, fit.min(maximum_width))
}

impl ReaderView {
    pub(in crate::epub) fn set_reading_ruler(&self, style: ReadingRulerStyle, cx: &mut Context<Self>) {
        if ReaderSettings::update(cx, |preferences| {
            if style != ReadingRulerStyle::Off {
                preferences.reading_ruler_last_style = style;
            } else if preferences.reading_ruler != ReadingRulerStyle::Off {
                preferences.reading_ruler_last_style = preferences.reading_ruler;
            }
            preferences.reading_ruler = style;
        }) {
            cx.notify();
        }
    }

    pub(in crate::epub) fn set_reading_ruler_enabled(&self, enabled: bool, cx: &mut Context<Self>) {
        if (ReaderSettings::preferences(cx).reading_ruler != ReadingRulerStyle::Off) == enabled {
            return;
        }
        let style = if enabled {
            match ReaderSettings::preferences(cx).reading_ruler_last_style {
                ReadingRulerStyle::Off => ReadingRulerStyle::Band,
                style => style,
            }
        } else {
            ReadingRulerStyle::Off
        };
        self.set_reading_ruler(style, cx);
    }

    pub(in crate::epub) fn set_reading_ruler_lines(&self, lines: usize, cx: &mut Context<Self>) {
        if ReaderSettings::update(cx, |preferences| preferences.reading_ruler_lines = lines.clamp(1, 5)) {
            cx.notify();
        }
    }

    pub(in crate::epub) fn active_sidebar_tab(&self, window: &Window) -> Option<SidebarTab> {
        if chrome::sidebar_overlays_document(window) { self.sidebar.effective(true) } else { Some(self.sidebar.effective_or_default(true)) }
    }

    /// The same horizontal geometry is used when painting and when the user
    /// asks to fit columns, including the sidebar's actual clamped width.
    pub(in crate::epub) fn horizontal_layout(window: &Window, cx: &Context<Self>, sidebar_visible: bool) -> (f32, f32, f32) {
        let sidebar_width = chrome::sidebar_width(sidebar_visible, window, cx);
        // GPUI's client-side window frame reserves space on both sides of the
        // window before laying out the reader. viewport_size includes that
        // space, while the HTML view's final bounds do not.
        let frame = gpui_component::window_paddings(window);
        let document_width = (f32::from(window.viewport_size().width - frame.left - frame.right) - sidebar_width).max(1.0);
        let margin = ReaderSettings::preferences(cx).horizontal_margin_px.min(((document_width - 100.0) / 2.0).max(0.0));
        (sidebar_width, margin, (document_width - margin * 2.0).max(1.0))
    }

    pub(in crate::epub) fn fitted_column_width(&self, available_width: f64, cx: &Context<Self>) -> f64 {
        let preferences = ReaderSettings::preferences(cx);
        let scale = preferences.scale.max(0.1);
        let viewport_width = available_width.max(1.0) / scale;
        let gap = f64::from(preferences.min_column_gap_px) / scale;
        let (minimum, maximum) = line_width_range_px(preferences);
        fitted_columns_and_width(viewport_width, gap, self.desired_column_count, minimum, maximum).1
    }

    pub(in crate::epub) fn max_columns_for_gap(&self, available_width: f64, cx: &Context<Self>) -> u8 {
        let preferences = ReaderSettings::preferences(cx);
        let scale = preferences.scale.max(0.1);
        let (minimum, maximum) = line_width_range_px(preferences);
        fitted_columns_and_width(available_width / scale, f64::from(preferences.min_column_gap_px) / scale, self.desired_column_count, minimum, maximum).0
    }

    /// Persists a preference, applies its renderer command, and refreshes the
    /// reader only when the value actually changed.
    fn apply_appearance(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences), command: impl FnOnce(&ReaderPreferences) -> RendererCommand) {
        if !ReaderSettings::update(cx, update) {
            return;
        }
        let command = command(ReaderSettings::preferences(cx));
        self.with_renderer(cx, |renderer, cx| renderer.apply(command, cx));
        notify(cx, Component::ReaderShell, "preferences_changed");
    }

    fn apply_style_override(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut ReaderPreferences)) {
        // Read before the update: the overrides carry the theme's two lightness
        // ends, and those come from the application rather than from any reader
        // preference.
        let theme = crate::settings::library_theme_adaptation(cx);
        let link_foreground = crate::settings::library_link_foreground(cx);
        self.apply_appearance(cx, update, move |preferences| RendererCommand::SetReaderStyleOverrides(preferences.style_overrides(Some(theme), link_foreground)));
    }

    pub(in crate::epub) fn adjust_font_size(&self, delta: f32, cx: &mut Context<Self>) {
        let font_size = (ReaderSettings::preferences(cx).font_size + delta).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.apply_appearance(cx, |preferences| preferences.font_size = font_size, |_| RendererCommand::SetFontSize(font_size));
    }

    pub(in crate::epub) fn set_reader_font_family(&self, font_family: ReaderFontFamily, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| {
            preferences.font_family = font_family;
            preferences.system_font_family = None;
        });
        self.font_picker.update(cx, |_, cx| cx.notify());
    }

    pub(in crate::epub) fn set_system_font_family(&self, family: String, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| preferences.system_font_family = Some(family));
    }

    pub(in crate::epub) fn set_reader_text_alignment(&self, text_alignment: ReaderTextAlignment, cx: &mut Context<Self>) {
        self.apply_style_override(cx, |preferences| preferences.text_alignment = text_alignment);
    }

    pub(in crate::epub) fn set_line_height(&self, line_height: f32, cx: &mut Context<Self>) {
        let line_height = line_height.clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        self.apply_style_override(cx, |preferences| preferences.line_height = line_height);
    }

    pub(in crate::epub) fn adjust_column_width(&self, delta: f64, window: &Window, cx: &mut Context<Self>) {
        // Keep the existing keyboard shortcuts useful: they shift both ends
        // of the typographic range by five characters.
        self.apply_line_width_range(window, cx, |minimum, maximum| {
            *minimum = (*minimum + delta.signum() * 5.0).clamp(20.0, 100.0);
            *maximum = (*maximum + delta.signum() * 5.0).clamp(20.0, 100.0);
        });
    }

    pub(in crate::epub) fn set_line_width_range(&self, range: (f64, f64), window: &Window, cx: &mut Context<Self>) {
        self.apply_line_width_range(window, cx, |minimum, maximum| {
            *minimum = range.0.clamp(20.0, 100.0);
            *maximum = range.1.clamp(*minimum, 100.0);
        });
    }

    fn apply_line_width_range(&self, window: &Window, cx: &mut Context<Self>, change: impl FnOnce(&mut f64, &mut f64)) {
        let current = ReaderSettings::preferences(cx).clone();
        let mut minimum = current.min_line_width_ch;
        let mut maximum = current.max_line_width_ch;
        change(&mut minimum, &mut maximum);
        if !ReaderSettings::update(cx, |preferences| {
            preferences.min_line_width_ch = minimum;
            preferences.max_line_width_ch = maximum;
        }) {
            return;
        }
        let sidebar_visible = (self.chrome.is_visible() || self.search.query.is_some()) && self.active_sidebar_tab(window).is_some();
        let (_, _, available_width) = Self::horizontal_layout(window, cx, sidebar_visible);
        let width = self.fitted_column_width(f64::from(available_width), cx);
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetColumnWidth(width), cx));
        notify(cx, Component::ReaderShell, "line_width_range_changed");
    }

    pub(in crate::epub) fn set_horizontal_margin(&self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(READER_SPACING_MIN, READER_SPACING_MAX);
        if ReaderSettings::update(cx, |preferences| preferences.horizontal_margin_px = value) {
            notify(cx, Component::ReaderShell, "horizontal_margin_changed");
        }
    }

    pub(in crate::epub) fn set_vertical_margin(&self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(READER_SPACING_MIN, READER_SPACING_MAX);
        if ReaderSettings::update(cx, |preferences| preferences.vertical_margin_px = value) {
            notify(cx, Component::ReaderShell, "vertical_margin_changed");
        }
    }

    pub(in crate::epub) fn set_min_column_gap(&self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(READER_SPACING_MIN, READER_SPACING_MAX);
        if ReaderSettings::update(cx, |preferences| preferences.min_column_gap_px = value) {
            notify(cx, Component::ReaderShell, "column_gap_changed");
        }
    }
}

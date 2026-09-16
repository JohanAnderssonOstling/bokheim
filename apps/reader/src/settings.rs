//! Reader preferences shared by all reader surfaces.
//!
//! [`ReaderSettings`] is mutable user state that is persisted on every change.

use std::rc::Rc;

use app_preferences::{ReaderFontFamily, ReaderPreferencePatch, ReaderTextAlignment};

/// The shared, synchronized description of how a PDF's layout is decided. The
/// reader carries `pdf_page_count` and `pdf_zoom` alongside it, so each mode
/// keeps the value it holds fixed.
pub use app_preferences::PdfZoomMode as PdfZoomModePreference;
use gpui::{App, Global};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReaderPreferences {
    pub font_size: f32,
    pub column_width: f64,
    pub scale: f64,
    pub toc_width: f32,
    #[serde(default = "default_toc_visible")]
    pub toc_visible: bool,
    #[serde(default)]
    pub font_family: ReaderFontFamily,
    #[serde(default)]
    pub text_alignment: ReaderTextAlignment,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    /// Set by the application from a user-facing setting, but not yet honoured
    /// by the native reader, which derives its column count from the viewport.
    #[serde(default = "default_max_column_count")]
    pub max_column_count: u8,
    #[serde(default = "default_pdf_page_count")]
    pub pdf_page_count: u8,
    #[serde(default = "default_pdf_trim_margins")]
    pub pdf_trim_margins: bool,
    /// Which end of the PDF layout the reader holds fixed. `pdf_page_count`
    /// carries the page count for [`PdfZoomModePreference::FitWidth`], and
    /// `pdf_zoom` the zoom for [`PdfZoomModePreference::Fixed`].
    #[serde(default)]
    pub pdf_zoom_mode: PdfZoomModePreference,
    /// Pixels per PDF point.
    #[serde(default = "default_pdf_zoom")]
    pub pdf_zoom: f32,
    /// How fast narration plays. Kept with the reader settings rather than with
    /// the book: it is a property of how someone listens, not of what they are
    /// listening to.
    #[serde(default = "default_audiobook_speed")]
    pub audiobook_speed: f64,
}

impl Default for ReaderPreferences {
    fn default() -> Self {
        Self {
            font_size: 20.0,
            column_width: 600.0,
            scale: 1.0,
            toc_width: 300.0,
            toc_visible: default_toc_visible(),
            font_family: ReaderFontFamily::default(),
            text_alignment: ReaderTextAlignment::default(),
            line_height: default_line_height(),
            max_column_count: default_max_column_count(),
            pdf_page_count: default_pdf_page_count(),
            pdf_trim_margins: default_pdf_trim_margins(),
            pdf_zoom_mode: PdfZoomModePreference::default(),
            pdf_zoom: default_pdf_zoom(),
            audiobook_speed: default_audiobook_speed(),
        }
    }
}

fn default_toc_visible() -> bool {
    !cfg!(feature = "kobo")
}

fn default_pdf_trim_margins() -> bool {
    true
}

fn default_pdf_zoom() -> f32 {
    1.0
}

fn default_pdf_page_count() -> u8 {
    2
}

fn default_line_height() -> f32 {
    1.5
}

fn default_max_column_count() -> u8 {
    2
}

fn default_audiobook_speed() -> f64 {
    1.0
}

impl ReaderPreferences {
    /// Applies the portion of reader state synchronized by the backend while
    /// preserving UI-only state such as TOC visibility and width.
    pub fn apply_synchronized(&mut self, saved: &app_preferences::ReaderPreferences) {
        self.font_size = saved.font_size_px();
        self.column_width = saved.max_line_width_px();
        self.scale = saved.zoom();
        self.line_height = saved.line_height();
        self.font_family = saved.font_family();
        self.text_alignment = saved.text_alignment();
        self.max_column_count = saved.max_column_count();
        self.pdf_zoom_mode = saved.pdf_zoom_mode();
        self.pdf_page_count = saved.pdf_page_count();
        self.pdf_zoom = saved.pdf_zoom();
        self.pdf_trim_margins = saved.pdf_trim_margins();
        self.audiobook_speed = saved.audiobook_speed();
    }

    /// Produces the backend preference updates represented by a UI change.
    pub fn synchronized_patches(&self, next: &Self) -> Vec<ReaderPreferencePatch> {
        let mut patches = Vec::new();
        if self.font_size != next.font_size {
            patches.push(ReaderPreferencePatch::FontSizePx(next.font_size));
        }
        if self.column_width != next.column_width {
            patches.push(ReaderPreferencePatch::MaxLineWidthPx(next.column_width));
        }
        if self.scale != next.scale {
            patches.push(ReaderPreferencePatch::Zoom(next.scale));
        }
        if self.line_height != next.line_height {
            patches.push(ReaderPreferencePatch::LineHeight(next.line_height));
        }
        if self.font_family != next.font_family {
            patches.push(ReaderPreferencePatch::FontFamily(next.font_family));
        }
        if self.text_alignment != next.text_alignment {
            patches.push(ReaderPreferencePatch::TextAlignment(next.text_alignment));
        }
        if self.max_column_count != next.max_column_count {
            patches.push(ReaderPreferencePatch::MaxColumnCount(next.max_column_count));
        }
        if self.pdf_zoom_mode != next.pdf_zoom_mode {
            patches.push(ReaderPreferencePatch::PdfZoomMode(next.pdf_zoom_mode));
        }
        if self.pdf_page_count != next.pdf_page_count {
            patches.push(ReaderPreferencePatch::PdfPageCount(next.pdf_page_count));
        }
        if self.pdf_zoom != next.pdf_zoom {
            patches.push(ReaderPreferencePatch::PdfZoom(next.pdf_zoom));
        }
        if self.pdf_trim_margins != next.pdf_trim_margins {
            patches.push(ReaderPreferencePatch::PdfTrimMargins(next.pdf_trim_margins));
        }
        if self.audiobook_speed != next.audiobook_speed {
            patches.push(ReaderPreferencePatch::AudiobookSpeed(next.audiobook_speed));
        }
        patches
    }

    pub(crate) fn style_overrides(&self, theme: Option<html::pipeline::ThemeAdaptation>) -> html::pipeline::ReaderStyleOverrides {
        let family = match self.font_family {
            ReaderFontFamily::Publisher => None,
            ReaderFontFamily::Serif => Some("serif".to_owned()),
        };
        let alignment = match self.text_alignment {
            ReaderTextAlignment::Publisher => None,
            ReaderTextAlignment::Start => Some(html::pipeline::TextAlign::Left),
            ReaderTextAlignment::Justify => Some(html::pipeline::TextAlign::Justify),
        };
        // `foreground` stays unset on purpose: it replaces every run's colour,
        // which flattens a publication rather than adapting it. `theme` is the
        // one that re-lights instead of overwriting.
        html::pipeline::ReaderStyleOverrides { font_family: family, text_align: alignment, line_height: Some(self.line_height), minimum_font_size: None, foreground: None, background: None, theme }
    }
}

/// The two ends of the reader's lightness range, taken from the application
/// theme, so a publication's colours are re-lit into the page it is drawn on.
pub(crate) fn library_theme_adaptation(cx: &App) -> html::pipeline::ThemeAdaptation {
    let theme = ui_components::browser_theme(cx);
    let opaque = |colour: gpui::Hsla| u32::from(gpui::Rgba::from(colour)) | 0xff;
    html::pipeline::ThemeAdaptation { ink: opaque(theme.text), surface: opaque(theme.page_bg) }
}

pub(crate) fn library_paint_palette(cx: &App) -> html_view_core::ReaderPaintPalette {
    let theme = ui_components::browser_theme(cx);
    // A paint-time foreground replaces the computed color of every text run,
    // including colors explicitly supplied by the publication. Leave it unset
    // so EPUB styles reach the painter; the reader theme still owns the canvas.
    html_view_core::ReaderPaintPalette { foreground: None, background: Some(u32::from(gpui::Rgba::from(theme.page_bg))) }
}

/// The reader's highlights, in the two spellings its renderers use.
///
/// What the colours *are* is [`ui_components::InteractionHighlights`]; this is
/// only the conversion, kept here because the two renderer colour types are the
/// reader's business and not the design system's.
pub(crate) fn library_interaction_palette(cx: &App) -> html_view_core::InteractionPalette {
    let highlights = ui_components::InteractionHighlights::from_theme(ui_components::browser_theme(cx));
    let colour = |[r, g, b, a]: [u8; 4]| html_view_core::Color::rgba8(r, g, b, a);
    html_view_core::InteractionPalette {
        selection: colour(highlights.selection),
        search_match: colour(highlights.search_match),
        active_search_match: colour(highlights.active_search_match),
        annotation: colour(highlights.annotation),
        active_annotation: colour(highlights.active_annotation),
    }
}

pub(crate) fn library_interaction_style(cx: &App) -> pdf_reader_core::ReaderInteractionStyle {
    let highlights = ui_components::InteractionHighlights::from_theme(ui_components::browser_theme(cx));
    let colour = |[r, g, b, a]: [u8; 4]| pdf_reader_core::RgbaColor::new(r, g, b, a);
    pdf_reader_core::ReaderInteractionStyle {
        selection: colour(highlights.selection),
        search_match: colour(highlights.search_match),
        active_search_match: colour(highlights.active_search_match),
        annotation: colour(highlights.annotation),
        active_annotation: colour(highlights.active_annotation),
    }
}

pub type SaveReaderSettings = Rc<dyn Fn(ReaderPreferences, &mut App)>;

/// User state shared by every reader surface, persisted through [`Self::save`]
/// whenever it changes.
pub(crate) struct ReaderSettings {
    pub(crate) preferences: ReaderPreferences,
    pub(crate) save: SaveReaderSettings,
}

impl Global for ReaderSettings {}

impl ReaderSettings {
    /// Applies `update` to the shared preferences and persists them.
    ///
    /// Returns whether anything actually changed; a no-op update neither
    /// persists nor reports a change, so callers can use the result to decide
    /// whether to repaint.
    pub(crate) fn update(cx: &mut App, update: impl FnOnce(&mut ReaderPreferences)) -> bool {
        let Some((preferences, save)) = ({
            let settings = cx.global_mut::<Self>();
            let previous = settings.preferences.clone();
            update(&mut settings.preferences);
            (previous != settings.preferences).then(|| (settings.preferences.clone(), settings.save.clone()))
        }) else {
            return false;
        };
        save(preferences, cx);
        true
    }

    pub(crate) fn preferences(cx: &App) -> &ReaderPreferences {
        &cx.global::<Self>().preferences
    }
}

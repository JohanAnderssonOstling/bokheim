//! Reader preferences and the process-wide globals every reader window reads.
//!
//! Two globals, split by lifecycle. [`ReaderSettings`] is mutable user state
//! that is persisted on every change; [`ReaderServices`] is the set of
//! collaborators installed once by [`crate::configure`] and never absent
//! afterwards, so nothing downstream has to unwrap them.

use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::app::preferences::{ReaderFontFamily, ReaderTextAlignment};

/// The shared, synchronized description of how a PDF's layout is decided. The
/// reader carries `pdf_page_count` and `pdf_zoom` alongside it, so each mode
/// keeps the value it holds fixed.
pub use epub_svelte_lib::app::preferences::PdfZoomMode as PdfZoomModePreference;
use gpui::{App, Global};
use ui_runtime::BlockingWorker;

use crate::backends::{NativeAudioBackend, TtsProvider};

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

impl ReaderPreferences {
    pub(crate) fn style_overrides(&self) -> html::pipeline::ReaderStyleOverrides {
        let family = match self.font_family {
            ReaderFontFamily::Publisher => None,
            ReaderFontFamily::Serif => Some("serif".to_owned()),
            ReaderFontFamily::SansSerif => Some("sans-serif".to_owned()),
        };
        let alignment = match self.text_alignment {
            ReaderTextAlignment::Publisher => None,
            ReaderTextAlignment::Start => Some(html::pipeline::TextAlign::Left),
            ReaderTextAlignment::Justify => Some(html::pipeline::TextAlign::Justify),
        };
        html::pipeline::ReaderStyleOverrides { font_family: family, text_align: alignment, line_height: Some(self.line_height), minimum_font_size: None, foreground: None, background: None }
    }
}

pub(crate) fn library_paint_palette(cx: &App) -> html_view_core::ReaderPaintPalette {
    let theme = ui_components::browser_theme(cx);
    // A paint-time foreground replaces the computed color of every text run,
    // including colors explicitly supplied by the publication. Leave it unset
    // so EPUB styles reach the painter; the reader theme still owns the canvas.
    html_view_core::ReaderPaintPalette { foreground: None, background: Some(u32::from(gpui::Rgba::from(theme.page_bg))) }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReaderWindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub type SaveReaderSettings = Rc<dyn Fn(ReaderPreferences, Option<ReaderWindowGeometry>, &mut App)>;

/// User state shared by every reader window, persisted through [`Self::save`]
/// whenever it changes.
pub(crate) struct ReaderSettings {
    pub(crate) preferences: ReaderPreferences,
    pub(crate) geometry: Option<ReaderWindowGeometry>,
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
        let Some((preferences, geometry, save)) = ({
            let settings = cx.global_mut::<Self>();
            let previous = settings.preferences.clone();
            update(&mut settings.preferences);
            (previous != settings.preferences).then(|| (settings.preferences.clone(), settings.geometry, settings.save.clone()))
        }) else {
            return false;
        };
        save(preferences, geometry, cx);
        true
    }

    /// Records a new window geometry and persists it, ignoring repeats.
    pub(crate) fn set_geometry(cx: &mut App, geometry: ReaderWindowGeometry) {
        let (preferences, save) = {
            let settings = cx.global_mut::<Self>();
            if settings.geometry == Some(geometry) {
                return;
            }
            settings.geometry = Some(geometry);
            (settings.preferences.clone(), settings.save.clone())
        };
        save(preferences, Some(geometry), cx);
    }

    pub(crate) fn preferences(cx: &App) -> &ReaderPreferences {
        &cx.global::<Self>().preferences
    }
}

/// Collaborators installed once by [`crate::configure`]. Present for the whole
/// life of the process, so readers use them without unwrapping.
pub(crate) struct ReaderServices {
    /// Off-thread work that may block: opening publications, decoding images,
    /// whole-book search.
    pub(crate) blocking: BlockingWorker,
    /// Off-thread library writes: reading positions, annotations, bookmarks.
    pub(crate) persistence: BlockingWorker,
    pub(crate) speech: Arc<dyn TtsProvider>,
    pub(crate) audio: Arc<dyn NativeAudioBackend>,
}

impl Global for ReaderServices {}

impl ReaderServices {
    pub(crate) fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }
}

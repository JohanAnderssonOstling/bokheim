//! Reader preferences shared by all reader surfaces.
//!
//! [`ReaderSettings`] is mutable user state that is persisted on every change.

use ui_text::text;

use content_address::BookId;
use language_tags::LanguageTag;
use pdf_view_gpui::ReadingRulerStyle as PdfReadingRulerStyle;
use std::{collections::BTreeMap, rc::Rc};

use crate::cjk::CjkFont;
use html_style_model::Hyphens;
use html::style::ReaderStyleOverrides;
use html_style_model::TextAlign;
use html::style::ThemeAdaptation;

use app_preferences::{DEFAULT_WORD_SPACING_PERCENT, ReaderFirstLineIndent, ReaderFontFamily, ReaderHyphenation, ReaderPreferencePatch, ReaderTextAlignment};

/// The shared, synchronized description of how a PDF's layout is decided. The
/// reader carries `pdf_page_count` and `pdf_zoom` alongside it, so each mode
/// keeps the value it holds fixed.
pub use app_preferences::PdfZoomMode as PdfZoomModePreference;
use gpui::{App, Global};

/// A host can register device fonts in the background. Closing the sender
/// releases every book waiting to create its text shaper.
pub struct SystemFontsLoading(pub async_channel::Receiver<()>);

impl Global for SystemFontsLoading {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReaderFontRole {
    Serif,
    SansSerif,
    Monospace,
    Cjk,
}

impl ReaderFontRole {
    pub(crate) const ALL: [Self; 3] = [Self::Serif, Self::SansSerif, Self::Monospace];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Serif => text!(common::serif),
            Self::SansSerif => text!(common::sans_serif),
            Self::Monospace => text!(reader::monospace),
            Self::Cjk => text!(reader::cjk),
        }
    }

    fn generic(self) -> &'static str {
        match self {
            Self::Serif => "serif",
            Self::SansSerif => "sans-serif",
            Self::Monospace => "monospace",
            Self::Cjk => "sans-serif",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Serif => 0,
            Self::SansSerif => 1,
            Self::Monospace => 2,
            Self::Cjk => 3,
        }
    }
}

/// Device-local typeface choice applied only to CJK text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CjkFontChoice {
    #[default]
    Publisher,
    Custom,
}

/// Which sidebar panel a reader last had open. TTS is not a panel: it plays
/// in a dock under the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SidebarTab {
    #[default]
    Contents,
    Notes,
    Search,
    Bookmarks,
    #[serde(other)]
    Settings,
}

/// How long narration pauses between sentences and between passages.
/// `Natural` keeps the voice's own timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NarrationPauses {
    Short,
    #[default]
    Natural,
    Long,
    Longer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NarrationVoice {
    George,
    Device,
    #[default]
    #[serde(other)]
    Alba,
}

/// One locally selected voice for a language, shared by its regional variants.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NarrationVoicePreference {
    pub voice: NarrationVoice,
    pub device_voice: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReaderPreferences {
    /// Validated settings shared with the backend and every frontend.
    #[serde(flatten)]
    pub synchronized: app_preferences::ReaderPreferences,
    /// Reading guides are a local display preference and do not create book annotations.
    pub reading_ruler: html_view_gpui::ReadingRulerStyle,
    pub reading_ruler_last_style: html_view_gpui::ReadingRulerStyle,
    pub reading_ruler_lines: usize,
    /// PDF guides are device-local and independent of the HTML ruler.
    pub pdf_reading_ruler: PdfReadingRulerStyle,
    pub pdf_reading_ruler_last_style: PdfReadingRulerStyle,
    pub pdf_reading_ruler_lines: usize,
    pub toc_width: f32,
    pub active_sidebar_tab: SidebarTab,
    /// Whether the reader controls were hidden when last used.
    pub chrome_hidden: bool,
    /// Whether a sidebar tab is open, independently of mobile controls.
    pub sidebar_open: bool,
    /// Installed font names belong to this device and are not synchronized.
    pub system_font_family: Option<String>,
    pub system_sans_serif_font_family: Option<String>,
    pub system_monospace_font_family: Option<String>,
    /// Whether to use this device's selected custom fonts.
    pub custom_fonts_enabled: bool,
    pub cjk_font_choice: CjkFontChoice,
    pub system_cjk_font_family: Option<String>,
    /// Kept on this device only; the backend does not synchronize it.
    pub narration_pauses: NarrationPauses,
    /// The narration source and bundled voice selected on this device.
    pub narration_voice: NarrationVoice,
    /// An opaque installed voice identity; local to this device and never synchronized.
    pub narration_device_voice: Option<String>,
    /// One voice per base language, local to this device and never synchronized.
    pub narration_voices: BTreeMap<String, NarrationVoicePreference>,
    /// Last chosen translation target, local to this device.
    pub translation_language: Option<LanguageTag>,
    /// Manually chosen source languages, local to this device and keyed by book.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub translation_sources: BTreeMap<BookId, LanguageTag>,
}

impl Default for ReaderPreferences {
    fn default() -> Self {
        Self {
            synchronized: app_preferences::ReaderPreferences::default(),
            reading_ruler: html_view_gpui::ReadingRulerStyle::Off,
            reading_ruler_last_style: default_reading_ruler_style(),
            reading_ruler_lines: default_reading_ruler_lines(),
            pdf_reading_ruler: PdfReadingRulerStyle::Off,
            pdf_reading_ruler_last_style: default_pdf_reading_ruler_style(),
            pdf_reading_ruler_lines: default_reading_ruler_lines(),
            toc_width: 300.0,
            active_sidebar_tab: SidebarTab::default(),
            chrome_hidden: false,
            sidebar_open: default_sidebar_open(),
            system_font_family: None,
            system_sans_serif_font_family: None,
            system_monospace_font_family: None,
            custom_fonts_enabled: false,
            cjk_font_choice: CjkFontChoice::default(),
            system_cjk_font_family: None,
            narration_pauses: NarrationPauses::default(),
            narration_voice: NarrationVoice::default(),
            narration_device_voice: None,
            narration_voices: BTreeMap::new(),
            translation_language: None,
            translation_sources: BTreeMap::new(),
        }
    }
}

fn default_pdf_reading_ruler_style() -> PdfReadingRulerStyle {
    PdfReadingRulerStyle::Band
}

fn default_reading_ruler_lines() -> usize {
    1
}

fn default_reading_ruler_style() -> html_view_gpui::ReadingRulerStyle {
    html_view_gpui::ReadingRulerStyle::Band
}

fn default_sidebar_open() -> bool {
    !cfg!(any(feature = "mobile", feature = "kobo")) && !ui_components::touch_ui()
}

impl ReaderPreferences {
    pub(crate) fn saved_cjk_font_family(&self) -> Option<&str> {
        self.system_cjk_font_family.as_deref().filter(|family| !family.trim().is_empty() && *family != ui_components::CJK_FONT_FAMILY)
    }

    pub(crate) fn custom_font_family(&self, role: ReaderFontRole) -> Option<&str> {
        match role {
            ReaderFontRole::Serif => self.system_font_family.as_deref(),
            ReaderFontRole::SansSerif => self.system_sans_serif_font_family.as_deref(),
            ReaderFontRole::Monospace => self.system_monospace_font_family.as_deref(),
            ReaderFontRole::Cjk => self.system_cjk_font_family.as_deref(),
        }
    }

    pub(crate) fn set_custom_font_family(&mut self, role: ReaderFontRole, family: String) {
        match role {
            ReaderFontRole::Serif => self.system_font_family = Some(family),
            ReaderFontRole::SansSerif => self.system_sans_serif_font_family = Some(family),
            ReaderFontRole::Monospace => self.system_monospace_font_family = Some(family),
            ReaderFontRole::Cjk => {
                self.system_cjk_font_family = Some(family);
                self.cjk_font_choice = CjkFontChoice::Custom;
                return;
            }
        }
        self.custom_fonts_enabled = true;
    }

    pub(crate) fn custom_fonts_active(&self) -> bool {
        self.custom_fonts_enabled
    }

    fn custom_font_override(&self, role: ReaderFontRole) -> Option<String> {
        if !self.custom_fonts_active() {
            return None;
        }
        self.custom_font_family(role).map(|family| format!("\"{}\", {}", family.replace('\\', "\\\\").replace('"', "\\\""), role.generic()))
    }

    /// Applies the portion of reader state synchronized by the backend while
    /// preserving UI-only state such as the active sidebar tab and its width.
    pub fn apply_synchronized(&mut self, saved: &app_preferences::ReaderPreferences) {
        self.synchronized = *saved;
    }

    /// Produces the backend preference updates represented by a UI change.
    pub fn synchronized_patches(&self, next: &Self) -> Vec<ReaderPreferencePatch> {
        self.synchronized.patches_to(&next.synchronized)
    }

    /// UI controls use the same validation as backend preference updates.
    pub(crate) fn apply_preference(&mut self, patch: ReaderPreferencePatch) {
        if let Err(error) = patch.apply(&mut self.synchronized) {
            log::warn!("Reader preference was rejected: {error}");
        }
    }

    pub(crate) fn apply_cjk_font(&self, overrides: &mut ReaderStyleOverrides, cjk: Option<CjkFont>) {
        if cjk.is_none() {
            return;
        }
        let embedded = format!("\"{}\"", ui_components::CJK_FONT_FAMILY);
        overrides.cjk_font_fallback = Some(embedded.clone());
        overrides.cjk_font_family = match self.cjk_font_choice {
            CjkFontChoice::Publisher => None,
            CjkFontChoice::Custom => Some(self.system_cjk_font_family.as_deref().filter(|family| !family.trim().is_empty()).map(|family| format!("\"{}\"", family.replace('\\', "\\\\").replace('"', "\\\""))).unwrap_or(embedded)),
        };
    }

    pub(crate) fn style_overrides_for_book(&self, cjk: Option<CjkFont>, theme: Option<ThemeAdaptation>, link_foreground: u32) -> ReaderStyleOverrides {
        let mut overrides = self.style_overrides(theme, link_foreground);
        self.apply_cjk_font(&mut overrides, cjk);
        overrides
    }

    pub(crate) fn style_overrides(&self, theme: Option<ThemeAdaptation>, link_foreground: u32) -> ReaderStyleOverrides {
        let family = match self.synchronized.font_family() {
            ReaderFontFamily::Publisher => None,
            ReaderFontFamily::Serif => Some("NV Garamond, serif".to_owned()),
            ReaderFontFamily::SansSerif => Some("Raleway, sans-serif".to_owned()),
        };
        let family = if self.custom_fonts_active() { None } else { family };
        let alignment = match self.synchronized.text_alignment() {
            ReaderTextAlignment::Publisher => None,
            ReaderTextAlignment::Start => Some(TextAlign::Left),
            ReaderTextAlignment::Justify => Some(TextAlign::Justify),
        };
        // `foreground` stays unset on purpose: it replaces every run's colour,
        // which flattens a book rather than adapting it. `theme` is the
        // one that re-lights instead of overwriting.
        ReaderStyleOverrides {
            hyphens: match self.synchronized.hyphenation() {
                ReaderHyphenation::Book => None,
                ReaderHyphenation::Off => Some(Hyphens::None),
                ReaderHyphenation::On => Some(Hyphens::Auto),
            },
            font_weight_adjustment: self.synchronized.font_weight().adjustment(),
            word_spacing_factor: (self.synchronized.word_spacing_percent() != DEFAULT_WORD_SPACING_PERCENT).then_some(self.synchronized.word_spacing_percent() / 100.0),
            font_family: family,
            cjk_font_family: None,
            cjk_font_fallback: None,
            serif_font_family: self.custom_font_override(ReaderFontRole::Serif),
            sans_serif_font_family: self.custom_font_override(ReaderFontRole::SansSerif),
            monospace_font_family: self.custom_font_override(ReaderFontRole::Monospace),
            text_align: alignment,
            line_height: self.synchronized.override_line_height().then_some(self.synchronized.line_height()),
            paragraph_spacing_em: self.synchronized.override_paragraph_spacing().then_some(self.synchronized.paragraph_spacing_em()),
            first_line_indent_em: match self.synchronized.first_line_indent() {
                ReaderFirstLineIndent::Book => None,
                ReaderFirstLineIndent::Off => Some(0.0),
                ReaderFirstLineIndent::Custom => Some(self.synchronized.first_line_indent_em()),
            },
            // The font-size control scales publisher typography from the CSS
            // regular size, including fixed sizes and nested relative sizes.
            canonical_font_size: Some(16.0),
            minimum_font_size: None,
            foreground: None,
            link_foreground: Some(link_foreground),
            background: None,
            strip_root_spacing: true,
            theme,
        }
    }
}

/// The two ends of the reader's lightness range, taken from the application
/// theme, so a book's colours are re-lit into the page it is drawn on.
pub(crate) fn library_theme_adaptation(cx: &App) -> ThemeAdaptation {
    let theme = ui_components::browser_theme(cx);
    let opaque = |colour: gpui::Hsla| u32::from(gpui::Rgba::from(colour)) | 0xff;
    ThemeAdaptation { ink: opaque(theme.text), surface: opaque(theme.page_bg) }
}

pub(crate) fn library_link_foreground(cx: &App) -> u32 {
    u32::from(gpui::Rgba::from(ui_components::browser_theme(cx).accent)) | 0xff
}

pub(crate) fn library_paint_palette(cx: &App) -> skrift_column_paginator::ReaderPaintPalette {
    let theme = ui_components::browser_theme(cx);
    // A paint-time foreground replaces the computed color of every text run,
    // including colors explicitly supplied by the book. Leave it unset
    // so EPUB styles reach the painter; the reader theme still owns the canvas.
    skrift_column_paginator::ReaderPaintPalette { foreground: None, background: Some(u32::from(gpui::Rgba::from(theme.page_bg))) }
}

/// The reader's highlights, in the two spellings its renderers use.
///
/// What the colours *are* is [`ui_components::InteractionHighlights`]; this is
/// only the conversion, kept here because the two renderer colour types are the
/// reader's business and not the design system's.
pub(crate) fn library_interaction_palette(cx: &App) -> skrift_column_paginator::InteractionPalette {
    let highlights = ui_components::InteractionHighlights::from_theme(ui_components::browser_theme(cx));
    let colour = |[r, g, b, a]: [u8; 4]| skrift_column_paginator::Color::rgba8(r, g, b, a);
    skrift_column_paginator::InteractionPalette {
        selection: colour(highlights.selection),
        search_match: colour(highlights.search_match),
        active_search_match: colour(highlights.active_search_match),
        annotation: colour(highlights.annotation),
        active_annotation: colour(highlights.active_annotation),
        reading: colour(highlights.reading),
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
        reading: colour(highlights.reading),
    }
}

/// Persists the full preferences locally and the changed synchronized fields
/// remotely. The reader computes patches while it owns both versions.
pub type SaveReaderSettings = Rc<dyn Fn(ReaderPreferences, Vec<ReaderPreferencePatch>, &mut App)>;

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
        let Some((preferences, patches, save)) = ({
            let settings = cx.global_mut::<Self>();
            let previous = settings.preferences.clone();
            update(&mut settings.preferences);
            (previous != settings.preferences).then(|| (settings.preferences.clone(), previous.synchronized_patches(&settings.preferences), settings.save.clone()))
        }) else {
            return false;
        };
        save(preferences, patches, cx);
        true
    }

    pub(crate) fn preferences(cx: &App) -> &ReaderPreferences {
        &cx.global::<Self>().preferences
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_preferences::ReaderFontWeight;
    use gpui::{AppContext, TestAppContext};

    #[test]
    fn device_voice_preferences_stay_local_and_round_trip() {
        let defaults = ReaderPreferences::default();
        let preferences = ReaderPreferences {
            narration_voice: NarrationVoice::Device,
            narration_device_voice: Some("opaque-device-voice".into()),
            narration_voices: [("sv".into(), NarrationVoicePreference { voice: NarrationVoice::Device, device_voice: Some("opaque-device-voice".into()) })].into(),
            ..defaults.clone()
        };
        assert!(defaults.synchronized_patches(&preferences).is_empty());
        let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
        restored.apply_synchronized(&app_preferences::ReaderPreferences::default());
        assert_eq!(restored.narration_voice, NarrationVoice::Device);
        assert_eq!(restored.narration_device_voice.as_deref(), Some("opaque-device-voice"));
        assert_eq!(restored.narration_voices, preferences.narration_voices);
        let snapshot = serde_json::to_value(defaults).unwrap();

        let restored: ReaderPreferences = serde_json::from_value(snapshot).unwrap();
        assert_eq!(restored.narration_voice, NarrationVoice::Alba);
        assert!(restored.narration_device_voice.is_none());
        assert!(restored.narration_voices.is_empty());
    }

    #[test]
    fn pdf_ruler_preferences_are_local_independent_and_round_trip() {
        let mut defaults = ReaderPreferences::default();
        let synchronized = app_preferences::ReaderPreferences::default();
        defaults.apply_synchronized(&synchronized);
        let preferences = ReaderPreferences {
            reading_ruler: html_view_gpui::ReadingRulerStyle::Underline,
            pdf_reading_ruler: PdfReadingRulerStyle::Focus,
            pdf_reading_ruler_last_style: PdfReadingRulerStyle::Band,
            pdf_reading_ruler_lines: 3,
            ..defaults.clone()
        };
        assert!(defaults.synchronized_patches(&preferences).is_empty());
        let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
        restored.apply_synchronized(&synchronized);
        assert_eq!(restored, preferences);
        restored.pdf_reading_ruler = PdfReadingRulerStyle::Off;
        let saved = serde_json::to_value(&restored).unwrap();
        let disabled: ReaderPreferences = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(disabled.pdf_reading_ruler_last_style, PdfReadingRulerStyle::Band);
        assert_eq!(disabled.reading_ruler, html_view_gpui::ReadingRulerStyle::Underline);
        let snapshot = saved;
        let migrated: ReaderPreferences = serde_json::from_value(snapshot).unwrap();
        assert_eq!(migrated.pdf_reading_ruler, PdfReadingRulerStyle::Off);
        assert_eq!(migrated.pdf_reading_ruler_last_style, PdfReadingRulerStyle::Band);
        assert_eq!(migrated.pdf_reading_ruler_lines, preferences.pdf_reading_ruler_lines);
        assert_eq!(migrated.reading_ruler, html_view_gpui::ReadingRulerStyle::Underline);
    }

    #[test]
    fn pdf_and_comic_spacing_round_trip_without_changing_html_spacing() {
        let mut previous = ReaderPreferences::default();
        previous.apply_synchronized(&app_preferences::ReaderPreferences::default());
        let mut next = previous.clone();
        for patch in [
            ReaderPreferencePatch::PdfHorizontalMarginPx(8.0),
            ReaderPreferencePatch::PdfVerticalMarginPx(16.0),
            ReaderPreferencePatch::PdfPageGapPx(24.0),
            ReaderPreferencePatch::ComicHorizontalMarginPx(32.0),
            ReaderPreferencePatch::ComicVerticalMarginPx(40.0),
            ReaderPreferencePatch::ComicPageGapPx(48.0),
        ] {
            next.apply_preference(patch);
        }
        let patches = previous.synchronized_patches(&next);
        assert_eq!(patches.len(), 6);
        let mut backend = app_preferences::ReaderPreferences::default();
        for patch in patches {
            patch.apply(&mut backend).unwrap();
        }
        let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&next).unwrap()).unwrap();
        restored.apply_synchronized(&backend);
        assert_eq!(restored, next);
        assert_eq!(restored.synchronized.horizontal_margin_px(), previous.synchronized.horizontal_margin_px());
        assert_eq!(restored.synchronized.vertical_margin_px(), previous.synchronized.vertical_margin_px());
        assert_eq!(restored.synchronized.min_column_gap_px(), previous.synchronized.min_column_gap_px());
        let snapshot = serde_json::to_value(&next).unwrap();
        assert_eq!(serde_json::from_value::<ReaderPreferences>(snapshot).unwrap(), previous);
    }

    #[test]
    fn hyphenation_choices_reach_the_renderer_and_survive_backend_updates() {
        let previous = ReaderPreferences::default();
        let snapshot = serde_json::to_value(&previous).unwrap();

        assert_eq!(serde_json::from_value::<ReaderPreferences>(snapshot).unwrap().synchronized.hyphenation(), ReaderHyphenation::Book);
        for choice in ReaderHyphenation::ALL {
            let mut preferences = previous.clone();
            preferences.apply_preference(ReaderPreferencePatch::Hyphenation(choice));
            let mut backend = app_preferences::ReaderPreferences::default();
            for patch in previous.synchronized_patches(&preferences) {
                patch.apply(&mut backend).unwrap();
            }
            let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
            restored.apply_synchronized(&backend);
            assert_eq!(restored.synchronized.hyphenation(), choice);
            assert_eq!(
                restored.style_overrides(None, 0).hyphens,
                match choice {
                    ReaderHyphenation::Book => None,
                    ReaderHyphenation::Off => Some(Hyphens::None),
                    ReaderHyphenation::On => Some(Hyphens::Auto),
                }
            );
        }
    }

    #[test]
    fn first_line_indent_off_removes_indentation_without_losing_custom_value() {
        let mut current = ReaderPreferences::default();
        current.apply_preference(ReaderPreferencePatch::FirstLineIndentEm(2.1));
        current.apply_preference(ReaderPreferencePatch::FirstLineIndent(ReaderFirstLineIndent::Custom));
        let mut backend = app_preferences::ReaderPreferences::default();
        backend.set_first_line_indent_em(2.1).unwrap();
        backend.set_first_line_indent(ReaderFirstLineIndent::Custom);
        for (mode, expected) in [(ReaderFirstLineIndent::Off, Some(0.0)), (ReaderFirstLineIndent::Book, None), (ReaderFirstLineIndent::Custom, Some(2.1))] {
            let mut next = current.clone();
            next.apply_preference(ReaderPreferencePatch::FirstLineIndent(mode));
            let patches = current.synchronized_patches(&next);
            assert_eq!(patches, vec![ReaderPreferencePatch::FirstLineIndent(mode)]);
            for patch in patches {
                patch.apply(&mut backend).unwrap();
            }
            let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&next).unwrap()).unwrap();
            restored.apply_synchronized(&backend);
            assert_eq!(restored.synchronized.first_line_indent(), mode);
            assert_eq!(restored.synchronized.first_line_indent_em(), 2.1);
            assert_eq!(restored.style_overrides(None, 0).first_line_indent_em, expected);
            current = restored;
        }
    }

    #[test]
    fn paragraph_controls_reach_layout_and_preserve_custom_values_in_book_mode() {
        let previous = ReaderPreferences::default();
        let snapshot = serde_json::to_value(&previous).unwrap();
        let migrated: ReaderPreferences = serde_json::from_value(snapshot).unwrap();
        assert_eq!(migrated.synchronized.paragraph_spacing_em(), 1.0);
        assert_eq!(migrated.synchronized.first_line_indent_em(), 1.5);
        assert_eq!(migrated.style_overrides(None, 0).paragraph_spacing_em, None);
        assert_eq!(migrated.style_overrides(None, 0).first_line_indent_em, None);
        let mut custom = previous.clone();
        custom.apply_preference(ReaderPreferencePatch::ParagraphSpacingEm(0.7));
        custom.apply_preference(ReaderPreferencePatch::FirstLineIndentEm(2.1));
        custom.apply_preference(ReaderPreferencePatch::OverrideParagraphSpacing(true));
        custom.apply_preference(ReaderPreferencePatch::FirstLineIndent(ReaderFirstLineIndent::Custom));
        let mut backend = app_preferences::ReaderPreferences::default();
        for patch in previous.synchronized_patches(&custom) {
            patch.apply(&mut backend).unwrap();
        }
        let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&custom).unwrap()).unwrap();
        restored.apply_synchronized(&backend);
        assert_eq!(restored.style_overrides(None, 0).paragraph_spacing_em, Some(0.7));
        assert_eq!(restored.style_overrides(None, 0).first_line_indent_em, Some(2.1));
        let mut book = custom.clone();
        book.apply_preference(ReaderPreferencePatch::OverrideParagraphSpacing(false));
        book.apply_preference(ReaderPreferencePatch::FirstLineIndent(ReaderFirstLineIndent::Book));
        for patch in custom.synchronized_patches(&book) {
            patch.apply(&mut backend).unwrap();
        }
        restored.apply_synchronized(&backend);
        assert_eq!(restored.synchronized.paragraph_spacing_em(), 0.7);
        assert_eq!(restored.synchronized.first_line_indent_em(), 2.1);
        assert_eq!(restored.style_overrides(None, 0).paragraph_spacing_em, None);
        assert_eq!(restored.style_overrides(None, 0).first_line_indent_em, None);
    }

    #[test]
    fn word_spacing_reaches_the_renderer_and_survives_backend_updates() {
        let previous = ReaderPreferences::default();
        let snapshot = serde_json::to_value(&previous).unwrap();

        assert_eq!(serde_json::from_value::<ReaderPreferences>(snapshot).unwrap().synchronized.word_spacing_percent(), 100.0);
        assert_eq!(previous.style_overrides(None, 0).word_spacing_factor, None);
        let mut preferences = previous.clone();
        preferences.apply_preference(ReaderPreferencePatch::WordSpacingPercent(120.0));
        let mut backend = app_preferences::ReaderPreferences::default();
        for patch in previous.synchronized_patches(&preferences) {
            patch.apply(&mut backend).unwrap();
        }
        let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
        restored.apply_synchronized(&backend);
        assert_eq!(restored.style_overrides(None, 0).word_spacing_factor, Some(1.2));
    }

    #[test]
    fn font_weight_choices_reach_the_renderer_and_survive_backend_updates() {
        let previous = ReaderPreferences::default();
        for choice in ReaderFontWeight::ALL {
            let mut preferences = previous.clone();
            preferences.apply_preference(ReaderPreferencePatch::FontWeight(choice));
            let mut backend = app_preferences::ReaderPreferences::default();
            for patch in previous.synchronized_patches(&preferences) {
                patch.apply(&mut backend).unwrap();
            }
            let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
            restored.apply_synchronized(&backend);
            assert_eq!(restored.style_overrides(None, 0).font_weight_adjustment, choice.adjustment());
        }
    }

    #[test]
    fn publisher_line_height_survives_reload_and_backend_preference_updates() {
        let mut preferences = ReaderPreferences::default();
        preferences.apply_preference(ReaderPreferencePatch::LineHeight(1.8));
        let snapshot = serde_json::to_value(&preferences).unwrap();

        let previous: ReaderPreferences = serde_json::from_value(snapshot).unwrap();
        assert!(previous.synchronized.override_line_height());
        assert_eq!(previous.style_overrides(None, 0).line_height, Some(1.8));

        preferences.apply_preference(ReaderPreferencePatch::OverrideLineHeight(false));
        let patches = previous.synchronized_patches(&preferences);
        assert_eq!(patches, vec![ReaderPreferencePatch::OverrideLineHeight(false)]);
        let mut saved = app_preferences::ReaderPreferences::default();
        saved.set_line_height(1.8).unwrap();
        for patch in patches {
            patch.apply(&mut saved).unwrap();
        }
        let mut restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
        restored.apply_synchronized(&saved);
        assert!(!restored.synchronized.override_line_height());
        assert_eq!(restored.synchronized.line_height(), 1.8);
        assert_eq!(restored.style_overrides(None, 0).line_height, None);

        restored.apply_preference(ReaderPreferencePatch::OverrideLineHeight(true));
        assert_eq!(restored.style_overrides(None, 0).line_height, Some(1.8));
    }

    #[test]
    fn ruler_settings_survive_local_reload_without_synchronization() {
        let synchronized = app_preferences::ReaderPreferences::default();
        let mut defaults = ReaderPreferences::default();
        defaults.apply_synchronized(&synchronized);
        let preferences = ReaderPreferences { reading_ruler: html_view_gpui::ReadingRulerStyle::Focus, reading_ruler_last_style: html_view_gpui::ReadingRulerStyle::Underline, reading_ruler_lines: 3, ..defaults.clone() };
        assert!(defaults.synchronized_patches(&preferences).is_empty());
        let mut restored: ReaderPreferences = serde_json::from_str(&serde_json::to_string(&preferences).unwrap()).unwrap();
        restored.apply_synchronized(&synchronized);
        assert_eq!(restored.reading_ruler, preferences.reading_ruler);
        assert_eq!(restored.reading_ruler_last_style, preferences.reading_ruler_last_style);
        assert_eq!(restored.reading_ruler_lines, 3);
        assert!(defaults.synchronized_patches(&restored).is_empty());
        restored.reading_ruler = html_view_gpui::ReadingRulerStyle::Off;
        let disabled: ReaderPreferences = serde_json::from_str(&serde_json::to_string(&restored).unwrap()).unwrap();
        assert_eq!(disabled.reading_ruler_last_style, preferences.reading_ruler_last_style);
        assert!(defaults.synchronized_patches(&disabled).is_empty());
    }

    #[test]
    fn system_font_is_local_and_survives_reload_and_synchronized_updates() {
        let mut preferences = ReaderPreferences::default();
        preferences.system_font_family = Some("Noto Serif".into());
        assert!(ReaderPreferences::default().synchronized_patches(&preferences).is_empty());
        let saved = serde_json::to_value(&preferences).unwrap();
        let mut restored: ReaderPreferences = serde_json::from_value(saved.clone()).unwrap();
        restored.apply_synchronized(&app_preferences::ReaderPreferences::default());
        assert_eq!(restored.system_font_family.as_deref(), Some("Noto Serif"));
        assert_eq!(restored.style_overrides(None, 0).font_family, None);
        assert_eq!(restored.style_overrides(None, 0).serif_font_family.as_deref(), Some("\"Noto Serif\", serif"));
        let snapshot = saved;

        assert_eq!(serde_json::from_value::<ReaderPreferences>(snapshot).unwrap().system_font_family, None);
    }

    #[test]
    fn cjk_profile_is_local_and_independent_of_latin_fonts() {
        let mut preferences = ReaderPreferences::default();
        preferences.set_custom_font_family(ReaderFontRole::Serif, "NV Garamond".into());
        let latin = preferences.style_overrides_for_book(None, None, 0);
        let book = preferences.style_overrides_for_book(Some(CjkFont::Japanese), None, 0);
        assert_eq!(book.font_family, latin.font_family);
        assert_eq!(book.serif_font_family, latin.serif_font_family);
        assert_eq!(book.cjk_font_family, None);
        assert_eq!(book.cjk_font_fallback.as_deref(), Some("\"Droid Sans Fallback\""));
        let before = preferences.clone();
        preferences.set_custom_font_family(ReaderFontRole::Cjk, "A, B's \"CJK\"".into());
        assert!(before.synchronized_patches(&preferences).is_empty());
        assert_eq!(preferences.style_overrides_for_book(None, None, 0), latin);
        let custom = preferences.style_overrides_for_book(Some(CjkFont::Korean), None, 0);
        assert_eq!(custom.serif_font_family, latin.serif_font_family);
        assert_eq!(custom.font_family, latin.font_family);
        assert_eq!(custom.cjk_font_family.as_deref(), Some("\"A, B's \\\"CJK\\\"\""));
        let mut restored: ReaderPreferences = serde_json::from_slice(&serde_json::to_vec(&preferences).unwrap()).unwrap();
        restored.apply_synchronized(&app_preferences::ReaderPreferences::default());
        assert_eq!(restored.cjk_font_choice, CjkFontChoice::Custom);
        assert_eq!(restored.system_cjk_font_family, preferences.system_cjk_font_family);
        restored.cjk_font_choice = CjkFontChoice::Publisher;
        let publisher = restored.style_overrides_for_book(Some(CjkFont::Japanese), None, 0);
        assert_eq!(publisher.cjk_font_family, None);
        assert!(publisher.cjk_font_fallback.is_some());
        assert_eq!(restored.system_cjk_font_family, preferences.system_cjk_font_family);
    }

    #[test]
    fn empty_custom_cjk_uses_embedded_fallback() {
        let mut saved = serde_json::to_value(ReaderPreferences::default()).unwrap();
        let mut preferences: ReaderPreferences = serde_json::from_value(saved).unwrap();
        assert_eq!(preferences.cjk_font_choice, CjkFontChoice::Publisher);
        preferences.set_custom_font_family(ReaderFontRole::Cjk, "  ".into());
        assert!(!preferences.custom_fonts_active());
        let overrides = preferences.style_overrides_for_book(Some(CjkFont::Japanese), None, 0);
        assert_eq!(overrides.cjk_font_family.as_deref(), Some("\"Droid Sans Fallback\""));
        assert_eq!(overrides.cjk_font_fallback.as_deref(), Some("\"Droid Sans Fallback\""));
    }

    #[test]
    fn custom_profile_selects_immediately_and_keeps_fonts_when_using_presets() {
        let mut preferences = ReaderPreferences::default();
        preferences.custom_fonts_enabled = true;
        assert!(preferences.custom_fonts_active());
        assert_eq!(preferences.style_overrides(None, 0).font_family, None);
        preferences.set_custom_font_family(ReaderFontRole::SansSerif, "Noto Serif".into());
        preferences.custom_fonts_enabled = false;
        preferences.apply_preference(ReaderPreferencePatch::FontFamily(ReaderFontFamily::Serif));
        let overrides = preferences.style_overrides(None, 0);
        assert_eq!(overrides.font_family.as_deref(), Some("NV Garamond, serif"));
        assert_eq!(overrides.sans_serif_font_family, None);
        preferences.custom_fonts_enabled = true;
        assert_eq!(preferences.style_overrides(None, 0).sans_serif_font_family.as_deref(), Some("\"Noto Serif\", sans-serif"));
        let restored: ReaderPreferences = serde_json::from_slice(&serde_json::to_vec(&preferences).unwrap()).unwrap();
        assert!(restored.custom_fonts_active());
        assert_eq!(restored.custom_font_family(ReaderFontRole::SansSerif), Some("Noto Serif"));
    }

    #[test]
    fn system_font_names_are_quoted_without_changing_their_identity() {
        let preferences = ReaderPreferences { system_font_family: Some("A, B's \"Serif\" \\ Font".into()), ..Default::default() };
        assert_eq!(preferences.style_overrides(None, 0).serif_font_family.as_deref(), Some(r#""A, B's \"Serif\" \\ Font", serif"#));
    }

    #[test]
    fn all_three_custom_roles_are_independent_local_preferences() {
        let mut preferences = ReaderPreferences::default();
        preferences.set_custom_font_family(ReaderFontRole::Serif, "Noto Sans".into());
        preferences.set_custom_font_family(ReaderFontRole::SansSerif, "Noto Serif".into());
        preferences.set_custom_font_family(ReaderFontRole::Monospace, "Raleway".into());
        assert!(ReaderPreferences::default().synchronized_patches(&preferences).is_empty());
        let restored: ReaderPreferences = serde_json::from_value(serde_json::to_value(&preferences).unwrap()).unwrap();
        let overrides = restored.style_overrides(None, 0);
        assert_eq!(overrides.font_family, None);
        assert_eq!(overrides.serif_font_family.as_deref(), Some("\"Noto Sans\", serif"));
        assert_eq!(overrides.sans_serif_font_family.as_deref(), Some("\"Noto Serif\", sans-serif"));
        assert_eq!(overrides.monospace_font_family.as_deref(), Some("\"Raleway\", monospace"));
    }

    #[gpui::test]
    fn persistence_receives_only_the_fields_changed_by_each_update(cx: &mut TestAppContext) {
        let saves = cx.new(|_| Vec::new());
        let target = saves.clone();
        cx.update(|cx| {
            cx.set_global(ReaderSettings {
                preferences: ReaderPreferences::default(),
                save: Rc::new(move |preferences, patches, cx| {
                    assert_eq!(ReaderSettings::preferences(cx), &preferences);
                    target.update(cx, |saves, _| saves.push((preferences, patches)));
                }),
            });
            assert!(!ReaderSettings::update(cx, |_| {}));
            assert!(ReaderSettings::update(cx, |preferences| preferences.apply_preference(ReaderPreferencePatch::FontSizePx(preferences.synchronized.font_size_px() + (1.0)))));
            assert!(ReaderSettings::update(cx, |preferences| preferences.apply_preference(ReaderPreferencePatch::MaxLineWidthPx(preferences.synchronized.max_line_width_px() + (10.0)))));
            assert!(ReaderSettings::update(cx, |preferences| preferences.chrome_hidden = true));
            assert!(!ReaderSettings::update(cx, |preferences| preferences.chrome_hidden = true));
        });
        saves.read_with(cx, |saves, _| {
            assert_eq!(saves.len(), 3);
            assert_eq!(saves[0].1, vec![ReaderPreferencePatch::FontSizePx(saves[0].0.synchronized.font_size_px())]);
            assert_eq!(saves[1].1, vec![ReaderPreferencePatch::MaxLineWidthPx(saves[1].0.synchronized.max_line_width_px())]);
            assert!(saves[2].0.chrome_hidden);
            assert!(saves[2].1.is_empty(), "local state must not generate synchronized patches");
        });
    }
}

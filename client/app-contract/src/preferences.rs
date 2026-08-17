use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;

pub const FONT_SIZE_MIN: f32 = 8.0;
pub const FONT_SIZE_MAX: f32 = 64.0;
pub const LINE_WIDTH_MIN: f64 = 200.0;
pub const LINE_WIDTH_MAX: f64 = 1200.0;
pub const ZOOM_MIN: f64 = 0.5;
pub const ZOOM_MAX: f64 = 3.0;
pub const LINE_HEIGHT_MIN: f32 = 1.0;
pub const LINE_HEIGHT_MAX: f32 = 2.4;
pub const PDF_ZOOM_MIN: f32 = 0.05;
pub const PDF_ZOOM_MAX: f32 = 10.0;
pub const PDF_PAGE_COUNT_MIN: u8 = 1;
pub const PDF_PAGE_COUNT_MAX: u8 = 6;
pub const COLUMN_COUNT_MIN: u8 = 1;
pub const COLUMN_COUNT_MAX: u8 = 4;
pub const UI_FONT_SIZE_MIN: u8 = 12;
pub const UI_FONT_SIZE_MAX: u8 = 24;
pub const UI_FONT_SIZE_DEFAULT: u8 = 16;
pub const WEB_DESKTOP_MIN_WIDTH_PX: u16 = 720;

fn default_catalog_languages() -> Vec<String> {
    vec!["en".to_owned()]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WebPreferenceProfile {
    Mobile,
    #[default]
    Desktop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WebPreferenceProfilesContract {
    pub profiles: Vec<WebPreferenceProfile>,
    pub desktop_min_width_px: u16,
}

impl Default for WebPreferenceProfilesContract {
    fn default() -> Self {
        Self { profiles: vec![WebPreferenceProfile::Mobile, WebPreferenceProfile::Desktop], desktop_min_width_px: WEB_DESKTOP_MIN_WIDTH_PX }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationTheme {
    #[default]
    GrecoFuturistic,
    LibrarySerif,
}

/// Which of a theme's two palettes to draw.
///
/// Separate from [`ApplicationTheme`] rather than doubling its variants: the
/// theme is a taste choice and the appearance usually is not one at all —
/// most people want it to follow the system, and reading happens at night.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationAppearance {
    #[default]
    System,
    Light,
    Dark,
}

impl ApplicationAppearance {
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::System => "Match the desktop or phone setting.",
            Self::Light => "Always light.",
            Self::Dark => "Always dark.",
        }
    }

    /// Resolves `System` against what the platform reports. Platforms that
    /// cannot report an appearance — the Kobo build — pass `false` and get the
    /// light palette, which is correct for e-ink.
    pub const fn resolve(self, system_prefers_dark: bool) -> ResolvedAppearance {
        match self {
            Self::Light => ResolvedAppearance::Light,
            Self::Dark => ResolvedAppearance::Dark,
            Self::System if system_prefers_dark => ResolvedAppearance::Dark,
            Self::System => ResolvedAppearance::Light,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedAppearance {
    #[default]
    Light,
    Dark,
}

/// One theme at one appearance.
///
/// `card_bg` and `white_surface` used to be two tokens 1.027:1 apart — the same
/// cream twice — and are now a single `raised_bg`. `hover_overlay` carries
/// alpha (`0xRRGGBBAA`) so that one value reads correctly over the page, a
/// panel and a raised card alike; a solid colour would need one token per
/// surface, times four palettes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Type)]
pub struct ApplicationPalette {
    pub surface_bg: u32,
    pub raised_bg: u32,
    pub ink_text: u32,
    pub subtle_text: u32,
    pub accent: u32,
    pub border: u32,
    pub hover_overlay: u32,
    pub active_highlight: u32,
    pub danger_text: u32,
    pub danger_bg: u32,
    /// State that needs a decision but is not a failure: a library holding
    /// unsynced changes, bytes reserved by an upload in flight. Separate from
    /// `accent` so "fine" and "look at this" never render the same, and from
    /// `danger_*` so a pending transfer does not read as an error.
    pub warning_text: u32,
    pub warning_bg: u32,
}

impl ApplicationTheme {
    pub const fn next(self) -> Self {
        match self {
            Self::GrecoFuturistic => Self::LibrarySerif,
            Self::LibrarySerif => Self::GrecoFuturistic,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::GrecoFuturistic => "Sage",
            Self::LibrarySerif => "Sepia",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::GrecoFuturistic => "Cool, restrained surfaces with muted green accents.",
            Self::LibrarySerif => "Warm, bookish surfaces with brown accents.",
        }
    }

    /// Palettes are derived, not hand-picked: every one holds body text at
    /// ~13.5:1, muted text at ~4.9:1, the accent at ~5.5:1, and — the fix for
    /// borders that used to sit at 1.3:1 — a boundary at >=3.2:1 against
    /// whichever adjacent surface it contrasts with least. Hue and saturation
    /// carry each theme's character; only lightness moves.
    pub const fn palette(self, appearance: ResolvedAppearance) -> ApplicationPalette {
        match (self, appearance) {
            (Self::GrecoFuturistic, ResolvedAppearance::Light) => ApplicationPalette {
                surface_bg: 0xf5f1e8,
                raised_bg: 0xfffcf6,
                ink_text: 0x1f2220,
                subtle_text: 0x65675d,
                accent: 0x486754,
                border: 0x8b7e5b,
                hover_overlay: 0x0000000d,
                active_highlight: 0xe1e6d8,
                danger_text: 0x9b312a,
                danger_bg: 0xfbe4da,
                warning_text: 0x8a6000,
                warning_bg: 0xf7e9c6,
            },
            (Self::GrecoFuturistic, ResolvedAppearance::Dark) => ApplicationPalette {
                surface_bg: 0x24231e,
                raised_bg: 0x32302a,
                ink_text: 0xe9ebe9,
                subtle_text: 0x979a8f,
                accent: 0x79a289,
                border: 0x9a8c65,
                hover_overlay: 0xffffff12,
                active_highlight: 0x3c442c,
                danger_text: 0xdd8983,
                danger_bg: 0x401411,
                warning_text: 0xd8a83f,
                warning_bg: 0x3a2f10,
            },
            (Self::LibrarySerif, ResolvedAppearance::Light) => ApplicationPalette {
                surface_bg: 0xf6efe1,
                raised_bg: 0xfffaf2,
                ink_text: 0x231d16,
                subtle_text: 0x70614e,
                accent: 0x7b4f2e,
                border: 0x957a4d,
                hover_overlay: 0x0000000d,
                active_highlight: 0xe8d6b7,
                danger_text: 0x892b25,
                danger_bg: 0xf9e2d7,
                warning_text: 0x866300,
                warning_bg: 0xf9ebcb,
            },
            (Self::LibrarySerif, ResolvedAppearance::Dark) => ApplicationPalette {
                surface_bg: 0x24221e,
                raised_bg: 0x322f29,
                ink_text: 0xede9e3,
                subtle_text: 0xa79580,
                accent: 0xc48c61,
                border: 0xa88a57,
                hover_overlay: 0xffffff12,
                active_highlight: 0x493d27,
                danger_text: 0xde8983,
                danger_bg: 0x401411,
                warning_text: 0xe0b23f,
                warning_bg: 0x3d3011,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UiFontFamily {
    #[serde(alias = "ibm_plex_sans", alias = "liberation_sans")]
    SansSerif,
    #[serde(alias = "source_serif_4", alias = "liberation_serif")]
    Serif,
}

impl UiFontFamily {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SansSerif => "Sans Serif",
            Self::Serif => "Serif",
        }
    }

    pub const fn font_name(self) -> &'static str {
        match self {
            Self::SansSerif => "Noto Sans",
            Self::Serif => "Libertinus Serif",
        }
    }

    pub const fn generic_family(self) -> &'static str {
        match self {
            Self::SansSerif => "sans-serif",
            Self::Serif => "serif",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::SansSerif => Self::Serif,
            Self::Serif => Self::SansSerif,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
#[specta(skip_attr = "serde")]
pub struct BrowsingPreferences {
    theme: ApplicationTheme,
    #[serde(default)]
    appearance: ApplicationAppearance,
    interface_font: UiFontFamily,
    #[specta(type = u8)]
    ui_font_size_px: UiFontSize,
    library_view: LibraryView,
    show_unavailable_books: bool,
    /// Empty means that catalogue searches include every language. New and
    /// pre-language-filter profiles start with English and can explicitly
    /// choose All languages from the catalogue dropdown.
    #[serde(default = "default_catalog_languages")]
    included_catalog_languages: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryView {
    #[default]
    Covers,
    Detailed,
}

impl LibraryView {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Covers => "Covers",
            Self::Detailed => "Detailed",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Covers => "A grid of cover art.",
            Self::Detailed => "Rows with the blurb and actions beside the cover.",
        }
    }
}

impl Default for BrowsingPreferences {
    fn default() -> Self {
        Self {
            theme: ApplicationTheme::default(),
            appearance: ApplicationAppearance::default(),
            interface_font: UiFontFamily::SansSerif,
            ui_font_size_px: UiFontSize(UI_FONT_SIZE_DEFAULT),
            library_view: LibraryView::Covers,
            show_unavailable_books: false,
            included_catalog_languages: default_catalog_languages(),
        }
    }
}

impl BrowsingPreferences {
    pub fn theme(&self) -> ApplicationTheme {
        self.theme
    }

    pub fn appearance(&self) -> ApplicationAppearance {
        self.appearance
    }

    pub fn interface_font(&self) -> UiFontFamily {
        self.interface_font
    }

    pub fn ui_font_size_px(&self) -> u8 {
        self.ui_font_size_px.get()
    }
    pub fn library_view(&self) -> LibraryView {
        self.library_view
    }
    pub fn show_unavailable_books(&self) -> bool {
        self.show_unavailable_books
    }

    pub fn included_catalog_languages(&self) -> &[String] {
        &self.included_catalog_languages
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "preference", content = "value", rename_all = "snake_case")]
pub enum BrowsingPreferencePatch {
    Theme(ApplicationTheme),
    InterfaceFont(UiFontFamily),
    UiFontSizePx(u8),
    Appearance(ApplicationAppearance),
    LibraryView(LibraryView),
    ShowUnavailableBooks(bool),
    IncludedCatalogLanguages(Vec<String>),
}

/// Host-local policy for transfers started by background synchronization.
/// Web hosts may persist it in an account-scoped form-factor profile. Explicit
/// user actions are never suppressed by this preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
#[specta(skip_attr = "serde")]
pub struct TransferPreferences {
    allow_background_on_metered: bool,
    auto_download_limit_mb: Option<u32>,
}

impl TransferPreferences {
    pub const fn allow_background_on_metered(self) -> bool {
        self.allow_background_on_metered
    }

    pub const fn auto_download_limit_mb(self) -> Option<u32> {
        self.auto_download_limit_mb
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "preference", content = "value", rename_all = "snake_case")]
pub enum TransferPreferencePatch {
    AllowBackgroundOnMetered(bool),
    AutoDownloadLimitMb(Option<u32>),
}

impl TransferPreferencePatch {
    pub fn apply(self, preferences: &mut TransferPreferences) {
        match self {
            Self::AllowBackgroundOnMetered(value) => preferences.allow_background_on_metered = value,
            Self::AutoDownloadLimitMb(value) => preferences.auto_download_limit_mb = value,
        }
    }
}

impl BrowsingPreferencePatch {
    pub fn apply(self, preferences: &mut BrowsingPreferences) -> Result<(), PreferenceValueError> {
        match self {
            Self::Theme(value) => preferences.theme = value,
            Self::InterfaceFont(value) => preferences.interface_font = value,
            Self::UiFontSizePx(value) => preferences.ui_font_size_px = UiFontSize::new(value)?,
            Self::Appearance(value) => preferences.appearance = value,
            Self::LibraryView(value) => preferences.library_view = value,
            Self::ShowUnavailableBooks(value) => preferences.show_unavailable_books = value,
            Self::IncludedCatalogLanguages(mut value) => {
                value.retain(|language| CATALOG_LANGUAGE_OPTIONS.iter().any(|option| option.0 == language));
                value.sort();
                value.dedup();
                preferences.included_catalog_languages = value;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ApplicationThemeOption {
    pub value: ApplicationTheme,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UiFontOption {
    pub value: UiFontFamily,
    pub label: String,
    pub font_name: String,
    pub generic_family: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LibraryViewOption {
    pub value: LibraryView,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogLanguageOption {
    pub value: String,
    pub label: String,
}

const CATALOG_LANGUAGE_OPTIONS: [(&str, &str); 15] = [
    ("be", "Belarusian"),
    ("bg", "Bulgarian"),
    ("de", "German"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("hu", "Hungarian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("la", "Latin"),
    ("no", "Norwegian"),
    ("pl", "Polish"),
    ("sv", "Swedish"),
    ("smi", "Sámi languages"),
    ("uk", "Ukrainian"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ApplicationAppearanceOption {
    pub value: ApplicationAppearance,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BrowsingPreferenceOptions {
    pub appearances: Vec<ApplicationAppearanceOption>,
    pub themes: Vec<ApplicationThemeOption>,
    pub fonts: Vec<UiFontOption>,
    pub library_views: Vec<LibraryViewOption>,
    pub catalog_languages: Vec<CatalogLanguageOption>,
}

impl Default for BrowsingPreferenceOptions {
    fn default() -> Self {
        let fonts = [UiFontFamily::SansSerif, UiFontFamily::Serif]
            .into_iter()
            .map(|value| UiFontOption { value, label: value.label().to_string(), font_name: value.font_name().to_string(), generic_family: value.generic_family().to_string() })
            .collect();
        let themes = [ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif].into_iter().map(|value| ApplicationThemeOption { value, label: value.label().to_string(), description: value.description().to_string() }).collect();
        let library_views = [LibraryView::Covers, LibraryView::Detailed].into_iter().map(|value| LibraryViewOption { value, label: value.label().to_string(), description: value.description().to_string() }).collect();
        let appearances = [ApplicationAppearance::System, ApplicationAppearance::Light, ApplicationAppearance::Dark]
            .into_iter()
            .map(|value| ApplicationAppearanceOption { value, label: value.label().to_string(), description: value.description().to_string() })
            .collect();
        let catalog_languages = CATALOG_LANGUAGE_OPTIONS.into_iter().map(|(value, label)| CatalogLanguageOption { value: value.to_owned(), label: label.to_owned() }).collect();
        Self { appearances, themes, fonts, library_views, catalog_languages }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BrowsingPreferencesContract {
    pub defaults: BrowsingPreferences,
    pub options: BrowsingPreferenceOptions,
    pub constraints: BrowsingPreferenceConstraints,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BrowsingPreferenceConstraints {
    pub ui_font_size_px: [u8; 2],
}

impl Default for BrowsingPreferencesContract {
    fn default() -> Self {
        Self { defaults: BrowsingPreferences::default(), options: BrowsingPreferenceOptions::default(), constraints: BrowsingPreferenceConstraints { ui_font_size_px: [UI_FONT_SIZE_MIN, UI_FONT_SIZE_MAX] } }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderFlow {
    #[default]
    Paginated,
    Scrolled,
}

/// How a PDF page's on-screen size is decided.
///
/// Every mode settles the same quantity — the zoom, in pixels per PDF point —
/// and they differ only in which end is held fixed. `pdf_page_count` carries
/// the count for [`PdfZoomMode::FitWidth`] and `pdf_zoom` the zoom for
/// [`PdfZoomMode::Fixed`], so switching modes and back restores what was set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PdfZoomMode {
    /// The page count is chosen and the zoom follows.
    #[default]
    FitWidth,
    /// The zoom is chosen and the page count follows.
    Fixed,
    /// A page's height fills the viewport, and the page count follows.
    FitHeight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderFontFamily {
    #[default]
    Publisher,
    Serif,
    SansSerif,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderTextAlignment {
    #[default]
    Publisher,
    Start,
    Justify,
}

/// Complete reader settings consumed by every frontend. Account-wide fields
/// are synchronized separately; layout-sensitive fields remain in a local
/// device or web form-factor profile. Neither belongs in the library event log.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
#[specta(skip_attr = "serde")]
pub struct ReaderPreferences {
    #[specta(type = specta_typescript::Number)]
    font_size_px: FontSize,
    #[specta(type = specta_typescript::Number)]
    max_line_width_px: LineWidth,
    #[specta(type = specta_typescript::Number)]
    zoom: Zoom,
    #[specta(type = specta_typescript::Number)]
    line_height: LineHeight,
    flow: ReaderFlow,
    font_family: ReaderFontFamily,
    text_alignment: ReaderTextAlignment,
    hyphenation: bool,
    #[specta(type = u8)]
    max_column_count: ColumnCount,
    /// Which end of the PDF layout is held fixed; the rest is derived.
    pdf_zoom_mode: PdfZoomMode,
    /// How many PDF pages sit side by side under [`PdfZoomMode::FitWidth`].
    #[specta(type = u8)]
    pdf_page_count: PdfPageCount,
    /// Pixels per PDF point under [`PdfZoomMode::Fixed`].
    #[specta(type = specta_typescript::Number)]
    pdf_zoom: PdfZoom,
    /// Automatically removes uniform outer margins from rendered PDF pages.
    pdf_trim_margins: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreferenceValueError(&'static str);

impl fmt::Display for PreferenceValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} is outside its supported finite range", self.0)
    }
}

impl std::error::Error for PreferenceValueError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct UiFontSize(u8);

impl UiFontSize {
    fn new(value: u8) -> Result<Self, PreferenceValueError> {
        if (UI_FONT_SIZE_MIN..=UI_FONT_SIZE_MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(PreferenceValueError("UI font size"))
        }
    }

    const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for UiFontSize {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

macro_rules! bounded_float {
    ($name:ident, $primitive:ty, $label:literal, $minimum:expr, $maximum:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq, Serialize)]
        #[serde(transparent)]
        struct $name($primitive);

        impl $name {
            fn new(value: $primitive) -> Result<Self, PreferenceValueError> {
                if value.is_finite() && ($minimum..=$maximum).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(PreferenceValueError($label))
                }
            }

            const fn get(self) -> $primitive {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(<$primitive>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

bounded_float!(FontSize, f32, "font size", FONT_SIZE_MIN, FONT_SIZE_MAX);
bounded_float!(LineWidth, f64, "line width", LINE_WIDTH_MIN, LINE_WIDTH_MAX);
bounded_float!(Zoom, f64, "zoom", ZOOM_MIN, ZOOM_MAX);
bounded_float!(LineHeight, f32, "line height", LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
bounded_float!(PdfZoom, f32, "PDF zoom", PDF_ZOOM_MIN, PDF_ZOOM_MAX);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct ColumnCount(u8);

impl ColumnCount {
    fn new(value: u8) -> Result<Self, PreferenceValueError> {
        if (COLUMN_COUNT_MIN..=COLUMN_COUNT_MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(PreferenceValueError("column count"))
        }
    }

    const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ColumnCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct PdfPageCount(u8);

impl PdfPageCount {
    fn new(value: u8) -> Result<Self, PreferenceValueError> {
        if (PDF_PAGE_COUNT_MIN..=PDF_PAGE_COUNT_MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(PreferenceValueError("PDF page count"))
        }
    }

    const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PdfPageCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Default for ReaderPreferences {
    fn default() -> Self {
        Self {
            font_size_px: FontSize(20.0),
            max_line_width_px: LineWidth(600.0),
            zoom: Zoom(1.0),
            line_height: LineHeight(1.5),
            flow: ReaderFlow::Paginated,
            font_family: ReaderFontFamily::Publisher,
            text_alignment: ReaderTextAlignment::Publisher,
            hyphenation: true,
            max_column_count: ColumnCount(2),
            pdf_zoom_mode: PdfZoomMode::FitWidth,
            pdf_page_count: PdfPageCount(1),
            pdf_zoom: PdfZoom(1.0),
            pdf_trim_margins: true,
        }
    }
}

impl ReaderPreferences {
    pub fn font_size_px(self) -> f32 {
        self.font_size_px.get()
    }
    pub fn max_line_width_px(self) -> f64 {
        self.max_line_width_px.get()
    }
    pub fn zoom(self) -> f64 {
        self.zoom.get()
    }
    pub fn line_height(self) -> f32 {
        self.line_height.get()
    }
    pub fn flow(self) -> ReaderFlow {
        self.flow
    }
    pub fn font_family(self) -> ReaderFontFamily {
        self.font_family
    }
    pub fn text_alignment(self) -> ReaderTextAlignment {
        self.text_alignment
    }
    pub fn hyphenation(self) -> bool {
        self.hyphenation
    }
    pub fn max_column_count(self) -> u8 {
        self.max_column_count.get()
    }
    pub fn pdf_zoom_mode(self) -> PdfZoomMode {
        self.pdf_zoom_mode
    }
    pub fn pdf_page_count(self) -> u8 {
        self.pdf_page_count.get()
    }
    pub fn pdf_zoom(self) -> f32 {
        self.pdf_zoom.get()
    }
    pub fn pdf_trim_margins(self) -> bool {
        self.pdf_trim_margins
    }

    pub fn set_font_size_px(&mut self, value: f32) -> Result<(), PreferenceValueError> {
        self.font_size_px = FontSize::new(value)?;
        Ok(())
    }
    pub fn set_max_line_width_px(&mut self, value: f64) -> Result<(), PreferenceValueError> {
        self.max_line_width_px = LineWidth::new(value)?;
        Ok(())
    }
    pub fn set_zoom(&mut self, value: f64) -> Result<(), PreferenceValueError> {
        self.zoom = Zoom::new(value)?;
        Ok(())
    }
    pub fn set_line_height(&mut self, value: f32) -> Result<(), PreferenceValueError> {
        self.line_height = LineHeight::new(value)?;
        Ok(())
    }
    pub fn set_flow(&mut self, value: ReaderFlow) {
        self.flow = value;
    }
    pub fn set_font_family(&mut self, value: ReaderFontFamily) {
        self.font_family = value;
    }
    pub fn set_text_alignment(&mut self, value: ReaderTextAlignment) {
        self.text_alignment = value;
    }
    pub fn set_hyphenation(&mut self, value: bool) {
        self.hyphenation = value;
    }
    pub fn set_max_column_count(&mut self, value: u8) -> Result<(), PreferenceValueError> {
        self.max_column_count = ColumnCount::new(value)?;
        Ok(())
    }
    pub fn set_pdf_zoom_mode(&mut self, value: PdfZoomMode) {
        self.pdf_zoom_mode = value;
    }
    pub fn set_pdf_page_count(&mut self, value: u8) -> Result<(), PreferenceValueError> {
        self.pdf_page_count = PdfPageCount::new(value)?;
        Ok(())
    }
    pub fn set_pdf_zoom(&mut self, value: f32) -> Result<(), PreferenceValueError> {
        self.pdf_zoom = PdfZoom::new(value)?;
        Ok(())
    }
    pub fn set_pdf_trim_margins(&mut self, value: bool) {
        self.pdf_trim_margins = value;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "preference", content = "value", rename_all = "snake_case")]
pub enum ReaderPreferencePatch {
    #[specta(type = specta_typescript::Number)]
    FontSizePx(f32),
    #[specta(type = specta_typescript::Number)]
    MaxLineWidthPx(f64),
    #[specta(type = specta_typescript::Number)]
    Zoom(f64),
    #[specta(type = specta_typescript::Number)]
    LineHeight(f32),
    Flow(ReaderFlow),
    FontFamily(ReaderFontFamily),
    TextAlignment(ReaderTextAlignment),
    Hyphenation(bool),
    MaxColumnCount(u8),
    PdfZoomMode(PdfZoomMode),
    PdfPageCount(u8),
    #[specta(type = specta_typescript::Number)]
    PdfZoom(f32),
    PdfTrimMargins(bool),
}

impl ReaderPreferencePatch {
    pub fn apply(self, preferences: &mut ReaderPreferences) -> Result<(), PreferenceValueError> {
        match self {
            Self::FontSizePx(value) => preferences.set_font_size_px(value),
            Self::MaxLineWidthPx(value) => preferences.set_max_line_width_px(value),
            Self::Zoom(value) => preferences.set_zoom(value),
            Self::LineHeight(value) => preferences.set_line_height(value),
            Self::Flow(value) => {
                preferences.set_flow(value);
                Ok(())
            }
            Self::FontFamily(value) => {
                preferences.set_font_family(value);
                Ok(())
            }
            Self::TextAlignment(value) => {
                preferences.set_text_alignment(value);
                Ok(())
            }
            Self::Hyphenation(value) => {
                preferences.set_hyphenation(value);
                Ok(())
            }
            Self::MaxColumnCount(value) => preferences.set_max_column_count(value),
            Self::PdfZoomMode(value) => {
                preferences.set_pdf_zoom_mode(value);
                Ok(())
            }
            Self::PdfPageCount(value) => preferences.set_pdf_page_count(value),
            Self::PdfZoom(value) => preferences.set_pdf_zoom(value),
            Self::PdfTrimMargins(value) => {
                preferences.set_pdf_trim_margins(value);
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderEngine {
    Native,
    Foliate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderPreferenceKey {
    FontSizePx,
    MaxLineWidthPx,
    Zoom,
    LineHeight,
    Flow,
    FontFamily,
    TextAlignment,
    Hyphenation,
    MaxColumnCount,
    PdfZoomMode,
    PdfPageCount,
    PdfZoom,
    PdfTrimMargins,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderCapabilities {
    pub engine: ReaderEngine,
    pub supported_preferences: Vec<ReaderPreferenceKey>,
}

impl ReaderCapabilities {
    pub fn for_engine(engine: ReaderEngine) -> Self {
        let supported_preferences = match engine {
            ReaderEngine::Native => vec![
                ReaderPreferenceKey::FontSizePx,
                ReaderPreferenceKey::MaxLineWidthPx,
                ReaderPreferenceKey::Zoom,
                ReaderPreferenceKey::LineHeight,
                ReaderPreferenceKey::FontFamily,
                ReaderPreferenceKey::TextAlignment,
                ReaderPreferenceKey::MaxColumnCount,
                ReaderPreferenceKey::PdfZoomMode,
                ReaderPreferenceKey::PdfPageCount,
                ReaderPreferenceKey::PdfZoom,
                ReaderPreferenceKey::PdfTrimMargins,
            ],
            ReaderEngine::Foliate => vec![
                ReaderPreferenceKey::FontSizePx,
                ReaderPreferenceKey::MaxLineWidthPx,
                ReaderPreferenceKey::Zoom,
                ReaderPreferenceKey::LineHeight,
                ReaderPreferenceKey::Flow,
                ReaderPreferenceKey::FontFamily,
                ReaderPreferenceKey::TextAlignment,
                ReaderPreferenceKey::Hyphenation,
                ReaderPreferenceKey::MaxColumnCount,
            ],
        };
        Self { engine, supported_preferences }
    }

    pub fn supports(&self, preference: ReaderPreferenceKey) -> bool {
        self.supported_preferences.contains(&preference)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ReaderPreferenceConstraints {
    pub font_size_px: [f32; 2],
    pub max_line_width_px: [f64; 2],
    pub zoom: [f64; 2],
    pub line_height: [f32; 2],
    pub max_column_count: [u8; 2],
    pub pdf_page_count: [u8; 2],
    pub pdf_zoom: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderFlowOption {
    pub value: ReaderFlow,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderFontFamilyOption {
    pub value: ReaderFontFamily,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderTextAlignmentOption {
    pub value: ReaderTextAlignment,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PdfZoomModeOption {
    pub value: PdfZoomMode,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderPreferenceOptions {
    pub flows: Vec<ReaderFlowOption>,
    pub font_families: Vec<ReaderFontFamilyOption>,
    pub text_alignments: Vec<ReaderTextAlignmentOption>,
    pub pdf_zoom_modes: Vec<PdfZoomModeOption>,
}

impl Default for ReaderPreferenceOptions {
    fn default() -> Self {
        Self {
            flows: vec![ReaderFlowOption { value: ReaderFlow::Paginated, label: "Pages".to_string() }, ReaderFlowOption { value: ReaderFlow::Scrolled, label: "Scrolling".to_string() }],
            font_families: vec![
                ReaderFontFamilyOption { value: ReaderFontFamily::Publisher, label: "Publisher".to_string() },
                ReaderFontFamilyOption { value: ReaderFontFamily::Serif, label: "Serif".to_string() },
                ReaderFontFamilyOption { value: ReaderFontFamily::SansSerif, label: "Sans serif".to_string() },
            ],
            text_alignments: vec![
                ReaderTextAlignmentOption { value: ReaderTextAlignment::Publisher, label: "Publisher".to_string() },
                ReaderTextAlignmentOption { value: ReaderTextAlignment::Start, label: "Left".to_string() },
                ReaderTextAlignmentOption { value: ReaderTextAlignment::Justify, label: "Justified".to_string() },
            ],
            pdf_zoom_modes: vec![
                PdfZoomModeOption { value: PdfZoomMode::FitWidth, label: "Fit width".to_string() },
                PdfZoomModeOption { value: PdfZoomMode::Fixed, label: "Zoom".to_string() },
                PdfZoomModeOption { value: PdfZoomMode::FitHeight, label: "Fit height".to_string() },
            ],
        }
    }
}

impl Default for ReaderPreferenceConstraints {
    fn default() -> Self {
        Self {
            font_size_px: [FONT_SIZE_MIN, FONT_SIZE_MAX],
            max_line_width_px: [LINE_WIDTH_MIN, LINE_WIDTH_MAX],
            zoom: [ZOOM_MIN, ZOOM_MAX],
            line_height: [LINE_HEIGHT_MIN, LINE_HEIGHT_MAX],
            max_column_count: [COLUMN_COUNT_MIN, COLUMN_COUNT_MAX],
            pdf_page_count: [PDF_PAGE_COUNT_MIN, PDF_PAGE_COUNT_MAX],
            pdf_zoom: [PDF_ZOOM_MIN, PDF_ZOOM_MAX],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ReaderPreferencesContract {
    pub defaults: ReaderPreferences,
    pub constraints: ReaderPreferenceConstraints,
    pub options: ReaderPreferenceOptions,
    pub native_capabilities: ReaderCapabilities,
    pub foliate_capabilities: ReaderCapabilities,
}

impl Default for ReaderPreferencesContract {
    fn default() -> Self {
        Self {
            defaults: ReaderPreferences::default(),
            constraints: ReaderPreferenceConstraints::default(),
            options: ReaderPreferenceOptions::default(),
            native_capabilities: ReaderCapabilities::for_engine(ReaderEngine::Native),
            foliate_capabilities: ReaderCapabilities::for_engine(ReaderEngine::Foliate),
        }
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;

    fn channel(value: f64) -> f64 {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(colour: u32) -> f64 {
        let component = |shift: u32| channel(f64::from((colour >> shift) & 0xff) / 255.0);
        0.2126 * component(16) + 0.7152 * component(8) + 0.0722 * component(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn palettes() -> Vec<(String, ApplicationPalette)> {
        let mut out = Vec::new();
        for theme in [ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif] {
            for appearance in [ResolvedAppearance::Light, ResolvedAppearance::Dark] {
                out.push((format!("{theme:?}/{appearance:?}"), theme.palette(appearance)));
            }
        }
        out
    }

    /// WCAG 1.4.11 sets 3:1 for a boundary that carries meaning. Card and panel
    /// definition rests entirely on these 1px borders, and they used to sit at
    /// 1.3:1 — visible only if you already knew where to look.
    #[test]
    fn borders_separate_from_every_surface_they_touch() {
        for (name, palette) in palettes() {
            for (surface, label) in [(palette.surface_bg, "surface_bg"), (palette.raised_bg, "raised_bg")] {
                let ratio = contrast(palette.border, surface);
                assert!(ratio >= 3.0, "{name}: border against {label} is {ratio:.2}:1, under the 3:1 floor for UI boundaries");
            }
        }
    }

    #[test]
    fn body_text_and_muted_text_pass_wcag_aa() {
        for (name, palette) in palettes() {
            for (surface, label) in [(palette.surface_bg, "surface_bg"), (palette.raised_bg, "raised_bg")] {
                let ink = contrast(palette.ink_text, surface);
                assert!(ink >= 7.0, "{name}: body text on {label} is {ink:.2}:1");
                let muted = contrast(palette.subtle_text, surface);
                assert!(muted >= 4.5, "{name}: muted text on {label} is {muted:.2}:1");
                let accent = contrast(palette.accent, surface);
                assert!(accent >= 4.5, "{name}: accent on {label} is {accent:.2}:1");
                // Warning text is set directly on a surface as often as it is on
                // its own tint — the verdict stripe and the state pills.
                let warning = contrast(palette.warning_text, surface);
                assert!(warning >= 4.5, "{name}: warning on {label} is {warning:.2}:1");
            }
            let danger = contrast(palette.danger_text, palette.danger_bg);
            assert!(danger >= 4.5, "{name}: danger text is {danger:.2}:1");
            let warning = contrast(palette.warning_text, palette.warning_bg);
            assert!(warning >= 4.5, "{name}: warning text is {warning:.2}:1");
        }
    }

    /// The two surfaces must remain visibly distinct: one continuous canvas
    /// and one raised card/popover tone.
    #[test]
    fn the_two_surfaces_are_distinguishable_from_one_another() {
        for (name, palette) in palettes() {
            let ratio = contrast(palette.surface_bg, palette.raised_bg);
            assert!(ratio >= 1.08, "{name}: surface/raised differ by only {ratio:.3}:1");
        }
    }

    /// Hover is an overlay rather than a solid so one value works on every
    /// surface. A fully opaque value would mean it had been written as a solid
    /// colour by mistake, which reads as a colour change rather than a hover.
    #[test]
    fn the_hover_overlay_is_translucent() {
        for (name, palette) in palettes() {
            let alpha = palette.hover_overlay & 0xff;
            assert!(alpha > 0 && alpha < 0x40, "{name}: hover alpha {alpha:#04x} is not a subtle overlay");
        }
    }

    #[test]
    fn a_platform_that_cannot_report_an_appearance_gets_the_light_palette() {
        assert_eq!(ApplicationAppearance::System.resolve(false), ResolvedAppearance::Light);
        assert_eq!(ApplicationAppearance::System.resolve(true), ResolvedAppearance::Dark);
        assert_eq!(ApplicationAppearance::Dark.resolve(false), ResolvedAppearance::Dark, "an explicit choice must ignore the system");
        assert_eq!(ApplicationAppearance::Light.resolve(true), ResolvedAppearance::Light);
    }

    #[test]
    fn light_and_dark_are_actually_light_and_dark() {
        for theme in [ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif] {
            let light = theme.palette(ResolvedAppearance::Light);
            let dark = theme.palette(ResolvedAppearance::Dark);
            assert!(luminance(light.surface_bg) > 0.5, "{theme:?} light background is not light");
            assert!(luminance(dark.surface_bg) < 0.1, "{theme:?} dark background is not dark");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_preference_patches_keep_the_tauri_json_shape() {
        let browsing = BrowsingPreferencePatch::LibraryView(LibraryView::Detailed);
        assert_eq!(serde_json::to_value(browsing).unwrap(), serde_json::json!({ "preference": "library_view", "value": "detailed" }));

        let languages = BrowsingPreferencePatch::IncludedCatalogLanguages(vec!["sv".to_owned(), "en".to_owned()]);
        assert_eq!(serde_json::to_value(languages).unwrap(), serde_json::json!({ "preference": "included_catalog_languages", "value": ["sv", "en"] }));

        let transfer = TransferPreferencePatch::AutoDownloadLimitMb(Some(512));
        assert_eq!(serde_json::to_value(transfer).unwrap(), serde_json::json!({ "preference": "auto_download_limit_mb", "value": 512 }));

        let reader = ReaderPreferencePatch::PdfZoomMode(PdfZoomMode::FitHeight);
        assert_eq!(serde_json::to_value(reader).unwrap(), serde_json::json!({ "preference": "pdf_zoom_mode", "value": "fit_height" }));

        let count = ReaderPreferencePatch::PdfPageCount(3);
        assert_eq!(serde_json::to_value(count).unwrap(), serde_json::json!({ "preference": "pdf_page_count", "value": 3 }));
    }

    #[test]
    fn catalogue_languages_are_normalized_to_supported_unique_values() {
        let mut preferences = BrowsingPreferences::default();
        assert_eq!(preferences.included_catalog_languages(), ["en"]);
        BrowsingPreferencePatch::IncludedCatalogLanguages(vec!["sv".to_owned(), "unknown".to_owned(), "en".to_owned(), "sv".to_owned()]).apply(&mut preferences).unwrap();
        assert_eq!(preferences.included_catalog_languages(), ["en", "sv"]);
        BrowsingPreferencePatch::IncludedCatalogLanguages(Vec::new()).apply(&mut preferences).unwrap();
        assert!(preferences.included_catalog_languages().is_empty());
    }

    #[test]
    fn older_profiles_receive_the_english_catalogue_default() {
        let preferences: BrowsingPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(preferences.included_catalog_languages(), ["en"]);
    }
}

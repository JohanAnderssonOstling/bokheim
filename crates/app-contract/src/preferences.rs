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
pub const COLUMN_COUNT_MIN: u8 = 1;
pub const COLUMN_COUNT_MAX: u8 = 4;
pub const UI_FONT_SIZE_MIN: u8 = 12;
pub const UI_FONT_SIZE_MAX: u8 = 24;
pub const UI_FONT_SIZE_DEFAULT: u8 = 16;
pub const WEB_DESKTOP_MIN_WIDTH_PX: u16 = 720;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Type)]
pub struct ApplicationPalette {
    pub app_bg: u32,
    pub surface_bg: u32,
    pub card_bg: u32,
    pub ink_text: u32,
    pub subtle_text: u32,
    pub accent: u32,
    pub border: u32,
    pub hover_bg: u32,
    pub active_highlight: u32,
    pub white_surface: u32,
    pub danger_text: u32,
    pub danger_bg: u32,
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
            Self::GrecoFuturistic => "Greco futuristic",
            Self::LibrarySerif => "Library serif",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::GrecoFuturistic => "Cool, restrained surfaces with muted green accents.",
            Self::LibrarySerif => "Warm, bookish surfaces with brown accents.",
        }
    }

    pub const fn palette(self) -> ApplicationPalette {
        match self {
            Self::GrecoFuturistic => ApplicationPalette {
                app_bg: 0xeeebe2,
                surface_bg: 0xf7f4ec,
                card_bg: 0xfffcf6,
                ink_text: 0x1f2220,
                subtle_text: 0x65675d,
                accent: 0x486754,
                border: 0xd5cfbf,
                hover_bg: 0xf0ebe0,
                active_highlight: 0xe1e6d8,
                white_surface: 0xfcf9f2,
                danger_text: 0x9b312a,
                danger_bg: 0xfbe4da,
            },
            Self::LibrarySerif => ApplicationPalette {
                app_bg: 0xefe8db,
                surface_bg: 0xf8f3e8,
                card_bg: 0xfffaf2,
                ink_text: 0x231d16,
                subtle_text: 0x70614e,
                accent: 0x7b4f2e,
                border: 0xd3c4aa,
                hover_bg: 0xf2e8d5,
                active_highlight: 0xe8d6b7,
                white_surface: 0xfcf7ed,
                danger_text: 0x892b25,
                danger_bg: 0xf9e2d7,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
#[specta(skip_attr = "serde")]
pub struct BrowsingPreferences {
    theme: ApplicationTheme,
    interface_font: UiFontFamily,
    heading_font: UiFontFamily,
    #[specta(type = u8)]
    ui_font_size_px: UiFontSize,
    library_view: LibraryView,
    show_unavailable_books: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryView {
    #[default]
    Covers,
    Detailed,
}

impl Default for BrowsingPreferences {
    fn default() -> Self {
        Self {
            theme: ApplicationTheme::default(),
            interface_font: UiFontFamily::SansSerif,
            heading_font: UiFontFamily::Serif,
            ui_font_size_px: UiFontSize(UI_FONT_SIZE_DEFAULT),
            library_view: LibraryView::Covers,
            show_unavailable_books: false,
        }
    }
}

impl BrowsingPreferences {
    pub fn theme(self) -> ApplicationTheme {
        self.theme
    }

    pub fn interface_font(self) -> UiFontFamily {
        self.interface_font
    }

    pub fn heading_font(self) -> UiFontFamily {
        self.heading_font
    }

    pub fn ui_font_size_px(self) -> u8 {
        self.ui_font_size_px.get()
    }
    pub fn library_view(self) -> LibraryView {
        self.library_view
    }
    pub fn show_unavailable_books(self) -> bool {
        self.show_unavailable_books
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "preference", content = "value", rename_all = "snake_case")]
pub enum BrowsingPreferencePatch {
    Theme(ApplicationTheme),
    InterfaceFont(UiFontFamily),
    HeadingFont(UiFontFamily),
    UiFontSizePx(u8),
    LibraryView(LibraryView),
    ShowUnavailableBooks(bool),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
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
            Self::HeadingFont(value) => preferences.heading_font = value,
            Self::UiFontSizePx(value) => preferences.ui_font_size_px = UiFontSize::new(value)?,
            Self::LibraryView(value) => preferences.library_view = value,
            Self::ShowUnavailableBooks(value) => preferences.show_unavailable_books = value,
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
    pub font_name: String,
    pub generic_family: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BrowsingPreferenceOptions {
    pub themes: Vec<ApplicationThemeOption>,
    pub fonts: Vec<UiFontOption>,
}

impl Default for BrowsingPreferenceOptions {
    fn default() -> Self {
        let fonts = [UiFontFamily::SansSerif, UiFontFamily::Serif].into_iter().map(|value| UiFontOption { value, font_name: value.font_name().to_string(), generic_family: value.generic_family().to_string() }).collect();
        let themes = [ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif].into_iter().map(|value| ApplicationThemeOption { value, label: value.label().to_string(), description: value.description().to_string() }).collect();
        Self { themes, fonts }
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
pub enum ReaderTheme {
    Light,
    #[default]
    Sepia,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderFlow {
    #[default]
    Paginated,
    Scrolled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PdfPageLayout {
    Single,
    Spread,
    #[default]
    Automatic,
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
    theme: ReaderTheme,
    flow: ReaderFlow,
    font_family: ReaderFontFamily,
    text_alignment: ReaderTextAlignment,
    hyphenation: bool,
    #[specta(type = u8)]
    max_column_count: ColumnCount,
    /// Controls how many PDF pages the native reader presents together.
    pdf_page_layout: PdfPageLayout,
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

impl Default for ReaderPreferences {
    fn default() -> Self {
        Self {
            font_size_px: FontSize(20.0),
            max_line_width_px: LineWidth(600.0),
            zoom: Zoom(1.0),
            line_height: LineHeight(1.5),
            theme: ReaderTheme::Sepia,
            flow: ReaderFlow::Paginated,
            font_family: ReaderFontFamily::Publisher,
            text_alignment: ReaderTextAlignment::Publisher,
            hyphenation: true,
            max_column_count: ColumnCount(2),
            pdf_page_layout: PdfPageLayout::Automatic,
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
    pub fn theme(self) -> ReaderTheme {
        self.theme
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
    pub fn pdf_page_layout(self) -> PdfPageLayout {
        self.pdf_page_layout
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
    pub fn set_theme(&mut self, value: ReaderTheme) {
        self.theme = value;
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
    pub fn set_pdf_page_layout(&mut self, value: PdfPageLayout) {
        self.pdf_page_layout = value;
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
    Theme(ReaderTheme),
    Flow(ReaderFlow),
    FontFamily(ReaderFontFamily),
    TextAlignment(ReaderTextAlignment),
    Hyphenation(bool),
    MaxColumnCount(u8),
    PdfPageLayout(PdfPageLayout),
    PdfTrimMargins(bool),
}

impl ReaderPreferencePatch {
    pub fn apply(self, preferences: &mut ReaderPreferences) -> Result<(), PreferenceValueError> {
        match self {
            Self::FontSizePx(value) => preferences.set_font_size_px(value),
            Self::MaxLineWidthPx(value) => preferences.set_max_line_width_px(value),
            Self::Zoom(value) => preferences.set_zoom(value),
            Self::LineHeight(value) => preferences.set_line_height(value),
            Self::Theme(value) => {
                preferences.set_theme(value);
                Ok(())
            }
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
            Self::PdfPageLayout(value) => {
                preferences.set_pdf_page_layout(value);
                Ok(())
            }
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
    Theme,
    Flow,
    FontFamily,
    TextAlignment,
    Hyphenation,
    MaxColumnCount,
    PdfPageLayout,
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
                ReaderPreferenceKey::Theme,
                ReaderPreferenceKey::FontFamily,
                ReaderPreferenceKey::TextAlignment,
                ReaderPreferenceKey::MaxColumnCount,
                ReaderPreferenceKey::PdfPageLayout,
                ReaderPreferenceKey::PdfTrimMargins,
            ],
            ReaderEngine::Foliate => vec![
                ReaderPreferenceKey::FontSizePx,
                ReaderPreferenceKey::MaxLineWidthPx,
                ReaderPreferenceKey::Zoom,
                ReaderPreferenceKey::LineHeight,
                ReaderPreferenceKey::Theme,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderThemeOption {
    pub value: ReaderTheme,
    pub label: String,
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
pub struct PdfPageLayoutOption {
    pub value: PdfPageLayout,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderPreferenceOptions {
    pub themes: Vec<ReaderThemeOption>,
    pub flows: Vec<ReaderFlowOption>,
    pub font_families: Vec<ReaderFontFamilyOption>,
    pub text_alignments: Vec<ReaderTextAlignmentOption>,
    pub pdf_page_layouts: Vec<PdfPageLayoutOption>,
}

impl Default for ReaderPreferenceOptions {
    fn default() -> Self {
        Self {
            themes: vec![
                ReaderThemeOption { value: ReaderTheme::Light, label: "Light".to_string() },
                ReaderThemeOption { value: ReaderTheme::Sepia, label: "Sepia".to_string() },
                ReaderThemeOption { value: ReaderTheme::Dark, label: "Dark".to_string() },
            ],
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
            pdf_page_layouts: vec![
                PdfPageLayoutOption { value: PdfPageLayout::Single, label: "Single page".to_string() },
                PdfPageLayoutOption { value: PdfPageLayout::Spread, label: "Two-page spread".to_string() },
                PdfPageLayoutOption { value: PdfPageLayout::Automatic, label: "Automatic".to_string() },
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
mod tests {
    use super::*;

    #[test]
    fn local_preference_patches_keep_the_tauri_json_shape() {
        let browsing = BrowsingPreferencePatch::LibraryView(LibraryView::Detailed);
        assert_eq!(serde_json::to_value(browsing).unwrap(), serde_json::json!({ "preference": "library_view", "value": "detailed" }));

        let transfer = TransferPreferencePatch::AutoDownloadLimitMb(Some(512));
        assert_eq!(serde_json::to_value(transfer).unwrap(), serde_json::json!({ "preference": "auto_download_limit_mb", "value": 512 }));

        let reader = ReaderPreferencePatch::PdfPageLayout(PdfPageLayout::Spread);
        assert_eq!(serde_json::to_value(reader).unwrap(), serde_json::json!({ "preference": "pdf_page_layout", "value": "spread" }));
    }
}

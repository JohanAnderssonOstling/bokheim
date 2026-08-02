use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const PREFERENCES_FILE: &str = "reader_preferences.json";
const BROWSING_PREFERENCES_FILE: &str = "browsing_preferences.json";
const TRANSFER_PREFERENCES_FILE: &str = "transfer_preferences.json";
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationTheme {
    GrecoFuturistic,
    LibrarySerif,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

impl Default for ApplicationTheme {
    fn default() -> Self {
        Self::GrecoFuturistic
    }
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
            Self::SansSerif => "sans-serif",
            Self::Serif => "serif",
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
}

impl Default for BrowsingPreferences {
    fn default() -> Self {
        Self { theme: ApplicationTheme::default(), interface_font: UiFontFamily::SansSerif, heading_font: UiFontFamily::Serif, ui_font_size_px: UiFontSize(UI_FONT_SIZE_DEFAULT) }
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "preference", content = "value", rename_all = "snake_case")]
pub enum BrowsingPreferencePatch {
    Theme(ApplicationTheme),
    InterfaceFont(UiFontFamily),
    HeadingFont(UiFontFamily),
    UiFontSizePx(u8),
}

/// Device-local policy for transfers started by background synchronization.
/// Explicit user actions are never suppressed by this preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
#[specta(skip_attr = "serde")]
pub struct TransferPreferences {
    allow_background_on_metered: bool,
}

impl TransferPreferences {
    pub const fn allow_background_on_metered(self) -> bool {
        self.allow_background_on_metered
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "preference", content = "value", rename_all = "snake_case")]
pub enum TransferPreferencePatch {
    AllowBackgroundOnMetered(bool),
}

impl TransferPreferencePatch {
    pub fn apply(self, preferences: &mut TransferPreferences) {
        match self {
            Self::AllowBackgroundOnMetered(value) => preferences.allow_background_on_metered = value,
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

/// Device-local reading preferences shared by all frontends.
///
/// These values deliberately live outside the library databases and sync log.
/// Each installation gets its own preferences file in its application-data
/// directory.
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
    /// Supported by the native renderer. Foliate ignores this preference.
    line_numbers: bool,
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
            line_numbers: true,
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
    pub fn line_numbers(self) -> bool {
        self.line_numbers
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
    pub fn set_line_numbers(&mut self, value: bool) {
        self.line_numbers = value;
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
    LineNumbers(bool),
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
            Self::LineNumbers(value) => {
                preferences.set_line_numbers(value);
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
    LineNumbers,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReaderCapabilities {
    pub engine: ReaderEngine,
    pub supported_preferences: Vec<ReaderPreferenceKey>,
}

impl ReaderCapabilities {
    pub fn for_engine(engine: ReaderEngine) -> Self {
        let supported_preferences = match engine {
            ReaderEngine::Native => vec![ReaderPreferenceKey::FontSizePx, ReaderPreferenceKey::MaxLineWidthPx, ReaderPreferenceKey::Zoom, ReaderPreferenceKey::LineNumbers],
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
    #[specta(type = [specta_typescript::Number; 2])]
    pub font_size_px: [f32; 2],
    #[specta(type = [specta_typescript::Number; 2])]
    pub max_line_width_px: [f64; 2],
    #[specta(type = [specta_typescript::Number; 2])]
    pub zoom: [f64; 2],
    #[specta(type = [specta_typescript::Number; 2])]
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
pub struct ReaderPreferenceOptions {
    pub themes: Vec<ReaderThemeOption>,
    pub flows: Vec<ReaderFlowOption>,
    pub font_families: Vec<ReaderFontFamilyOption>,
    pub text_alignments: Vec<ReaderTextAlignmentOption>,
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

impl crate::home::BackendContext {
    pub fn transfer_preferences_path(&self) -> PathBuf {
        self.app_data_dir().join(TRANSFER_PREFERENCES_FILE)
    }

    pub fn load_transfer_preferences(&self) -> io::Result<TransferPreferences> {
        match fs::read(self.transfer_preferences_path()) {
            Ok(bytes) => serde_json::from_slice::<TransferPreferences>(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(TransferPreferences::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save_transfer_preferences(&self, preferences: TransferPreferences) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_preferences_atomic(&self.transfer_preferences_path(), &bytes)
    }

    pub fn apply_transfer_preference_patch(&self, patch: TransferPreferencePatch) -> io::Result<TransferPreferences> {
        let mut preferences = self.load_transfer_preferences()?;
        patch.apply(&mut preferences);
        self.save_transfer_preferences(preferences)?;
        Ok(preferences)
    }

    pub fn browsing_preferences_path(&self) -> PathBuf {
        self.app_data_dir().join(BROWSING_PREFERENCES_FILE)
    }

    pub fn load_browsing_preferences(&self) -> io::Result<BrowsingPreferences> {
        match fs::read(self.browsing_preferences_path()) {
            Ok(bytes) => serde_json::from_slice::<BrowsingPreferences>(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BrowsingPreferences::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save_browsing_preferences(&self, preferences: BrowsingPreferences) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_preferences_atomic(&self.browsing_preferences_path(), &bytes)
    }

    pub fn apply_browsing_preference_patch(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        let mut preferences = self.load_browsing_preferences()?;
        patch.apply(&mut preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.save_browsing_preferences(preferences)?;
        Ok(preferences)
    }

    pub fn reader_preferences_path(&self) -> PathBuf {
        self.app_data_dir().join(PREFERENCES_FILE)
    }

    pub fn load_reader_preferences(&self) -> io::Result<ReaderPreferences> {
        match fs::read(self.reader_preferences_path()) {
            Ok(bytes) => serde_json::from_slice::<ReaderPreferences>(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ReaderPreferences::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save_reader_preferences(&self, preferences: ReaderPreferences) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_preferences_atomic(&self.reader_preferences_path(), &bytes)
    }

    pub fn apply_reader_preference_patch(&self, patch: ReaderPreferencePatch) -> io::Result<ReaderPreferences> {
        let mut preferences = self.load_reader_preferences()?;
        patch.apply(&mut preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.save_reader_preferences(preferences)?;
        Ok(preferences)
    }
}

fn write_preferences_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "preferences path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".preferences-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_theme_is_a_closed_persisted_preference() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::Theme(ApplicationTheme::LibrarySerif)).unwrap();
        assert_eq!(updated.theme(), ApplicationTheme::LibrarySerif);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        assert!(serde_json::from_str::<BrowsingPreferences>(r#"{"theme":"unknown"}"#).is_err());
    }

    #[test]
    fn ui_font_choices_are_independent_closed_persisted_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::InterfaceFont(UiFontFamily::Serif)).unwrap();
        assert_eq!(updated.interface_font(), UiFontFamily::Serif);
        assert_eq!(updated.heading_font(), UiFontFamily::Serif);
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::HeadingFont(UiFontFamily::SansSerif)).unwrap();
        assert_eq!(updated.interface_font(), UiFontFamily::Serif);
        assert_eq!(updated.heading_font(), UiFontFamily::SansSerif);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        assert!(serde_json::from_str::<BrowsingPreferences>(r#"{"interface_font":"unknown"}"#).is_err());
    }

    #[test]
    fn browsing_options_cover_every_closed_value_once() {
        let options = BrowsingPreferenceOptions::default();
        assert_eq!(options.themes.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif]);
        assert!(options.themes.iter().all(|option| !option.label.is_empty() && !option.description.is_empty()));
        assert_eq!(ApplicationTheme::GrecoFuturistic.next(), ApplicationTheme::LibrarySerif);
        assert_eq!(ApplicationTheme::LibrarySerif.next(), ApplicationTheme::GrecoFuturistic);
        assert_eq!(options.fonts.iter().map(|option| option.value).collect::<Vec<_>>(), vec![UiFontFamily::SansSerif, UiFontFamily::Serif]);
        assert!(options.fonts.iter().all(|option| !option.font_name.is_empty()));
        assert!(options.fonts.iter().all(|option| !option.generic_family.is_empty()));
    }

    #[test]
    fn old_browsing_preferences_receive_independent_font_defaults() {
        let preferences: BrowsingPreferences = serde_json::from_str(r#"{"theme":"library_serif"}"#).unwrap();
        assert_eq!(preferences.theme(), ApplicationTheme::LibrarySerif);
        assert_eq!(preferences.interface_font(), UiFontFamily::SansSerif);
        assert_eq!(preferences.heading_font(), UiFontFamily::Serif);
        assert_eq!(preferences.ui_font_size_px(), UI_FONT_SIZE_DEFAULT);
    }

    #[test]
    fn ui_font_size_is_bounded_and_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MIN)).unwrap();
        assert_eq!(updated.ui_font_size_px(), UI_FONT_SIZE_MIN);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        assert!(context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MIN - 1)).is_err());
        assert!(context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MAX + 1)).is_err());
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MAX)).unwrap();
        assert_eq!(updated.ui_font_size_px(), UI_FONT_SIZE_MAX);
        assert!(serde_json::from_str::<BrowsingPreferences>(&format!(r#"{{"ui_font_size_px":{}}}"#, UI_FONT_SIZE_MIN - 1)).is_err());
    }

    #[test]
    fn exact_font_preferences_migrate_to_system_font_categories() {
        let preferences: BrowsingPreferences = serde_json::from_str(r#"{"interface_font":"ibm_plex_sans","heading_font":"source_serif_4"}"#).unwrap();
        assert_eq!(preferences.interface_font(), UiFontFamily::SansSerif);
        assert_eq!(preferences.heading_font(), UiFontFamily::Serif);
        let preferences: BrowsingPreferences = serde_json::from_str(r#"{"interface_font":"liberation_sans","heading_font":"liberation_serif"}"#).unwrap();
        assert_eq!(preferences.interface_font(), UiFontFamily::SansSerif);
        assert_eq!(preferences.heading_font(), UiFontFamily::Serif);
    }

    #[test]
    fn rejects_invalid_numeric_preferences() {
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"font_size_px":null}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"max_line_width_px":9000.0}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"zoom":0.1}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"line_height":9.0}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"max_column_count":0}"#).is_err());
    }

    #[test]
    fn missing_json_fields_receive_current_defaults() {
        let preferences: ReaderPreferences = serde_json::from_str(r#"{"font_size_px":24.0}"#).unwrap();
        assert_eq!(preferences.font_size_px(), 24.0);
        assert_eq!(preferences.max_line_width_px(), 600.0);
        assert_eq!(preferences.theme(), ReaderTheme::Sepia);
    }

    #[test]
    fn atomic_preferences_write_replaces_the_complete_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reader_preferences.json");
        write_preferences_atomic(&path, br#"{"font_size_px":20.0}"#).unwrap();
        write_preferences_atomic(&path, br#"{"font_size_px":24.0}"#).unwrap();
        assert_eq!(fs::read(&path).unwrap(), br#"{"font_size_px":24.0}"#);
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn background_metered_policy_is_device_local_and_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        assert!(!context.load_transfer_preferences().unwrap().allow_background_on_metered());

        let saved = context.apply_transfer_preference_patch(TransferPreferencePatch::AllowBackgroundOnMetered(true)).unwrap();
        assert!(saved.allow_background_on_metered());
        assert!(context.load_transfer_preferences().unwrap().allow_background_on_metered());
        assert!(context.transfer_preferences_path().starts_with(context.app_data_dir()));
    }

    #[test]
    fn client_preference_contexts_do_not_leak_state_between_instances() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(first_dir.path().to_path_buf())).unwrap();
        let second = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(second_dir.path().to_path_buf())).unwrap();
        let mut first_preferences = ReaderPreferences::default();
        first_preferences.set_zoom(1.75).unwrap();
        first.save_reader_preferences(first_preferences).unwrap();

        assert_eq!(first.load_reader_preferences().unwrap().zoom(), 1.75);
        assert_eq!(second.load_reader_preferences().unwrap().zoom(), ReaderPreferences::default().zoom());
        assert!(!second.reader_preferences_path().exists());
    }

    #[test]
    fn malformed_persisted_preferences_fail_closed_without_being_rewritten() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let malformed = br#"{"zoom":99.0}"#;
        fs::write(context.reader_preferences_path(), malformed).unwrap();

        let error = context.load_reader_preferences().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(context.reader_preferences_path()).unwrap(), malformed);
    }

    #[test]
    fn preference_patch_validates_before_atomically_replacing_persisted_state() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::home::BackendContext::initialize(crate::home::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_reader_preference_patch(ReaderPreferencePatch::FontSizePx(28.0)).unwrap();
        assert_eq!(updated.font_size_px(), 28.0);

        let before_invalid_patch = fs::read(context.reader_preferences_path()).unwrap();
        let error = context.apply_reader_preference_patch(ReaderPreferencePatch::FontSizePx(100.0)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(context.reader_preferences_path()).unwrap(), before_invalid_patch);
    }

    #[test]
    fn reader_capabilities_make_renderer_specific_support_explicit() {
        let native = ReaderCapabilities::for_engine(ReaderEngine::Native);
        let foliate = ReaderCapabilities::for_engine(ReaderEngine::Foliate);
        assert!(native.supports(ReaderPreferenceKey::LineNumbers));
        assert!(!native.supports(ReaderPreferenceKey::Theme));
        assert!(!foliate.supports(ReaderPreferenceKey::LineNumbers));
        assert!(foliate.supports(ReaderPreferenceKey::Theme));
    }

    #[test]
    fn exported_constraints_are_the_same_constants_used_by_validation() {
        let constraints = ReaderPreferenceConstraints::default();
        assert_eq!(constraints.font_size_px, [FONT_SIZE_MIN, FONT_SIZE_MAX]);
        assert!(ReaderPreferences::default().set_font_size_px(constraints.font_size_px[0]).is_ok());
        assert!(ReaderPreferences::default().set_font_size_px(constraints.font_size_px[1]).is_ok());
        assert!(ReaderPreferences::default().set_font_size_px(constraints.font_size_px[1] + 1.0).is_err());
    }

    #[test]
    fn preference_options_cover_every_closed_enum_value_once() {
        let options = ReaderPreferenceOptions::default();
        assert_eq!(options.themes.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderTheme::Light, ReaderTheme::Sepia, ReaderTheme::Dark]);
        assert_eq!(options.flows.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderFlow::Paginated, ReaderFlow::Scrolled]);
        assert_eq!(options.font_families.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderFontFamily::Publisher, ReaderFontFamily::Serif, ReaderFontFamily::SansSerif]);
        assert_eq!(options.text_alignments.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderTextAlignment::Publisher, ReaderTextAlignment::Start, ReaderTextAlignment::Justify]);
    }
}

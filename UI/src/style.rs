use epub_svelte_lib::preferences::{ApplicationTheme, UiFontFamily};
use floem::peniko::Color;
use floem::prelude::{RwSignal, SignalGet};
use floem::reactive::use_context;
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Clone, Copy)]
pub struct ThemeContext {
    pub theme: RwSignal<ApplicationTheme>,
}

#[derive(Clone, Copy)]
pub struct TypographyContext {
    pub interface_font: RwSignal<UiFontFamily>,
    pub heading_font: RwSignal<UiFontFamily>,
}

#[derive(Clone)]
pub struct ThemePalette {
    pub app_bg: Color,
    pub surface_bg: Color,
    pub card_bg: Color,
    pub ink_text: Color,
    pub subtle_text: Color,
    pub accent: Color,
    pub border_color: Color,
    pub hover_bg: Color,
    pub active_highlight: Color,
    pub disabled_text: Color,
    pub disabled_border: Color,
    pub white_surface: Color,
    pub box_shadow: Color,
    pub danger_text: Color,
    pub danger_bg: Color,
    pub split_handle_bg: Color,
    pub split_handle_hover: Color,
    pub split_handle_active: Color,
    pub overlay_scrim: Color,
}

#[derive(Deserialize)]
struct ThemeDefinition {
    app_bg: String,
    surface_bg: String,
    card_bg: String,
    ink_text: String,
    subtle_text: String,
    accent: String,
    border_color: String,
    hover_bg: String,
    active_highlight: String,
    disabled_text: String,
    disabled_border: String,
    white_surface: String,
    box_shadow: String,
    danger_text: String,
    danger_bg: String,
    split_handle_bg: String,
    split_handle_hover: String,
    split_handle_active: String,
    overlay_scrim: String,
}

#[derive(Deserialize)]
struct ThemeDefinitions {
    library_serif: ThemeDefinition,
    greco_futuristic: ThemeDefinition,
}

#[derive(Deserialize)]
struct DesignContract {
    themes: ThemeDefinitions,
}

impl ThemeDefinition {
    fn into_palette(self) -> ThemePalette {
        ThemePalette {
            app_bg: parse_color(&self.app_bg),
            surface_bg: parse_color(&self.surface_bg),
            card_bg: parse_color(&self.card_bg),
            ink_text: parse_color(&self.ink_text),
            subtle_text: parse_color(&self.subtle_text),
            accent: parse_color(&self.accent),
            border_color: parse_color(&self.border_color),
            hover_bg: parse_color(&self.hover_bg),
            active_highlight: parse_color(&self.active_highlight),
            disabled_text: parse_color(&self.disabled_text),
            disabled_border: parse_color(&self.disabled_border),
            white_surface: parse_color(&self.white_surface),
            box_shadow: parse_color(&self.box_shadow),
            danger_text: parse_color(&self.danger_text),
            danger_bg: parse_color(&self.danger_bg),
            split_handle_bg: parse_color(&self.split_handle_bg),
            split_handle_hover: parse_color(&self.split_handle_hover),
            split_handle_active: parse_color(&self.split_handle_active),
            overlay_scrim: parse_color(&self.overlay_scrim),
        }
    }
}

fn parse_color(value: &str) -> Color {
    let hexadecimal = value.strip_prefix('#').expect("shared color must start with #");
    let byte = |offset| u8::from_str_radix(&hexadecimal[offset..offset + 2], 16).expect("shared color must contain hexadecimal bytes");
    match hexadecimal.len() {
        6 => Color::rgb8(byte(0), byte(2), byte(4)),
        8 => Color::rgba8(byte(0), byte(2), byte(4), byte(6)),
        _ => panic!("shared color must use #RRGGBB or #RRGGBBAA"),
    }
}

static THEMES: LazyLock<(ThemePalette, ThemePalette)> = LazyLock::new(|| {
    let contract: DesignContract = serde_json::from_str(include_str!("../../FrontendContract/design_tokens.json")).expect("shared design themes must be valid");
    (contract.themes.greco_futuristic.into_palette(), contract.themes.library_serif.into_palette())
});

pub fn palette_for(theme: ApplicationTheme) -> ThemePalette {
    match theme {
        ApplicationTheme::GrecoFuturistic => THEMES.0.clone(),
        ApplicationTheme::LibrarySerif => THEMES.1.clone(),
    }
}

pub fn active_application_theme() -> ApplicationTheme {
    use_context::<ThemeContext>().map(|context| context.theme.get()).unwrap_or_default()
}

pub fn palette() -> ThemePalette {
    palette_for(active_application_theme())
}

pub fn interface_font_family() -> String {
    use_context::<TypographyContext>().map(|typography| typography.interface_font.get().font_name().to_string()).unwrap_or_else(|| UiFontFamily::SansSerif.font_name().to_string())
}

pub fn heading_font_family() -> String {
    use_context::<TypographyContext>().map(|typography| typography.heading_font.get().font_name().to_string()).unwrap_or_else(|| UiFontFamily::Serif.font_name().to_string())
}

pub fn theme_button_label() -> String {
    format!("Theme: {}", active_application_theme().label())
}

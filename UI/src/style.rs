use floem::peniko::Color;
use floem::prelude::{Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::reactive::use_context;
use floem::style::CursorStyle;
use floem::text::Weight;
use floem::views::{label, Img, Label};

#[cfg(all(feature = "theme-library-serif", feature = "theme-greco-futuristic"))]
compile_error!("Enable only one theme feature: `theme-library-serif` or `theme-greco-futuristic`.");

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ThemeMode {
    GrecoFuturistic,
    LibrarySerif,
}

#[derive(Clone, Copy)]
pub struct ThemeContext {
    pub mode: RwSignal<ThemeMode>,
}

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub ui_font_family: &'static str,
    pub title_font_family: &'static str,
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

const LIBRARY_SERIF_THEME: ThemePalette = ThemePalette {
    ui_font_family: "IBM Plex Sans",
    title_font_family: "Source Serif 4",
    app_bg: Color::rgb8(244, 241, 234),
    surface_bg: Color::rgb8(236, 231, 220),
    card_bg: Color::rgb8(250, 247, 239),
    ink_text: Color::rgb8(31, 27, 22),
    subtle_text: Color::rgb8(107, 98, 87),
    accent: Color::rgb8(138, 106, 47),
    border_color: Color::rgb8(207, 197, 180),
    hover_bg: Color::rgb8(241, 236, 226),
    active_highlight: Color::rgb8(235, 225, 203),
    disabled_text: Color::rgb8(157, 146, 132),
    disabled_border: Color::rgb8(212, 204, 190),
    white_surface: Color::rgb8(255, 253, 248),
    box_shadow: Color::rgba8(44, 35, 24, 70),
    danger_text: Color::rgb8(150, 34, 34),
    danger_bg: Color::rgb8(252, 232, 225),
    split_handle_bg: Color::rgb8(207, 197, 180),
    split_handle_hover: Color::rgb8(176, 161, 136),
    split_handle_active: Color::rgb8(138, 106, 47),
    overlay_scrim: Color::rgba8(30, 24, 16, 120),
};

const GRECO_FUTURISTIC_THEME: ThemePalette = ThemePalette {
    ui_font_family: "IBM Plex Sans",
    title_font_family: "Source Serif 4",
    app_bg: Color::rgb8(250, 251, 252),
    surface_bg: Color::rgb8(243, 245, 247),
    card_bg: Color::rgb8(255, 255, 255),
    ink_text: Color::rgb8(26, 29, 36),
    subtle_text: Color::rgb8(112, 125, 142),
    accent: Color::rgb8(201, 169, 106),
    border_color: Color::rgb8(221, 228, 236),
    hover_bg: Color::rgb8(246, 249, 252),
    active_highlight: Color::rgb8(240, 246, 252),
    disabled_text: Color::rgb8(167, 177, 190),
    disabled_border: Color::rgb8(224, 231, 238),
    white_surface: Color::rgb8(255, 255, 255),
    box_shadow: Color::rgba8(80, 100, 130, 35),
    danger_text: Color::rgb8(190, 35, 35),
    danger_bg: Color::rgb8(255, 232, 232),
    split_handle_bg: Color::rgb8(224, 231, 238),
    split_handle_hover: Color::rgb8(174, 185, 200),
    split_handle_active: Color::rgb8(201, 169, 106),
    overlay_scrim: Color::rgba8(30, 24, 16, 120),
};

pub const DEFAULT_THEME_MODE: ThemeMode = {
    #[cfg(feature = "theme-library-serif")]
    {
        ThemeMode::LibrarySerif
    }
    #[cfg(not(feature = "theme-library-serif"))]
    {
        ThemeMode::GrecoFuturistic
    }
};

pub fn palette_for(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::GrecoFuturistic => GRECO_FUTURISTIC_THEME,
        ThemeMode::LibrarySerif => LIBRARY_SERIF_THEME,
    }
}

pub fn active_theme_mode() -> ThemeMode {
    use_context::<ThemeContext>().map(|theme| theme.mode.get()).unwrap_or(DEFAULT_THEME_MODE)
}

pub fn palette() -> ThemePalette {
    palette_for(active_theme_mode())
}

pub fn theme_button_label() -> String {
    match active_theme_mode() {
        ThemeMode::GrecoFuturistic => "Theme: Greco".to_string(),
        ThemeMode::LibrarySerif => "Theme: Serif".to_string(),
    }
}

pub fn toggle_theme() {
    if let Some(theme) = use_context::<ThemeContext>() {
        theme.mode.update(|mode| {
            *mode = match *mode {
                ThemeMode::GrecoFuturistic => ThemeMode::LibrarySerif,
                ThemeMode::LibrarySerif => ThemeMode::GrecoFuturistic,
            };
        });
    }
}

pub fn create_label(text: String, font_size: i32) -> Label {
    label(move || text.clone()).style(move |s| {
        let theme = palette();
        s.hover(|s| s.background(theme.hover_bg))
            .text_ellipsis()
            .font_size(font_size)
            .font_family(theme.ui_font_family.to_string())
            .font_weight(Weight::LIGHT)
            .color(theme.ink_text)
    })
}

pub fn label_style(label: Label, font_size: i32) -> Label {
    label.style(move |s| {
        let theme = palette();
        s.hover(|s| s.background(theme.hover_bg)).text_ellipsis().font_size(font_size).font_family(theme.ui_font_family.to_string()).color(theme.ink_text)
    })
}

pub fn create_book_title_label(title: String) -> Label {
    label(move || title.clone()).style(|s| {
        let theme = palette();
        s.width(196).height(46).font_size(17).font_family(theme.title_font_family.to_string()).font_weight(Weight::SEMIBOLD).color(theme.ink_text).text_ellipsis().margin_top(10)
    })
}

pub fn style_book_cover(img: Img) -> Img {
    img.style(move |s| {
        let theme = palette();
        s.width(200)
            .height(320)
            .border_radius(6)
            .border(1)
            .border_color(theme.border_color)
            .box_shadow_blur(14)
            .box_shadow_color(theme.box_shadow)
            .box_shadow_spread(0)
            .box_shadow_h_offset(2)
            .box_shadow_v_offset(8)
            .cursor(CursorStyle::Pointer)
    })
}

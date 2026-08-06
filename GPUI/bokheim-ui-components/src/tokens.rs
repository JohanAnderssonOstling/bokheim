use gpui::{App, Hsla};
use gpui_component::ActiveTheme;

#[derive(Clone, Copy)]
pub struct BrowserTheme {
    pub page_bg: Hsla,
    pub surface: Hsla,
    pub surface_alt: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub card_bg: Hsla,
    pub shadow: Hsla,
    pub overlay_scrim: Hsla,
    pub danger: Hsla,
}

pub fn browser_theme(cx: &App) -> BrowserTheme {
    let active = cx.theme();
    let mut shadow = active.border;
    shadow.a = 0.25;
    let mut overlay_scrim = active.foreground;
    overlay_scrim.a = 0.5;
    BrowserTheme {
        page_bg: active.background,
        surface: active.title_bar,
        surface_alt: active.secondary,
        border: active.border,
        text: active.foreground,
        text_muted: active.muted_foreground,
        accent: active.primary,
        accent_soft: active.accent,
        card_bg: active.group_box,
        shadow,
        overlay_scrim,
        danger: active.danger_foreground,
    }
}

pub const PAGE_BG: u32 = 0xeeebe2;
pub const SURFACE: u32 = 0xf7f4ec;
pub const SURFACE_ALT: u32 = 0xfcf9f2;
pub const BORDER: u32 = 0xd5cfbf;
pub const TEXT: u32 = 0x1f2220;
pub const TEXT_MUTED: u32 = 0x65675d;
pub const ACCENT: u32 = 0x486754;
pub const ACCENT_SOFT: u32 = 0xe1e6d8;
pub const CARD_BG: u32 = 0xfffcf6;
pub const SHADOW: u32 = 0x2b271f2a;
pub const BOOK_COVER_WIDTH: f32 = bokheim_app_contract::LARGE_BOOK_COVER_WIDTH_PX as f32;
pub const BOOK_COVER_HEIGHT: f32 = bokheim_app_contract::LARGE_BOOK_COVER_HEIGHT_PX as f32;
pub const BOOK_GRID_GAP: f32 = bokheim_app_contract::LARGE_BOOK_GRID_COLUMN_GAP_PX as f32;
pub const BOOK_GRID_ROW_HEIGHT: f32 = bokheim_app_contract::LARGE_BOOK_GRID_ROW_HEIGHT_PX as f32;
pub const NAV_RAIL_WIDTH: f32 = bokheim_app_contract::LARGE_SIDEBAR_WIDTH_PX as f32;
pub const TOPBAR_HEIGHT: f32 = bokheim_app_contract::LARGE_TOPBAR_HEIGHT_PX as f32;
pub const TOPBAR_ACTION_HEIGHT: f32 = bokheim_app_contract::LARGE_TOPBAR_ACTION_HEIGHT_PX as f32;
pub const TOPBAR_ACTION_GAP: f32 = bokheim_app_contract::LARGE_TOPBAR_ACTION_GAP_PX as f32;
pub const TOPBAR_ACTION_HORIZONTAL_PADDING: f32 = bokheim_app_contract::LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX as f32;
pub const CONTENT_PADDING: f32 = bokheim_app_contract::LARGE_CONTENT_PADDING_PX as f32;
pub const AUTHOR_PAGE_MAX_WIDTH: f32 = 1200.0;
pub const FOLDER_CARD_WIDTH: f32 = 280.0;
pub const FOLDER_GRID_GAP: f32 = 7.0;

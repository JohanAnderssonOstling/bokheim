use gpui::{App, FontWeight, Hsla};
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
    pub raised_bg: Hsla,
    /// Translucent: composited over whichever surface it lands on. Solid hover
    /// colours were the reason hover was invisible on the page background.
    pub hover: Hsla,
    pub shadow: Hsla,
    pub overlay_scrim: Hsla,
    pub danger: Hsla,
    /// State that needs a decision but is not a failure. Kept apart from
    /// `accent`, so a settled page carries no state colour at all, and from
    /// `danger`, so a pending transfer does not read as an error.
    pub warning: Hsla,
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
        raised_bg: active.group_box,
        hover: active.muted,
        shadow,
        overlay_scrim,
        danger: active.danger,
        warning: active.warning,
    }
}

/// The one elevation in the interface: something floating above the page —
/// a dialog, a popover, an editor. Everything else is defined by its border
/// and its surface, not by a shadow.
///
/// Book covers use a separate, tighter shadow that follows the artwork itself;
/// this larger shadow is reserved for floating interface surfaces.
pub fn overlay_shadow(theme: BrowserTheme) -> Vec<gpui::BoxShadow> {
    vec![gpui_component::box_shadow(gpui::px(0.0), gpui::px(14.0), gpui::px(34.0), gpui::px(0.0), theme.shadow)]
}

/// Spacing scale. Every gap, padding and margin picks from these; anything
/// that is a *component dimension* (a control height, a panel width) is a
/// named constant further down instead.
///
/// Mirrors `apps/ui/design-tokens/design_tokens.json`, which is the same scale
/// expressed for the web frontend.
pub const SPACE_XXS: f32 = 2.0;
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 16.0;

/// Native surfaces and controls use square corners. Semantic pills and
/// progress indicators may still opt into a fully rounded shape explicitly.
pub const RADIUS_SM: f32 = app_contract::SURFACE_CORNER_RADIUS_PX as f32;
pub const RADIUS_MD: f32 = app_contract::SURFACE_CORNER_RADIUS_PX as f32;

/// Type scale, in px. Applied as `rems(TEXT_* / 16.0)` so it tracks the user's
/// UI font size rather than being absolute.
pub const TEXT_XS: f32 = 12.0;
pub const TEXT_SM: f32 = 13.0;
pub const TEXT_MD: f32 = 15.0;
pub const TEXT_LG: f32 = 17.0;
pub const TITLE_MD: f32 = 21.0;
pub const TITLE_LG: f32 = 28.0;

pub const BOOK_COVER_WIDTH: f32 = app_contract::LARGE_BOOK_COVER_WIDTH_PX as f32;
pub const BOOK_COVER_HEIGHT: f32 = app_contract::LARGE_BOOK_COVER_HEIGHT_PX as f32;
pub const BOOK_COVER_ASPECT_RATIO: f32 = 1.0 / 1.5;
pub const BOOK_GRID_GAP: f32 = SPACE_XS;
pub const BOOK_GRID_BOOK_GAP: f32 = SPACE_SM;
pub const BOOK_GRID_MAX_GAP: f32 = SPACE_MD + SPACE_SM;
/// Book-card text reserves two title lines and one author line. Keep those
/// blocks in rems: the interface font can have different glyph widths, and the
/// user's UI font size must enlarge the card instead of being clipped by the
/// 16px-reference geometry in the application contract.
pub const BOOK_CARD_TITLE_SIZE: f32 = 16.0;
pub const BOOK_CARD_TITLE_WEIGHT: FontWeight = FontWeight(app_contract::frontend::BOOK_CARD_TITLE_WEIGHT as f32);
pub const BOOK_CARD_TITLE_LINE_HEIGHT_EM: f32 = 1.25;
pub const BOOK_CARD_TITLE_LINES: usize = app_contract::frontend::BOOK_CARD_TITLE_LINES as usize;
pub const BOOK_CARD_AUTHOR_SIZE: f32 = TEXT_XS;
pub const BOOK_CARD_AUTHOR_LINE_HEIGHT_EM: f32 = 1.0;
/// Compact native-card spacing. The mobile frontend retains the shared
/// contract's larger touch-oriented spacing.
pub const BOOK_CARD_COVER_CONTENT_GAP: f32 = SPACE_SM;
pub const BOOK_CARD_TEXT_GAP: f32 = SPACE_XS;
pub const BOOK_CARD_TITLE_HEIGHT_REM: f32 = BOOK_CARD_TITLE_SIZE / 16.0 * BOOK_CARD_TITLE_LINE_HEIGHT_EM * BOOK_CARD_TITLE_LINES as f32;
pub const BOOK_CARD_AUTHOR_HEIGHT_REM: f32 = BOOK_CARD_AUTHOR_SIZE / 16.0 * BOOK_CARD_AUTHOR_LINE_HEIGHT_EM;
pub const BOOK_CARD_FIXED_HEIGHT: f32 = BOOK_CARD_COVER_CONTENT_GAP + BOOK_CARD_TEXT_GAP;

pub const CAROUSEL_TITLE_SIZE: f32 = app_contract::frontend::CAROUSEL_TITLE_SIZE_PX as f32;
pub const CAROUSEL_TITLE_WEIGHT: FontWeight = FontWeight(app_contract::frontend::CAROUSEL_TITLE_WEIGHT as f32);
pub const CAROUSEL_SECTION_GAP: f32 = SPACE_MD;
pub const CAROUSEL_CONTENT_GAP: f32 = SPACE_MD;
pub const CAROUSEL_CONTROL_GAP: f32 = SPACE_XS;
pub const CAROUSEL_ARROW_SIZE: f32 = app_contract::frontend::CAROUSEL_ARROW_SIZE_PX as f32;

/// The navigation rail and library switcher scale with the UI rem; ordinary
/// destinations are shorter so the full route set remains visible.
pub const NAV_RAIL_SIZE_REM: f32 = 4.0;
pub const NAV_RAIL_ITEM_HEIGHT_REM: f32 = 2.75;
pub const NAV_RAIL_ICON_SIZE_REM: f32 = 1.5;
pub const BOTTOM_NAV_HEIGHT_REM: f32 = 4.0;
pub const TOPBAR_HEIGHT_REM: f32 = 4.0;
pub const TOPBAR_ACTION_HEIGHT: f32 = app_contract::LARGE_TOPBAR_ACTION_HEIGHT_PX as f32;
pub const TOPBAR_ACTION_LABEL_WEIGHT: FontWeight = FontWeight(app_contract::TOPBAR_ACTION_LABEL_WEIGHT as f32);
pub const TOPBAR_ACTION_GAP: f32 = SPACE_SM;
pub const TOPBAR_ACTION_HORIZONTAL_PADDING: f32 = SPACE_SM;
pub const FOLDER_CARD_WIDTH: f32 = 280.0;

/// Compact hierarchy items used when folders and publications share the same
/// paginator. These preserve the flat, borderless treatment of the original
/// hierarchy chips while keeping their geometry out of feature code.
pub const COMPACT_HIERARCHY_ITEM_HEIGHT_REM: f32 = NAV_RAIL_ITEM_HEIGHT_REM;
pub const COMPACT_HIERARCHY_ITEM_HEIGHT_OFFSET: f32 = -BOOK_GRID_GAP;
pub const COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING: f32 = SPACE_SM;
pub const COMPACT_HIERARCHY_ITEM_CONTENT_GAP: f32 = SPACE_SM;

/// Store topbar controls share the hierarchy topbar's action height so search,
/// dropdowns, navigation, and paging align when the pages are switched.
pub const CATALOG_CONTROL_HEIGHT: f32 = TOPBAR_ACTION_HEIGHT;

/// Shared geometry for segmented settings controls and steppers. Keeping
/// these dimensions here prevents individual settings surfaces from drifting.
pub const SETTINGS_CONTROL_HEIGHT: f32 = 36.0;
pub const SETTINGS_CHOICE_SEGMENT_WIDTH: f32 = 116.0;
pub const SETTINGS_STEPPER_VALUE_WIDTH: f32 = 72.0;
pub const SETTINGS_CONTROL_GAP: f32 = 1.0;

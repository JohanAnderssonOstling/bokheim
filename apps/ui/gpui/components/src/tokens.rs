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
pub const SPACE_XS: f32 = 5.0;
pub const SPACE_SM: f32 = 9.0;
pub const SPACE_MD: f32 = 14.0;
pub const SPACE_LG: f32 = 18.0;
pub const SPACE_XL: f32 = 28.0;

/// Native surfaces and controls use square corners. Semantic pills and
/// progress indicators may still opt into a fully rounded shape explicitly.
pub const RADIUS_SM: f32 = app_contract::SURFACE_CORNER_RADIUS_PX as f32;
pub const RADIUS_MD: f32 = app_contract::SURFACE_CORNER_RADIUS_PX as f32;
pub const RADIUS_LG: f32 = app_contract::SURFACE_CORNER_RADIUS_PX as f32;

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
pub const BOOK_GRID_GAP: f32 = app_contract::LARGE_BOOK_GRID_COLUMN_GAP_PX as f32;
/// Book-card text reserves two title lines and one author line. Keep those
/// blocks in rems: the interface font can have different glyph widths, and the
/// user's UI font size must enlarge the card instead of being clipped by the
/// 16px-reference geometry in the application contract.
pub const BOOK_CARD_TITLE_SIZE: f32 = app_contract::frontend::BOOK_CARD_TITLE_SIZE_PX as f32;
pub const BOOK_CARD_TITLE_WEIGHT: FontWeight = FontWeight(app_contract::frontend::BOOK_CARD_TITLE_WEIGHT as f32);
pub const BOOK_CARD_TITLE_LINE_HEIGHT: f32 = app_contract::frontend::BOOK_CARD_TITLE_LINE_HEIGHT_PX as f32;
pub const BOOK_CARD_TITLE_LINES: usize = app_contract::frontend::BOOK_CARD_TITLE_LINES as usize;
pub const BOOK_CARD_AUTHOR_SIZE: f32 = app_contract::frontend::BOOK_CARD_AUTHOR_SIZE_PX as f32;
pub const BOOK_CARD_AUTHOR_LINE_HEIGHT: f32 = app_contract::frontend::BOOK_CARD_AUTHOR_LINE_HEIGHT_PX as f32;
pub const BOOK_CARD_COVER_CONTENT_GAP: f32 = app_contract::frontend::BOOK_CARD_COVER_CONTENT_GAP_PX as f32;
pub const BOOK_CARD_TEXT_GAP: f32 = app_contract::frontend::BOOK_CARD_TEXT_GAP_PX as f32;
pub const BOOK_CARD_TITLE_HEIGHT_REM: f32 = BOOK_CARD_TITLE_LINE_HEIGHT * BOOK_CARD_TITLE_LINES as f32 / 16.0;
pub const BOOK_CARD_AUTHOR_HEIGHT_REM: f32 = BOOK_CARD_AUTHOR_LINE_HEIGHT / 16.0;
pub const BOOK_CARD_PROGRESS_HEIGHT: f32 = app_contract::frontend::BOOK_CARD_PROGRESS_HEIGHT_PX as f32;
pub const BOOK_GRID_ROW_TRAILING_SPACE: f32 = app_contract::frontend::BOOK_GRID_ROW_TRAILING_SPACE_PX as f32;

/// Cover, title, and author: the stable visual identity shared by library and
/// catalogue cards. Controls and optional progress must not make this block
/// taller, or paired description columns stop aligning with the cover card.
pub fn book_card_identity_height(rem_size: f32) -> f32 {
    BOOK_COVER_HEIGHT + BOOK_CARD_COVER_CONTENT_GAP + BOOK_CARD_TITLE_HEIGHT_REM * rem_size + BOOK_CARD_TEXT_GAP + BOOK_CARD_AUTHOR_HEIGHT_REM * rem_size + BOOK_GRID_ROW_TRAILING_SPACE
}

pub fn book_grid_row_height(rem_size: f32) -> f32 {
    book_grid_row_height_for_cover_width(BOOK_COVER_WIDTH, rem_size)
}

pub fn book_grid_row_height_for_cover_width(cover_width: f32, rem_size: f32) -> f32 {
    let cover_height = cover_width * BOOK_COVER_HEIGHT / BOOK_COVER_WIDTH;
    cover_height + BOOK_CARD_COVER_CONTENT_GAP + BOOK_CARD_TITLE_HEIGHT_REM * rem_size + BOOK_CARD_TEXT_GAP + BOOK_CARD_AUTHOR_HEIGHT_REM * rem_size + BOOK_CARD_TEXT_GAP + BOOK_CARD_PROGRESS_HEIGHT + BOOK_GRID_ROW_TRAILING_SPACE
}
/// A carousel arrow advances by most of what is on screen rather than by a
/// fixed number of cards, so the step follows the window instead of fighting
/// it. The fraction — and the square control the arrows sit in — are the
/// mobile shelf's, so both frontends step the same distance.
pub const CAROUSEL_STEP_FRACTION: f32 = 0.72;
pub const CAROUSEL_TITLE_SIZE: f32 = app_contract::frontend::CAROUSEL_TITLE_SIZE_PX as f32;
pub const CAROUSEL_TITLE_WEIGHT: FontWeight = FontWeight(app_contract::frontend::CAROUSEL_TITLE_WEIGHT as f32);
pub const CAROUSEL_SECTION_GAP: f32 = app_contract::frontend::CAROUSEL_SECTION_GAP_PX as f32;
pub const CAROUSEL_CONTENT_GAP: f32 = app_contract::frontend::CAROUSEL_CONTENT_GAP_PX as f32;
pub const CAROUSEL_CONTROL_GAP: f32 = app_contract::frontend::CAROUSEL_CONTROL_GAP_PX as f32;
pub const CAROUSEL_ARROW_SIZE: f32 = app_contract::frontend::CAROUSEL_ARROW_SIZE_PX as f32;

/// An author's row carries the covers of their most recently read books rather
/// than an initial in a disc. The row height is what two covers at the 1:1.6
/// ratio need.
pub const AUTHOR_ROW_COVER_COUNT: usize = app_contract::frontend::AUTHOR_ROW_COVER_COUNT as usize;
pub const AUTHOR_ROW_COVER_WIDTH: f32 = app_contract::frontend::AUTHOR_ROW_COVER_WIDTH_PX as f32;
pub const AUTHOR_ROW_COVER_HEIGHT: f32 = app_contract::frontend::AUTHOR_ROW_COVER_HEIGHT_PX as f32;
pub const AUTHOR_ROW_HEIGHT: f32 = app_contract::frontend::AUTHOR_ROW_HEIGHT_PX as f32;
pub const AUTHOR_ROW_COVER_GAP: f32 = app_contract::frontend::AUTHOR_ROW_COVER_GAP_PX as f32;
pub const AUTHOR_ROW_CONTENT_GAP: f32 = app_contract::frontend::AUTHOR_ROW_CONTENT_GAP_PX as f32;
pub const AUTHOR_ROW_HORIZONTAL_PADDING: f32 = app_contract::frontend::AUTHOR_ROW_HORIZONTAL_PADDING_PX as f32;
pub const AUTHOR_ROW_LABEL_SIZE: f32 = app_contract::frontend::AUTHOR_ROW_LABEL_SIZE_PX as f32;

pub const DETAILED_ROW_THUMBNAIL_WIDTH: f32 = app_contract::DETAILED_ROW_THUMBNAIL_WIDTH_PX as f32;
pub const DETAILED_ROW_THUMBNAIL_HEIGHT: f32 = app_contract::DETAILED_ROW_THUMBNAIL_HEIGHT_PX as f32;
pub const DETAILED_ROW_HEIGHT: f32 = app_contract::DETAILED_ROW_HEIGHT_PX as f32;
pub const DETAILED_ROW_VERTICAL_PADDING: f32 = app_contract::DETAILED_ROW_VERTICAL_PADDING_PX as f32;
pub const DETAILED_ROW_TEXT_MEASURE: f32 = app_contract::DETAILED_ROW_TEXT_MEASURE_PX as f32;
pub const DETAILED_ROW_DESCRIPTION_LINES: usize = app_contract::DETAILED_ROW_DESCRIPTION_LINES as usize;
pub const DETAILED_ROW_ACTIONS_BREAKPOINT: f32 = app_contract::DETAILED_ROW_ACTIONS_BREAKPOINT_PX as f32;
pub const NAV_RAIL_WIDTH: f32 = app_contract::LARGE_SIDEBAR_WIDTH_PX as f32;
pub const NAV_RAIL_LABEL_SIZE: f32 = app_contract::NAVIGATION_LABEL_SIZE_PX as f32;
pub const NAV_RAIL_LABEL_WEIGHT: FontWeight = FontWeight(app_contract::NAVIGATION_LABEL_WEIGHT as f32);
pub const NAV_RAIL_ICON_SIZE: f32 = app_contract::NAVIGATION_ICON_SIZE_PX as f32;
pub const NAV_RAIL_ICON_LABEL_GAP: f32 = app_contract::NAVIGATION_ICON_LABEL_GAP_PX as f32;
pub const NAV_RAIL_ITEM_MIN_HEIGHT: f32 = app_contract::NAVIGATION_ITEM_MIN_HEIGHT_PX as f32;
pub const NAV_RAIL_ITEM_MIN_HEIGHT_REM: f32 = NAV_RAIL_ITEM_MIN_HEIGHT / 16.0;
pub const NAV_RAIL_SWITCHER_HEIGHT_REM: f32 = 72.0 / 16.0;
pub const NAV_RAIL_WIDTH_REM: f32 = NAV_RAIL_WIDTH / 16.0;

/// Preserve the approved 96px rail at the default and smaller type sizes, then
/// give larger text proportionally more horizontal room.
pub fn nav_rail_width(rem_size: f32) -> f32 {
    NAV_RAIL_WIDTH.max(NAV_RAIL_WIDTH_REM * rem_size)
}
pub const TOPBAR_HEIGHT: f32 = app_contract::LARGE_TOPBAR_HEIGHT_PX as f32;
pub const TOPBAR_ACTION_HEIGHT: f32 = app_contract::LARGE_TOPBAR_ACTION_HEIGHT_PX as f32;
pub const TOPBAR_ACTION_LABEL_WEIGHT: FontWeight = FontWeight(app_contract::TOPBAR_ACTION_LABEL_WEIGHT as f32);
pub const TOPBAR_ACTION_GAP: f32 = app_contract::LARGE_TOPBAR_ACTION_GAP_PX as f32;
pub const TOPBAR_ACTION_HORIZONTAL_PADDING: f32 = app_contract::LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX as f32;
pub const CONTENT_PADDING: f32 = app_contract::LARGE_CONTENT_PADDING_PX as f32;
pub const AUTHOR_PAGE_MAX_WIDTH: f32 = 1200.0;
pub const FOLDER_CARD_WIDTH: f32 = 280.0;
/// Prevent malformed or unusually long hierarchy labels from creating
/// unbounded controls while allowing normal labels to determine their width.
pub const HIERARCHY_ITEM_MAX_WIDTH: f32 = 640.0;
/// A hierarchy cell holds one line, so it is shorter than the buttons it
/// replaces. The minimum width is what a folder name plus its count needs
/// before truncation starts to bite.
#[cfg(not(feature = "kobo"))]
pub const HIERARCHY_CELL_HEIGHT: f32 = app_contract::HIERARCHY_CELL_HEIGHT_PX as f32;
#[cfg(feature = "kobo")]
pub const HIERARCHY_CELL_HEIGHT: f32 = app_contract::HIERARCHY_TOUCH_CELL_MIN_HEIGHT_PX as f32;
// A cell starts at the approved uniform-grid width, grows with its row, and
// wraps when another minimum-width cell fits. Including the inter-cell margin
// in the maximum makes the first transition continuous: one maximum-width cell
// occupies exactly the same space as two minimum-width cells.
pub const HIERARCHY_CELL_MIN_WIDTH: f32 = app_contract::HIERARCHY_CELL_MIN_WIDTH_PX as f32;
pub const HIERARCHY_CELL_MARGIN: f32 = app_contract::HIERARCHY_CELL_GAP_PX as f32 / 2.0;
pub const HIERARCHY_ICON_SIZE: f32 = app_contract::HIERARCHY_ICON_SIZE_PX as f32;
pub const HIERARCHY_LABEL_SIZE: f32 = app_contract::HIERARCHY_LABEL_SIZE_PX as f32;
pub const HIERARCHY_CELL_MAX_WIDTH: f32 = HIERARCHY_CELL_MIN_WIDTH * 2.0 + HIERARCHY_CELL_MARGIN * 2.0;

/// A hierarchy cell is a row of text, so its geometry follows the reader's UI
/// font size while the margin between cells stays put. Styling can say
/// `rems(..)` and let the window resolve it, but the virtual grid measures in
/// real pixels, so these take the rem size explicitly.
pub const HIERARCHY_CELL_HEIGHT_REM: f32 = HIERARCHY_CELL_HEIGHT / 16.0;
pub const HIERARCHY_CELL_MIN_WIDTH_REM: f32 = HIERARCHY_CELL_MIN_WIDTH / 16.0;

pub fn hierarchy_cell_height(rem_size: f32) -> f32 {
    HIERARCHY_CELL_HEIGHT_REM * rem_size
}

pub fn hierarchy_cell_min_width(rem_size: f32) -> f32 {
    HIERARCHY_CELL_MIN_WIDTH_REM * rem_size
}

/// Kept continuous with the minimum, as the constant is: one maximum-width cell
/// occupies exactly the space of two minimum-width cells.
pub fn hierarchy_cell_max_width(rem_size: f32) -> f32 {
    hierarchy_cell_min_width(rem_size) * 2.0 + HIERARCHY_CELL_MARGIN * 2.0
}

/// The covers are fixed art and the name is text, so only the name's block
/// follows the font. At the default 16px this is the contract's stated height.
pub fn author_row_height(rem_size: f32) -> f32 {
    AUTHOR_ROW_COVER_HEIGHT + BOOK_CARD_TITLE_HEIGHT_REM * rem_size + AUTHOR_ROW_CONTENT_GAP + AUTHOR_ROW_HORIZONTAL_PADDING * 2.0
}
/// Leave a small amount of unused row width so fractional pixels and borders
/// can never make the layout wider than its measured container.
pub const HIERARCHY_LAYOUT_SAFETY: f32 = 4.0;
pub const FOLDER_GRID_GAP: f32 = SPACE_XS;

/// Store topbar controls share the hierarchy topbar's action height so search,
/// dropdowns, navigation, and paging align when the pages are switched.
pub const CATALOG_CONTROL_HEIGHT: f32 = TOPBAR_ACTION_HEIGHT;
pub const CATALOG_CHIP_HEIGHT: f32 = 24.0;
pub const CATALOG_SEARCH_WIDTH: f32 = 240.0;
/// Wide enough for a facet label beside its count, narrow enough that moving
/// the filters here never costs a publication column.
pub const CATALOG_FILTER_RAIL_WIDTH: f32 = 150.0;

/// Shared geometry for segmented settings controls and steppers. Keeping
/// these dimensions here prevents individual settings surfaces from drifting.
pub const SETTINGS_CONTROL_HEIGHT: f32 = 36.0;
pub const SETTINGS_CHOICE_SEGMENT_WIDTH: f32 = 116.0;
pub const SETTINGS_STEPPER_VALUE_WIDTH: f32 = 72.0;
pub const SETTINGS_CONTROL_GAP: f32 = 1.0;

/// Returns the stable column count and common cell width for a hierarchy grid.
///
/// A column is added only after every existing cell reaches its maximum, and
/// removed only when every current cell would fall below its minimum. The
/// whole range between those thresholds is deliberate hysteresis. Once the
/// available children occupy their columns, their cells keep growing to fill
/// the row instead of turning the remaining width into an outer margin.
pub fn hierarchy_grid_layout(available_width: f32, child_count: usize, previous_columns: usize, rem_size: f32) -> (usize, f32) {
    let minimum_width = hierarchy_cell_min_width(rem_size);
    let gap = HIERARCHY_CELL_MARGIN * 2.0;
    let minimum_layout_width = |columns: usize| columns as f32 * minimum_width + columns.saturating_sub(1) as f32 * gap;
    let maximum_layout_width = |columns: usize| columns as f32 * hierarchy_cell_max_width(rem_size) + columns.saturating_sub(1) as f32 * gap;
    let maximum_columns = child_count.max(1);
    let mut columns = previous_columns.clamp(1, maximum_columns);

    while columns < maximum_columns && available_width >= maximum_layout_width(columns) {
        columns += 1;
    }
    while columns > 1 && available_width < minimum_layout_width(columns) {
        columns -= 1;
    }

    let width = ((available_width - columns.saturating_sub(1) as f32 * gap) / columns as f32).max(minimum_width);
    (columns, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_grid_row_tracks_the_window_font_size() {
        assert_eq!(book_grid_row_height(16.0), app_contract::LARGE_BOOK_GRID_ROW_HEIGHT_PX as f32);
        assert_eq!(book_grid_row_height(20.0), app_contract::LARGE_BOOK_GRID_ROW_HEIGHT_PX as f32 + 16.0);
    }

    #[test]
    fn navigation_rail_grows_with_large_interface_text() {
        assert_eq!(nav_rail_width(12.0), 96.0);
        assert_eq!(nav_rail_width(16.0), 96.0);
        assert_eq!(nav_rail_width(24.0), 144.0);
    }

    #[test]
    fn hierarchy_columns_use_full_minimum_to_maximum_hysteresis() {
        let gap = HIERARCHY_CELL_MARGIN * 2.0;
        let two_column_minimum = HIERARCHY_CELL_MIN_WIDTH * 2.0 + gap;
        let one_column_maximum = HIERARCHY_CELL_MAX_WIDTH;
        let two_column_maximum = HIERARCHY_CELL_MAX_WIDTH * 2.0 + gap;

        assert_eq!(hierarchy_grid_layout(one_column_maximum - 1.0, 4, 1, 16.0).0, 1);
        assert_eq!(hierarchy_grid_layout(one_column_maximum, 4, 1, 16.0).0, 2);
        assert_eq!(hierarchy_grid_layout(two_column_minimum, 4, 2, 16.0).0, 2);
        assert_eq!(hierarchy_grid_layout(two_column_minimum - 1.0, 4, 2, 16.0).0, 1);
        assert_eq!(hierarchy_grid_layout(two_column_maximum - 1.0, 4, 2, 16.0).0, 2);
        assert_eq!(hierarchy_grid_layout(two_column_maximum, 4, 2, 16.0).0, 3);
        assert_eq!(hierarchy_grid_layout(two_column_minimum * 2.0, 1, 4, 16.0), (1, two_column_minimum * 2.0));
    }

    #[test]
    fn hierarchy_geometry_tracks_the_window_font_size() {
        // At the default the helpers reproduce the contract exactly, so nothing
        // moves for a reader who never touches the setting.
        assert_eq!(hierarchy_cell_height(16.0), HIERARCHY_CELL_HEIGHT);
        assert_eq!(hierarchy_cell_min_width(16.0), HIERARCHY_CELL_MIN_WIDTH);
        assert_eq!(hierarchy_cell_max_width(16.0), HIERARCHY_CELL_MAX_WIDTH);
        assert_eq!(author_row_height(16.0), AUTHOR_ROW_HEIGHT);

        // Larger interface text widens the cells, so a fixed width fits fewer
        // of them rather than truncating the labels inside them.
        assert!(hierarchy_cell_min_width(24.0) > HIERARCHY_CELL_MIN_WIDTH);
        assert!(hierarchy_cell_height(24.0) > HIERARCHY_CELL_HEIGHT);
        assert!(author_row_height(24.0) > AUTHOR_ROW_HEIGHT);
        let wide = 4.0 * (HIERARCHY_CELL_MIN_WIDTH + HIERARCHY_CELL_MARGIN * 2.0);
        assert!(hierarchy_grid_layout(wide, 8, 4, 24.0).0 < hierarchy_grid_layout(wide, 8, 4, 16.0).0);
    }
}

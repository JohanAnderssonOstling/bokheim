//! Browser-specific sizing policies for paginator groups.

use app_preferences::CoverText;
use gpui::{Pixels, Rems, px};
use ui_paginator::{PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};

/// The text block's contribution to a card's reserved height, under either
/// value of the "cover text" setting.
fn text_block_height(cover_text: CoverText) -> (f32, f32) {
    match cover_text {
        CoverText::Always => (ui_components::BOOK_CARD_TITLE_HEIGHT_REM, ui_components::BOOK_CARD_COVER_CONTENT_GAP),
        CoverText::CoverOnly => (ui_components::BOOK_CARD_NO_TEXT_HEIGHT_REM, ui_components::BOOK_CARD_NO_TEXT_FIXED_HEIGHT),
    }
}

/// The book grid on a browse page. Cover-only cards grow up to 25% before
/// expanding gaps.
pub(crate) fn book_card_policy(cover_text: CoverText) -> PaginatorGroupPolicy {
    let width = PaginatorWidthPolicy::pixels(px(ui_components::BOOK_CARD_WIDTH));
    let (extra_height, fixed_height) = text_block_height(cover_text);
    let sizing = PaginatorSizing::aspect_ratio(ui_components::BOOK_COVER_ASPECT_RATIO).with_rems(Rems(extra_height)).with_pixels(px(fixed_height)).with_padding(px(ui_components::BOOK_CARD_PADDING));
    let gap = px(ui_components::BOOK_GRID_BOOK_GAP);
    PaginatorGroupPolicy::new(width, sizing, gap).with_gap(gap).with_maximum_width(PaginatorWidthPolicy::pixels(px(ui_components::BOOK_CARD_MAXIMUM_WIDTH))).with_minimum_columns(ui_components::BOOK_GRID_MINIMUM_COLUMNS).flush_edges()
}

/// Cover-only cards in the graph sidebar. The preferred width is 200 px and
/// may stretch to 300 px; the paginator computes row height from the resolved
/// width and the cover aspect ratio. Phones keep two columns so the covers fit
/// in the bottom pane beside the graph.
#[cfg(all(feature = "graph", not(feature = "kobo")))]
pub(crate) fn graph_cover_policy() -> PaginatorGroupPolicy {
    let width = PaginatorWidthPolicy::pixels(px(200.0));
    let gap = px(ui_components::BOOK_GRID_BOOK_GAP);
    PaginatorGroupPolicy::new(width, PaginatorSizing::aspect_ratio(ui_components::BOOK_COVER_ASPECT_RATIO).with_padding(px(ui_components::BOOK_CARD_PADDING)), gap)
        .with_gap(gap)
        .with_maximum_width(PaginatorWidthPolicy::pixels(px(300.0)))
        .with_minimum_columns(if ui_components::has_native_back_button() { 2 } else { 1 })
        .with_edge_inset(px(12.0))
}

fn section_cover_cell_width() -> f32 {
    ui_components::SECTION_CARD_COVER_WIDTH + ui_components::BOOK_CARD_PADDING * 2.0
}

fn section_body_height(cover_text: CoverText, rem_size: Pixels) -> Pixels {
    browse_section_cover_policy(cover_text).sizing.height(px(section_cover_cell_width()), rem_size)
}

fn section_body_with_text(_width: Pixels, rem_size: Pixels) -> Pixels {
    section_body_height(CoverText::Always, rem_size)
}

fn section_body_cover_only(_width: Pixels, rem_size: Pixels) -> Pixels {
    section_body_height(CoverText::CoverOnly, rem_size)
}

/// Containers share the height of book previews, including optional cover text.
pub(crate) fn browse_section_card_policy(cover_text: CoverText) -> PaginatorGroupPolicy {
    let width = PaginatorWidthPolicy::pixels(px(ui_components::SECTION_CARD_WIDTH));
    // The body cells provide the full vertical padding below the heading.
    let heading_height = ui_components::SECTION_CARD_HEADING_HEIGHT_REM + ui_components::SECTION_CARD_HEADING_PADDING_REM * 2.0;
    let sizing = PaginatorSizing::rems(Rems(heading_height)).with_nested(match cover_text {
        CoverText::Always => section_body_with_text,
        CoverText::CoverOnly => section_body_cover_only,
    });
    // Keep the gap between tiles, while the first tile starts at the same
    // content inset as the breadcrumb and the first book cover.
    PaginatorGroupPolicy::new(width, sizing, px(ui_components::SPACE_MD)).with_gap(px(ui_components::SPACE_MD)).with_edge_inset(px(ui_components::CONTENT_INSET)).flush_edges().with_item_shadow()
}

/// Home's shelves: the same cards, each with the row to itself. Any width wider
/// than the window does that — see `SHELF_ROW_WIDTH_REM`.
pub(crate) fn browse_shelf_card_policy(cover_text: CoverText) -> PaginatorGroupPolicy {
    PaginatorGroupPolicy { width: PaginatorWidthPolicy::new(Rems(ui_components::SHELF_ROW_WIDTH_REM)), ..browse_section_card_policy(cover_text) }
}

/// Folder cards share a cover-height body, whether they contain subfolders
/// or books. The preferred width fits two covers side by side and can grow
/// to twice that width.
pub(crate) fn browse_folder_card_policy(cover_text: CoverText) -> PaginatorGroupPolicy {
    let width = PaginatorWidthPolicy::pixels(px(ui_components::FOLDER_CARD_WIDTH));
    PaginatorGroupPolicy { width, ..browse_section_card_policy(cover_text) }.with_maximum_width(PaginatorWidthPolicy::pixels(px(ui_components::FOLDER_CARD_WIDTH * 2.0)))
}

pub(crate) fn browse_folder_item_policy(cover_text: CoverText, container: bool) -> PaginatorGroupPolicy {
    if container {
        return browse_section_item_policy(cover_text, true);
    }
    browse_section_item_policy(cover_text, false).with_minimum_columns(2)
}

/// A book row inside a browse section card. Cover dimensions stay in pixels;
/// optional text adds its rem height and the cover-to-text gap.
pub(crate) fn browse_section_cover_policy(cover_text: CoverText) -> PaginatorGroupPolicy {
    let width = PaginatorWidthPolicy::pixels(px(section_cover_cell_width()));
    let gap = px(ui_components::BOOK_GRID_BOOK_GAP);
    let (text_height, text_gap) = text_block_height(cover_text);
    let sizing = PaginatorSizing::pixels(px(ui_components::SECTION_CARD_COVER_WIDTH / ui_components::BOOK_COVER_ASPECT_RATIO + text_gap)).with_rems(Rems(text_height)).with_padding(px(ui_components::BOOK_CARD_PADDING));
    PaginatorGroupPolicy::new(width, sizing, gap).with_gap(gap).with_maximum_width(width).pack_start()
}

/// Containers use one full-width column; shelves retain their cover cells.
pub(crate) fn browse_section_item_policy(cover_text: CoverText, container: bool) -> PaginatorGroupPolicy {
    if container {
        return PaginatorGroupPolicy::new(PaginatorWidthPolicy::new(Rems(ui_components::SHELF_ROW_WIDTH_REM)), browse_section_cover_policy(cover_text).sizing, Pixels::ZERO)
            .with_minimum_columns(ui_components::SECTION_CARD_SUBJECT_COLUMNS)
            .with_gap(Pixels::ZERO)
            .flush_edges();
    }
    let width = PaginatorWidthPolicy::pixels(px(section_cover_cell_width()));
    PaginatorGroupPolicy::new(width, browse_section_cover_policy(cover_text).sizing, Pixels::ZERO).with_gap(Pixels::ZERO).with_maximum_width(width).pack_start()
}

#[cfg(test)]
mod tests {
    use super::{browse_folder_card_policy, browse_folder_item_policy, browse_section_card_policy, browse_section_item_policy, browse_shelf_card_policy};
    use app_preferences::CoverText;
    use gpui::px;
    use ui_components as components;
    use ui_paginator::group_geometry;

    #[test]
    fn folder_cards_fit_two_covers_and_use_the_same_height_for_subfolders() {
        for rem in [12.0, 16.0, 24.0, 32.0] {
            for setting in [CoverText::CoverOnly, CoverText::Always] {
                let card = browse_folder_card_policy(setting);
                assert_eq!(card.width.width(px(rem)), px(348.0));
                for width in [160.0, 200.0, 240.0, 348.0] {
                    let body_width = px(width - (components::SPACE_MD - components::BOOK_CARD_PADDING) * 2.0);
                    let books = browse_folder_item_policy(setting, false);
                    let folders = browse_folder_item_policy(setting, true);
                    let covers = group_geometry(books, 0, body_width, px(rem));
                    let children = group_geometry(folders, 0, body_width, px(rem));
                    assert_eq!(covers.columns, 2);
                    assert_eq!(children.columns, 1);
                    assert!(covers.child_width * 2.0 <= body_width);
                    assert_eq!(books.sizing.height(covers.child_width, px(rem)), folders.sizing.height(children.child_width, px(rem)));
                }
            }
        }
    }

    #[test]
    fn shelves_size_to_covers_and_optional_titles() {
        let cover_height = components::SECTION_CARD_COVER_WIDTH / components::BOOK_COVER_ASPECT_RATIO + components::BOOK_CARD_PADDING * 2.0;
        let heading_height = components::SECTION_CARD_HEADING_HEIGHT_REM + components::SECTION_CARD_HEADING_PADDING_REM * 2.0;
        for rem in [12.0, 16.0, 24.0, 32.0] {
            for setting in [CoverText::CoverOnly, CoverText::Always] {
                let text_height = if setting == CoverText::Always { components::BOOK_CARD_TITLE_HEIGHT_REM * rem + components::BOOK_CARD_COVER_CONTENT_GAP } else { 0.0 };
                let expected_body = px(cover_height + text_height);
                let shelf = browse_shelf_card_policy(setting);
                let items = browse_section_item_policy(setting, false);
                for width in [390.0, 780.0] {
                    assert_eq!(items.sizing.height(px(width), px(rem)), expected_body);
                    assert_eq!(shelf.sizing.height(px(width), px(rem)), expected_body + px(heading_height * rem));
                }
            }
        }
    }

    #[test]
    fn containers_keep_one_full_width_column_across_viewports_and_font_sizes() {
        for rem in [12.0, 16.0, 24.0, 32.0] {
            for setting in [CoverText::CoverOnly, CoverText::Always] {
                let card = browse_section_card_policy(setting);
                let items = browse_section_item_policy(setting, true);
                assert_eq!(card.width.width(px(rem)), px(components::SECTION_CARD_WIDTH));
                for width in [240.0, 320.0, 390.0, 600.0, 1200.0] {
                    let columns = group_geometry(items, 0, px(width), px(rem));
                    assert_eq!(columns.columns, 1);
                    assert_eq!(columns.child_width, px(width));
                }
            }
        }
    }

    #[test]
    fn containers_match_folder_and_book_preview_heights_across_platforms() {
        for rem in [12.0, 16.0, 24.0, 32.0] {
            for setting in [CoverText::CoverOnly, CoverText::Always] {
                let body = browse_section_item_policy(setting, false).sizing.height(px(166.0), px(rem));
                for width in [240.0, 390.0, 780.0] {
                    assert_eq!(browse_section_item_policy(setting, true).sizing.height(px(width), px(rem)), body);
                    let height = browse_section_card_policy(setting).sizing.height(px(width), px(rem));
                    assert_eq!(height, browse_folder_card_policy(setting).sizing.height(px(width), px(rem)));
                    assert_eq!(height, browse_shelf_card_policy(setting).sizing.height(px(width), px(rem)));
                }
            }
        }
    }
}

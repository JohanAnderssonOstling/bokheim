//! Conversion from adapter results to paginator groups.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Entity, SharedString, WeakEntity, div, px, rems};
use library_model::BrowseRow;
use ui_components as components;

use super::adapter::BrowseAdapter;
use super::page::BrowsePage;
use crate::library::widgets::{PaginatorChild, PaginatorGapPolicy, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};

const BROWSE_ITEM_PREFERRED_WIDTH_REM: f32 = 10.0;
const BROWSE_ITEM_MINIMUM_WIDTH_REM: f32 = BROWSE_ITEM_PREFERRED_WIDTH_REM * 0.8;
const BROWSE_ITEM_MAXIMUM_WIDTH_REM: f32 = BROWSE_ITEM_MINIMUM_WIDTH_REM * 2.0;
const CHIP_ITEM_MINIMUM_WIDTH_REM: f32 = 10.0;
const CHIP_ITEM_PREFERRED_WIDTH_REM: f32 = 14.0;
const CHIP_ITEM_MAXIMUM_WIDTH_REM: f32 = 20.0;
const BOOK_CARD_ASPECT_RATIO: f32 = components::BOOK_COVER_ASPECT_RATIO;
const BOOK_CARD_EXTRA_HEIGHT_REM: f32 = components::BOOK_CARD_TITLE_HEIGHT_REM + components::BOOK_CARD_AUTHOR_HEIGHT_REM;

pub(super) fn paginator_groups<A: BrowseAdapter>(page: WeakEntity<BrowsePage<A>>, chips: Rc<Vec<BrowseRow>>, books: Rc<Vec<Entity<A::Book>>>, wait_for_book_readiness: bool) -> Vec<PaginatorGroup> {
    let mut chip_children = Vec::with_capacity(chips.len());
    let mut book_children = Vec::with_capacity(books.len());

    for chip in chips.iter() {
        let chip = chip.clone();
        let page = page.clone();
        chip_children.push(PaginatorChild::new(move |window, cx| {
            let location = chip.id.clone();
            let open_page = page.clone();
            let height = rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM).to_pixels(window.rem_size()) + px(components::COMPACT_HIERARCHY_ITEM_HEIGHT_OFFSET);
            let item = components::compact_hierarchy_item(format!("browse-chip-{location}"), chip.name.clone(), chip.path.clone().map(SharedString::from), chip.book_count, components::browser_theme(cx)).on_click(move |_, window, cx| {
                let _ = open_page.update(cx, |page, cx| page.open_child(location.clone(), window, cx));
            });
            div().w_full().h(height).child(item).into_any_element()
        }));
    }

    for book in books.iter() {
        let prepared_book = book.clone();
        let ready_book = book.clone();
        let rendered_book = book.clone();
        let activated_book = book.clone();
        let child = PaginatorChild::new(move |_, _| div().w_full().min_w_0().child(rendered_book.clone()).into_any_element())
            .with_activate(move |window, cx| A::activate_book(&activated_book, window, cx))
            .with_prepare(move |cx| A::prepare_book(&prepared_book, cx));
        let child = if wait_for_book_readiness { child.with_ready(move |cx| A::book_ready(&ready_book, cx)) } else { child };
        book_children.push(if wait_for_book_readiness { if let Some(timeout) = A::initial_cover_timeout() { child.with_ready_timeout(timeout) } else { child } } else { child });
    }

    let chip_width = PaginatorWidthPolicy::new(rems(CHIP_ITEM_MINIMUM_WIDTH_REM), rems(CHIP_ITEM_PREFERRED_WIDTH_REM), rems(CHIP_ITEM_MAXIMUM_WIDTH_REM));
    let book_width = PaginatorWidthPolicy::new(rems(BROWSE_ITEM_MINIMUM_WIDTH_REM), rems(BROWSE_ITEM_PREFERRED_WIDTH_REM), rems(BROWSE_ITEM_MAXIMUM_WIDTH_REM));
    let mut groups = Vec::with_capacity(2);
    if !chip_children.is_empty() {
        groups.push(PaginatorGroup::new(
            PaginatorGroupPolicy::new(
                chip_width,
                PaginatorSizing::rems(rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM)).with_pixels(px(components::COMPACT_HIERARCHY_ITEM_HEIGHT_OFFSET)),
                PaginatorGapPolicy::new(px(components::SPACE_XXS), px(components::SPACE_XS)).with_maximum(px(components::SPACE_SM)),
                PaginatorGapPolicy::new(px(components::SPACE_XXS), px(components::SPACE_XS)),
            ),
            chip_children,
        ));
    }
    if !book_children.is_empty() {
        groups.push(PaginatorGroup::new(
            PaginatorGroupPolicy::new(
                book_width,
                PaginatorSizing::aspect_ratio(BOOK_CARD_ASPECT_RATIO).with_rems(rems(BOOK_CARD_EXTRA_HEIGHT_REM)).with_pixels(px(components::BOOK_CARD_FIXED_HEIGHT)),
                PaginatorGapPolicy::new(px(components::BOOK_GRID_GAP), px(components::BOOK_GRID_BOOK_GAP)).with_maximum(px(components::BOOK_GRID_MAX_GAP)),
                PaginatorGapPolicy::new(px(components::BOOK_GRID_GAP), px(components::BOOK_GRID_BOOK_GAP)),
            ),
            book_children,
        ));
    }
    groups
}

//! Conversion from a listing to paginator groups.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, Bounds, Div, Entity, Pixels, Point, SharedString, Stateful, Window, div, px, rems};
use gpui_component::menu::OpenContextMenu;
use library_model::{BrowseHeadingState, BrowsePageState, BrowseRow};
use sync_common::DirId;
use ui_components as components;

use super::cards::{CardEvent, CardEvents};
use super::drag_drop::{BookDrag, drag_payload, folder_drag_source, folder_drop_target};
use super::listing::{BrowseItem, BrowseItems, BrowseKind, book_id, chip_id, is_author_heading, resolve_items, shown_count, shows_download_mark};
use super::page::BrowseSplitSide;
use super::section_card::SectionCard;
use crate::library::widgets::{
    BookCardInteraction, BookCardPresentation, BookCardView, PaginatorChild, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorState, PaginatorWidthPolicy, book_card_policy, browse_section_card_policy,
    browse_shelf_card_policy,
};

/// A child opens, splits, shows its resolved menu, and carries folder placements.
pub(super) fn child_frame(
    element: impl IntoElement, row: &BrowseRow, events: CardEvents, library_id: uuid::Uuid, source: Option<DirId>, items: Vec<BrowseItem>, in_selection: bool, chip_icon: components::NavigationIcon, theme: components::BrowserTheme, cx: &App,
) -> Stateful<Div> {
    let location = row.id;
    let split_location = location;
    let open_events = events.clone();
    let menu_events = events.clone();
    let menu_items = items.clone();
    let long_press_events = events.clone();
    let long_press_items = items.clone();
    let mut frame = div()
        .id(format!("browse-chip-frame-{}", row.id))
        .w_full()
        .h_full()
        .child(element)
        .on_click(move |event, window, cx| {
            let event = if event.modifiers().control && crate::split_panes_available(window) { CardEvent::SplitChild(location, BrowseSplitSide::Left) } else { CardEvent::OpenChild(location) };
            open_events(event, window, cx);
            cx.stop_propagation();
        })
        .on_aux_click(move |event, window, cx| {
            if !event.is_secondary() {
                return;
            }
            let split = event.modifiers().control && crate::split_panes_available(window);
            if split {
                menu_events(CardEvent::SplitChild(split_location, BrowseSplitSide::Right), window, cx);
            } else if in_selection {
                return;
            } else {
                menu_events(CardEvent::ItemsMenu(menu_items.clone(), event.position()), window, cx);
            }
            cx.stop_propagation();
        })
        .on_long_press(move |event, window, cx| {
            if !in_selection {
                long_press_events(CardEvent::ItemsMenu(long_press_items.clone(), event.position), window, cx);
                cx.stop_propagation();
            }
        });
    if let Some(source) = source {
        if let library_model::BrowseLocation::Folder(destination) = row.id {
            frame = folder_drop_target(frame, library_id, destination, row.name.clone().into(), true, events, theme);
        }
        if let Some((targets, preview)) = drag_payload(&items, Some(source), library_id, chip_icon, cx) {
            frame = folder_drag_source(frame, targets, preview);
        }
    }
    frame
}

pub(super) fn download_mark(row: &BrowseRow, events: CardEvents, theme: components::BrowserTheme) -> Option<gpui::AnyElement> {
    shows_download_mark(row).then(|| {
        let location = row.id;
        div()
            .id(format!("chip-download-{location}"))
            .debug_selector(move || format!("chip-download-{location}"))
            .flex_none()
            .cursor_pointer()
            .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Download remaining books to this device").build(window, cx))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click({
                let location = row.id;
                move |_, window, cx| {
                    cx.stop_propagation();
                    events(CardEvent::Download(location), window, cx);
                }
            })
            .child(components::download_glyph().text_color(theme.text))
            .into_any_element()
    })
}

/// Independently focusable contents use the same activation and menu as the pointer.
pub(super) fn keyboard_item(frame: Stateful<Div>, activate: impl Fn(&mut Window, &mut App) + 'static, menu: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static, cx: &App) -> Stateful<Div> {
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let menu_bounds = bounds.clone();
    let theme = components::browser_theme(cx);
    frame
        .relative()
        .focusable()
        .tab_index(0)
        .key_context("BrowseItem")
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(move |style| style.border_color(theme.text))
        .child(components::measured_bounds(bounds))
        .on_action(move |_: &OpenContextMenu, window, cx| menu(menu_bounds.get().bottom_left(), window, cx))
        .on_key_down(move |event, window, cx| {
            if event.keystroke.modifiers != gpui::Modifiers::default() {
                return;
            }
            match event.keystroke.key.as_str() {
                "enter" => activate(window, cx),
                "left" | "up" => window.focus_prev(cx),
                "right" | "down" => window.focus_next(cx),
                _ => return,
            }
            cx.stop_propagation();
        })
}

/// The three book rails share identity, activation and cover preparation.
pub(super) fn book_child(book: Entity<BookCardView>, presentation: BookCardPresentation, interaction: impl Fn(&PaginatorState, &SharedString, &App) -> BookCardInteraction + 'static, drag: Option<BookDrag>, cx: &mut App) -> PaginatorChild {
    let item_id = book_id(book.read(cx).key(), book.read(cx).source_directory());
    let frame_id = item_id.clone();
    let rendered = book.clone();
    let activated = book.clone();
    let view = BookCardView::presentation(book.clone(), presentation, cx);
    PaginatorChild::new(move |state, window, cx| {
        let card = BookCardView::present(view.clone(), interaction(state, &frame_id, cx), window, cx);
        let mut frame = div().id(frame_id.clone()).w_full().h_full().min_w_0().child(card);
        if presentation != BookCardPresentation::Grid {
            let activate = rendered.clone();
            let menu = rendered.clone();
            frame = keyboard_item(frame, move |window, cx| BookCardView::activate(&activate, window, cx), move |position, window, cx| BookCardView::open_items_menu(std::slice::from_ref(&menu), position, window, cx), cx);
        }
        if presentation == BookCardPresentation::Section {
            let theme = components::browser_theme(cx);
            frame = frame.hover(move |style| style.bg(theme.hover)).on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation());
        }
        if let Some((targets, preview)) = drag.as_ref().and_then(|drag| drag(state, &frame_id, cx)) {
            frame = folder_drag_source(frame, targets, preview);
        }
        frame.into_any_element()
    })
    .with_id(item_id)
    .with_activate(move |window, cx| BookCardView::activate(&activated, window, cx))
    .with_prepare(move |window, cx| BookCardView::prefetch_for_window(&book, window, cx))
}

pub(super) fn paginator_groups(
    events: CardEvents,
    library_id: uuid::Uuid,
    chips: Rc<Vec<BrowseRow>>,
    kind: BrowseKind,
    books: Rc<Vec<Entity<BookCardView>>>,
    // The folder being browsed. Present only for listings whose children are
    // real directories, which are the only ones that can be dragged.
    source: Option<DirId>,
    section_cards: &HashMap<library_model::BrowseLocation, Entity<SectionCard>>,
    items: BrowseItems,
    page: Option<&BrowsePageState>,
    book_indices: &BTreeMap<usize, Entity<BookCardView>>,
    heading_rows: &BTreeMap<usize, BrowseRow>,
    cx: &mut App,
) -> Vec<PaginatorGroup> {
    let cover_text = crate::stores::current_cover_text(cx);
    let chip_icon = kind.child_icon();
    // Home's shelves are fixed rows rather than places to go: their cards span
    // the page, and their headings do not open anything.
    let shelves = kind == BrowseKind::Home;
    // A listing's children are section cards or plain chips. Both appear only at
    // the catalog's root, where the feeds are chips beside the subjects' cards.
    let mut chip_children = Vec::new();
    let mut card_children = Vec::new();
    let section_ids: HashSet<_> = page.into_iter().flat_map(|page| page.section_ids.iter().copied()).collect();

    let headings = page.and_then(|page| page.headings.as_ref());
    let rows = outer_heading_rows(&chips, headings, heading_rows);
    for chip in rows {
        let Some(chip) = chip else {
            let placeholder = PaginatorChild::new(|_, _, cx| div().size_full().bg(components::browser_theme(cx).hover).into_any_element());
            if headings.is_some_and(|headings| headings.cards) {
                card_children.push(placeholder);
            } else {
                chip_children.push(placeholder.with_intrinsic_width(components::HierarchyItemWidth { icon: chip_icon, label: "Loading authors…".into(), count: None, show_download: false }));
            }
            continue;
        };
        let chip_icon = if matches!(chip.id, library_model::BrowseLocation::Store(_)) { components::NavigationIcon::Storefront } else { chip_icon };
        let width_label = SharedString::from(chip.name.clone());
        let child_count = shown_count(&chip);
        let show_download = shows_download_mark(&chip);
        let item_id = chip_id(chip.id);
        let rendered_id = item_id.clone();
        let chip = chip.clone();
        let events = events.clone();
        let drag_items = items.clone();
        let section_card = section_cards.get(&chip.id).cloned();
        let is_card = section_card.is_some() || section_ids.contains(&chip.id);
        // Chips previously carried a click handler only, so the paginator had
        // nothing to invoke for them. Enter and a click now share one path.
        let activate_events = events.clone();
        let activate_location = chip.id;
        let child = PaginatorChild::new(move |state, _, cx| {
            let focused = state.is_focused(&rendered_id);
            let selected = state.is_selected(&rendered_id);
            let theme = components::browser_theme(cx);
            let item = match &section_card {
                None if is_card => div().size_full().p_4().bg(theme.hover).child(chip.name.clone()).into_any_element(),
                None => components::compact_hierarchy_item(
                    format!("browse-chip-{}", chip.id),
                    chip_icon,
                    chip.name.clone(),
                    chip.path.clone().map(SharedString::from),
                    child_count,
                    download_mark(&chip, events.clone(), theme),
                    selected,
                    focused,
                    state.marquee,
                    theme,
                )
                .debug_selector(|| format!("browse-chip-{}", chip.id))
                .into_any_element(),
                Some(card) => SectionCard::present(card.clone(), focused, selected, state.marquee).into_any_element(),
            };
            if shelves {
                return div().relative().size_full().child(item).into_any_element();
            }
            let selected = resolve_items(&state.target_ids(&rendered_id), &drag_items);
            let frame = child_frame(item, &chip, events.clone(), library_id, source, selected, state.is_selected(&rendered_id), chip_icon, theme, cx);
            let frame = if is_card { frame.relative() } else { frame.h(rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM)) };
            frame.into_any_element()
        })
        .with_id(item_id)
        .with_intrinsic_width(components::HierarchyItemWidth { icon: chip_icon, label: width_label, count: child_count, show_download });
        let child = if shelves { child } else { child.with_activate(move |window, cx| activate_events(CardEvent::OpenChild(activate_location), window, cx)) };
        if is_card { card_children.push(child) } else { chip_children.push(child) }
    }

    let book_children = direct_book_children(books, page, book_indices, BookCardPresentation::Grid, Some((items, source, library_id, chip_icon)), cx);

    let chip_width = PaginatorWidthPolicy::pixels(px(1.0));
    let mut groups = Vec::with_capacity(3);
    if !chip_children.is_empty() {
        let policy = PaginatorGroupPolicy::new(chip_width, PaginatorSizing::rems(rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM)), px(components::COMPACT_HIERARCHY_ITEM_ROW_GAP))
            .with_gap(px(components::COMPACT_HIERARCHY_ITEM_ROW_GAP))
            .intrinsic()
            .flush_edges()
            .with_edge_inset(px(components::BOOK_CARD_PADDING));
        groups.push(PaginatorGroup::new(policy, chip_children));
    }
    if !card_children.is_empty() {
        groups.push(PaginatorGroup::new(if shelves { browse_shelf_card_policy(cover_text) } else { browse_section_card_policy(cover_text) }, card_children));
    }
    if !book_children.is_empty() {
        groups.push(PaginatorGroup::new(book_card_policy(cover_text), book_children));
    }
    groups
}

/// Reserve unloaded author positions without allocating names or card views.
fn outer_heading_rows(chips: &[BrowseRow], headings: Option<&BrowseHeadingState>, loaded: &BTreeMap<usize, BrowseRow>) -> Vec<Option<BrowseRow>> {
    if let Some(headings) = headings {
        chips.iter().filter(|chip| !is_author_heading(chip.id)).cloned().map(Some).chain((0..headings.total).map(|index| loaded.get(&index).cloned())).collect()
    } else {
        chips.iter().cloned().map(Some).collect()
    }
}

#[cfg(test)]
mod heading_tests {
    use super::*;
    use library_model::BrowseLocation;

    #[test]
    fn sparse_author_headings_preserve_distant_positions_and_leave_feed_chips_first() {
        let row = |id| BrowseRow { id: BrowseLocation::CatalogAuthor(Some(uuid::Uuid::from_u128(id))), name: format!("Writer {id}"), path: None, book_count: 2, downloaded_book_count: 0 };
        let feed = BrowseRow { id: BrowseLocation::Store(uuid::Uuid::nil()), name: "Store".into(), path: None, book_count: 0, downloaded_book_count: 0 };
        let first = row(1);
        let distant = row(28);
        let loaded = BTreeMap::from([(0, first.clone()), (27, distant.clone())]);
        for cards in [false, true] {
            let headings = BrowseHeadingState { total: 30, revision: "one".into(), cards };
            let rows = outer_heading_rows(&[first.clone(), distant.clone(), feed.clone()], Some(&headings), &loaded);
            assert_eq!(rows.len(), 31);
            assert_eq!(rows[0], Some(feed.clone()));
            assert_eq!(rows[1], Some(first.clone()));
            assert!(rows[2..28].iter().all(Option::is_none));
            assert_eq!(rows[28], Some(distant.clone()));
            assert!(rows[29..].iter().all(Option::is_none));
        }
        assert_eq!(outer_heading_rows(&[feed.clone()], None, &loaded), vec![Some(feed)]);
    }
}

/// Unloaded slots retain their positions; loaded books retain their identity.
pub(super) fn direct_book_children(
    books: Rc<Vec<Entity<BookCardView>>>, page: Option<&BrowsePageState>, indices: &BTreeMap<usize, Entity<BookCardView>>, presentation: BookCardPresentation,
    drag_context: Option<(BrowseItems, Option<DirId>, uuid::Uuid, components::NavigationIcon)>, cx: &mut App,
) -> Vec<PaginatorChild> {
    let total = page.map_or(books.len(), |page| page.book_total);
    let placeholder = PaginatorChild::new(|_, _, _| div().size_full().into_any_element());
    (0..total)
        .map(|index| {
            let book = if page.is_some() { indices.get(&index) } else { books.get(index) };
            let Some(book) = book else { return placeholder.clone() };
            let drag = drag_context.as_ref().map(|(items, source, library_id, icon)| {
                let (items, source, library_id, icon) = (items.clone(), *source, *library_id, *icon);
                Rc::new(move |state: &PaginatorState, id: &SharedString, cx: &App| drag_payload(&resolve_items(&state.target_ids(id), &items), source, library_id, icon, cx)) as BookDrag
            });
            book_child(book.clone(), presentation, move |state, id, _| BookCardInteraction { focused: state.is_focused(id), selected: state.is_selected(id), marquee: state.marquee }, drag, cx)
        })
        .collect()
}

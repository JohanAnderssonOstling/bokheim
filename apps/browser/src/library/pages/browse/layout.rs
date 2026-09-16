//! Conversion from adapter results to paginator groups.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, Entity, Pixels, Point, SharedString, WeakEntity, div, px, rems};
use library_model::BrowseRow;
use sync_common::DirId;
use ui_components as components;

use super::adapter::BrowseAdapter;
use super::folder_actions::FolderTargets;
use super::page::{BrowsePage, BrowseSelectionState, BrowseSplitSide};
use crate::library::widgets::{Paginator, PaginatorChild, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy, book_card_policy};

/// What dragging the child at `index` carries: the whole selection when that
/// child is part of one, and otherwise just that child.
///
/// Built from the shared lists rather than read back off the page — these
/// closures run inside the paginator's own render, where the page is already
/// borrowed.
fn drag_targets<A: BrowseAdapter>(index: usize, chips: &[BrowseRow], chip_groups: &[library_model::BrowseChipGroup], books: &[Entity<A::Book>], selected: &BTreeSet<usize>, source: DirId, cx: &App) -> Option<FolderTargets> {
    let indices: Vec<usize> = if selected.contains(&index) { selected.iter().copied().collect() } else { vec![index] };
    let mut targets = FolderTargets { folders: Vec::new(), folder_parents: Default::default(), books: Vec::new(), source };
    for index in indices {
        match chips.get(index) {
            Some(chip) => {
                if let Ok(id) = DirId::parse_str(&chip.id) {
                    targets.folders.push(id);
                    if let Some(parent) = chip_groups.iter().find(|group| (group.start..group.end).contains(&index)).and_then(|group| DirId::parse_str(&group.parent.id).ok()) {
                        targets.folder_parents.insert(id, parent);
                    }
                }
            }
            None => {
                let Some(book) = books.get(index - chips.len()) else { continue };
                if let Some(hash) = A::book_content_hash(book, cx) {
                    targets.books.push(hash);
                }
            }
        }
    }
    (!targets.is_empty()).then_some(targets)
}

/// The preview that follows the pointer during a drag.
///
/// A single item keeps its shape so it is immediately clear what is being
/// moved. Rectangular and multi-item drags use a compact icon grid so the
/// whole selection remains visible without obscuring the pointer.
struct DragPreview<B: gpui::Render + 'static> {
    items: Vec<DragPreviewItem<B>>,
    label: SharedString,
    /// Where inside the dragged element the drag began. GPUI paints this view
    /// at `pointer - cursor_offset`; the small nudge keeps the ghost clear of
    /// the pointer while it is being carried.
    grab: Point<Pixels>,
    detailed_books: bool,
}

enum DragPreviewItem<B: gpui::Render + 'static> {
    Book(Entity<B>),
    Folder { icon: components::NavigationIcon, label: SharedString, path: Option<SharedString> },
}

impl<B: gpui::Render + 'static> Clone for DragPreviewItem<B> {
    fn clone(&self) -> Self {
        match self {
            Self::Book(book) => Self::Book(book.clone()),
            Self::Folder { icon, label, path } => Self::Folder { icon: *icon, label: label.clone(), path: path.clone() },
        }
    }
}

impl<B: gpui::Render + 'static> gpui::Render for DragPreview<B> {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let preview = if self.items.is_empty() {
            components::drag_preview(self.label.clone(), theme).into_any_element()
        } else if self.items.len() > 1 {
            // A rectangular/multi-selection should read as a group without
            // turning the drag ghost into a stack of full-size cards.
            div()
                .w(px(76.0))
                .flex()
                .flex_wrap()
                .gap(px(components::SPACE_XXS))
                .children(self.items.iter().map(|item| {
                    let icon = match item {
                        DragPreviewItem::Book(_) => components::NavigationIcon::BookOpen,
                        DragPreviewItem::Folder { icon, .. } => *icon,
                    };
                    div().w(px(24.0)).h(px(24.0)).flex().items_center().justify_center().rounded(px(components::RADIUS_SM)).bg(theme.accent_wash).child(components::navigation_icon(icon)).into_any_element()
                }))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_start()
                .gap(px(components::SPACE_XS))
                .children(self.items.iter().map(|item| {
                    match item {
                        DragPreviewItem::Book(book) => div()
                            .w(if self.detailed_books { px(components::BOOK_CARD_DETAIL_FIXED_WIDTH) + rems(components::BOOK_CARD_DETAIL_TEXT_WIDTH_REM).to_pixels(window.rem_size()) } else { px(components::BOOK_CARD_WIDTH) })
                            .flex_none()
                            .child(book.clone())
                            .into_any_element(),
                        DragPreviewItem::Folder { icon, label, path } => components::compact_hierarchy_item(format!("drag-preview-folder-{label}"), *icon, label.clone(), path.clone(), None, None, false, false, theme).into_any_element(),
                    }
                }))
                .into_any_element()
        };
        // Keep the preview visible against the page while making it clear that
        // it is a transient copy rather than the original card.
        div().opacity(0.58).pl(self.grab.x + px(DRAG_PREVIEW_NUDGE)).pt(self.grab.y + px(DRAG_PREVIEW_NUDGE)).child(preview)
    }
}

fn drag_preview_items<A: BrowseAdapter>(index: usize, chips: &[BrowseRow], books: &[Entity<A::Book>], selected: &BTreeSet<usize>, chip_icon: components::NavigationIcon, cx: &App) -> Vec<DragPreviewItem<A::Book>> {
    let indices: Vec<usize> = if selected.contains(&index) { selected.iter().copied().collect() } else { vec![index] };
    indices
        .into_iter()
        .filter_map(|index| match chips.get(index) {
            Some(chip) => DirId::parse_str(&chip.id).ok().map(|_| DragPreviewItem::Folder { icon: chip_icon, label: SharedString::from(chip.name.clone()), path: chip.path.clone().map(SharedString::from) }),
            None => {
                let book = books.get(index.checked_sub(chips.len())?)?;
                A::book_content_hash(book, cx).map(|_| DragPreviewItem::Book(book.clone()))
            }
        })
        .collect()
}

/// Distance from the pointer to the pill's corner.
const DRAG_PREVIEW_NUDGE: f32 = 12.0;

fn drag_label(targets: &FolderTargets) -> String {
    match (targets.folders.len(), targets.books.len()) {
        (1, 0) => "1 folder".to_owned(),
        (0, 1) => "1 book".to_owned(),
        (folders, 0) => format!("{folders} folders"),
        (0, books) => format!("{books} books"),
        (folders, books) => format!("{folders} folders, {books} books"),
    }
}

/// `cursor` carries the paginator's keyboard cursor to the chips, which are
/// closures rather than entities and so cannot be pushed into the way book
/// cards are. A shared cell rather than a read of either entity: these closures
/// run inside the paginator's own render, where reading it back would be a
/// borrow of something already borrowed.
pub(super) fn paginator_groups<A: BrowseAdapter>(
    page: WeakEntity<BrowsePage<A>>,
    chips: Rc<Vec<BrowseRow>>,
    chip_groups: &[library_model::BrowseChipGroup],
    chip_icon: components::NavigationIcon,
    books: Rc<Vec<Entity<A::Book>>>,
    detailed_books: bool,
    cursor: Rc<Cell<Option<usize>>>,
    selected: Entity<BrowseSelectionState>,
    // The chip a drag is currently over, which is drawn as the drop it would be.
    drop_target: Rc<Cell<Option<usize>>>,
    // Whether a rubber band is sweeping, which suppresses every child's hover.
    marquee: Rc<Cell<bool>>,
    // The folder being browsed. Present only for listings whose children are
    // real directories, which are the only ones that can be dragged.
    source: Option<DirId>,
    paginator: WeakEntity<Paginator>,
) -> Vec<PaginatorGroup> {
    let mut chip_children = Vec::with_capacity(chips.len());
    let mut book_children = Vec::with_capacity(books.len());

    for (chip_index, chip) in chips.iter().enumerate() {
        let width_label = SharedString::from(chip.name.clone());
        let width_count = A::child_count(chip);
        let show_download = A::can_download_child(chip);
        let chip = chip.clone();
        let page = page.clone();
        let cursor = cursor.clone();
        let selected = selected.clone();
        let drag_chips = chips.clone();
        let drag_groups = chip_groups.to_vec();
        let drag_books = books.clone();
        let drop_target = drop_target.clone();
        let marquee = marquee.clone();
        // Chips previously carried a click handler only, so the paginator had
        // nothing to invoke for them. Enter and a click now share one path.
        let activate_page = page.clone();
        let activate_location = chip.id.clone();
        chip_children.push(
            PaginatorChild::new(move |window, cx| {
                let location = chip.id.clone();
                let split_location = location.clone();
                let open_page = page.clone();
                let split_page = page.clone();
                let menu_page = page.clone();
                let menu_selected = selected.clone();
                let height = rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM).to_pixels(window.rem_size()) + px(components::COMPACT_HIERARCHY_ITEM_HEIGHT_OFFSET);
                // Chips are declared first, so a chip's flat child index is its
                // position in this list.
                //
                // A chip the band picked up is drawn like the cursor's chip: the
                // two never coexist, because moving the cursor drops the selection.
                let selected_now = selected.read(cx).indices.clone();
                let focused = cursor.get() == Some(chip_index) || selected_now.contains(&chip_index);
                // A folder about to receive a drop answers with the wash a hover
                // uses — only a folder, since only a folder can take one. It cannot
                // be a `drag_over` style on this frame: the chip's own button paints
                // an opaque background over it.
                let drop_hovered = source.is_some() && drop_target.get() == Some(chip_index) && DirId::parse_str(&chip.id).is_ok();
                let count = A::child_count(&chip);
                let theme = components::browser_theme(cx);
                let download = show_download.then(|| {
                    let target = page.clone();
                    let location = chip.id.clone();
                    div()
                        .id(format!("chip-download-{location}"))
                        .debug_selector(|| format!("chip-download-{}", chip.id))
                        .flex_none()
                        .cursor_pointer()
                        .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Download remaining books to this device").build(window, cx))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = target.update(cx, |page, cx| page.download_child(location.clone(), cx));
                        })
                        .children(components::book_download_indicator(false, false, theme))
                        .into_any_element()
                });
                let item = components::compact_hierarchy_item(format!("browse-chip-{location}"), chip_icon, chip.name.clone(), chip.path.clone().map(SharedString::from), count, download, focused, marquee.get(), theme)
                    .debug_selector(|| format!("browse-chip-{}", chip.id))
                    .when(drop_hovered && !focused, |item| item.bg(theme.hover))
                    .on_click(move |event, window, cx| {
                        if event.modifiers().control && split_page.upgrade().is_some_and(|page| page.read(cx).can_split(window)) {
                            let _ = split_page.update(cx, |page, cx| page.request_split_child(location.clone(), BrowseSplitSide::Left, cx));
                            cx.stop_propagation();
                        } else {
                            let _ = open_page.update(cx, |page, cx| page.open_child(location.clone(), window, cx));
                        }
                    });
                // A chip inside a selection leaves its secondary click to the
                // paginator, which opens one menu over everything selected. On its
                // own, a chip opens its own menu here: the chip's element is a
                // button, so the click is taken on this wrapper.
                let mut frame = div().id(SharedString::from(format!("browse-chip-frame-{}", chip.id))).w_full().h(height).child(item).on_aux_click(move |event, window, cx| {
                    if event.modifiers().control && event.is_secondary() && menu_page.upgrade().is_some_and(|page| page.read(cx).can_split(window)) {
                        let _ = menu_page.update(cx, |page, cx| page.request_split_child(split_location.clone(), BrowseSplitSide::Right, cx));
                        cx.stop_propagation();
                        return;
                    }
                    if !event.is_secondary() || menu_selected.read(cx).indices.contains(&chip_index) {
                        return;
                    }
                    let _ = menu_page.update(cx, |page, cx| page.open_items_menu(chip_index, event.position(), window, cx));
                    cx.stop_propagation();
                });
                // Folders are both ends of a drag: a chip can be picked up, and a
                // chip is where anything picked up can be put down.
                if let Some(source) = source {
                    if let Ok(destination) = DirId::parse_str(&chip.id) {
                        let drop_page = page.clone();
                        let import_page = page.clone();
                        frame = frame
                            .on_drop::<gpui::ExternalPaths>(move |paths, _, cx| {
                                let paths = paths.paths().to_vec();
                                let _ = import_page.update(cx, |page, cx| page.import_dropped_paths(Some(destination), paths, cx));
                            })
                            .on_drop::<FolderTargets>(move |targets, window, cx| {
                                let targets = targets.clone();
                                let _ = drop_page.update(cx, |page, cx| page.drop_onto_folder(destination, targets, window, cx));
                            })
                            // Dropping a folder on itself is the one case the
                            // backend would reject; catching it here keeps the
                            // cursor from promising a drop that cannot happen.
                            .can_drop(move |dragged, _, _| dragged.is::<gpui::ExternalPaths>() || dragged.downcast_ref::<FolderTargets>().is_some_and(|targets| !targets.folders.contains(&destination)));
                    }
                    if let Some(targets) = drag_targets::<A>(chip_index, &drag_chips, &drag_groups, &drag_books, &selected_now, source, cx) {
                        let label = SharedString::from(drag_label(&targets));
                        let preview_items = drag_preview_items::<A>(chip_index, &drag_chips, &drag_books, &selected_now, chip_icon, cx);
                        frame = frame.on_drag(targets, move |_, grab, _, cx| cx.new(|_| DragPreview { items: preview_items.clone(), label: label.clone(), grab, detailed_books }));
                    }
                }
                frame.into_any_element()
            })
            .with_intrinsic_width(move |window, cx| components::compact_hierarchy_item_width(chip_icon, width_label.clone(), width_count, show_download, window, cx))
            .with_activate(move |window, cx| {
                let _ = activate_page.update(cx, |page, cx| page.open_child(activate_location.clone(), window, cx));
            }),
        );
    }

    for (book_index, book) in books.iter().enumerate() {
        let prepared_book = book.clone();
        let rendered_book = book.clone();
        let activated_book = book.clone();
        let drag_chips = chips.clone();
        let drag_groups = chip_groups.to_vec();
        let drag_books = books.clone();
        let selected = selected.clone();
        // Books are declared after chips, so a book's flat child index is its
        // position plus the chip count.
        let child_index = chips.len() + book_index;
        let child = PaginatorChild::new(move |_, cx| {
            // Identified because a drag source has to be a stateful element.
            let mut frame = div().id(SharedString::from(format!("browse-book-frame-{child_index}"))).w_full().min_w_0().child(rendered_book.clone());
            let selected_now = selected.read(cx).indices.clone();
            if let Some(source) = source
                && let Some(targets) = drag_targets::<A>(child_index, &drag_chips, &drag_groups, &drag_books, &selected_now, source, cx)
            {
                let label = SharedString::from(drag_label(&targets));
                let preview_items = drag_preview_items::<A>(child_index, &drag_chips, &drag_books, &selected_now, chip_icon, cx);
                frame = frame.on_drag(targets, move |_, grab, _, cx| cx.new(|_| DragPreview { items: preview_items.clone(), label: label.clone(), grab, detailed_books }));
            }
            frame.into_any_element()
        })
        .with_activate(move |window, cx| A::activate_book(&activated_book, window, cx))
        .with_prepare(move |window, cx| A::prepare_book(&prepared_book, window, cx));
        book_children.push(child);
    }

    let chip_width = PaginatorWidthPolicy::pixels(px(1.0));
    let mut groups = Vec::with_capacity(2);
    if !chip_children.is_empty() {
        let policy =
            PaginatorGroupPolicy::new(chip_width, PaginatorSizing::rems(rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM)).with_pixels(px(components::COMPACT_HIERARCHY_ITEM_HEIGHT_OFFSET)), px(components::COMPACT_HIERARCHY_ITEM_ROW_GAP))
                .with_gap(px(components::COMPACT_HIERARCHY_ITEM_ROW_GAP))
                .intrinsic()
                .flush_edges()
                .with_edge_inset(px(components::BOOK_CARD_PADDING));
        let mut children = chip_children.into_iter();
        let mut offset = 0;
        for section in chip_groups {
            if section.start > offset {
                groups.push(PaginatorGroup::new(policy, children.by_ref().take(section.start - offset)));
            }
            let label = section.parent.name.clone();
            let location = section.parent.id.clone();
            let header_page = page.clone();
            groups.push(PaginatorGroup::new(policy, children.by_ref().take(section.end - section.start)).with_header(PaginatorSizing::rems(rems(1.5)).with_pixels(px(components::SPACE_SM * 2.0)), move |_, cx| {
                let theme = components::browser_theme(cx);
                let page = header_page.clone();
                let location = location.clone();
                let selector = format!("browse-group-{}", location);
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .size_full()
                    .py(px(components::SPACE_SM))
                    .flex()
                    .items_center()
                    .gap(px(components::SPACE_SM))
                    .child(div().id("group-title").min_w_0().text_ellipsis().line_clamp(1).text_size(rems(components::TEXT_MD)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.text).child(label.clone()))
                    .child(div().flex_1().h(px(1.0)).bg(theme.rule))
                    .into_any_element()
            }));
            offset = section.end;
        }
        let remaining = children.collect::<Vec<_>>();
        if !remaining.is_empty() {
            groups.push(PaginatorGroup::new(policy, remaining));
        }
    }
    if !book_children.is_empty() {
        groups.push(PaginatorGroup::new(book_card_policy(detailed_books), book_children));
    }
    groups
}

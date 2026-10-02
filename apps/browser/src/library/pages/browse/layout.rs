//! Conversion from a listing to paginator groups.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, Entity, Pixels, Point, SharedString, WeakEntity, div, px, rems};
use library_model::BrowseRow;
use sync_common::DirId;
use ui_components as components;

use super::folder_actions::{FolderTargets, describe};
use super::listing::{BrowseItem, BrowseItems, book_id, chip_id, shows_download_mark};
use super::page::{BrowsePage, BrowseSplitSide};
use super::section_card::SectionCard;
use crate::library::widgets::{
    BookCardInteraction, BookCardPresentation, BookCardView, PaginatorChild, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorState, PaginatorWidthPolicy, book_card_policy, browse_section_card_policy,
};

/// Makes `element` a place to put things down in the folder `destination`:
/// files from the desktop are imported into it, and folders and books dragged
/// from the listing are moved or copied into it. Chips, breadcrumb segments and
/// the open space of the folder being browsed all take drops alike.
pub(super) fn folder_drop_target<E: ParentElement + InteractiveElement + Styled>(element: E, destination: DirId, name: SharedString, highlight: bool, page: WeakEntity<BrowsePage>, theme: components::BrowserTheme) -> E {
    let import_page = page.clone();
    let files_name = name.clone();
    let element = element
        // Says where the drag would land. Nested targets are drawn after the
        // one they sit in, so the innermost, which takes the drop, has the
        // last word.
        .drag_over::<FolderTargets>(move |style, targets, _, cx| {
            cx.set_global(DropHint(Some(Hint { destination, name: name.clone(), valid: targets.accepts(destination) })));
            style
        })
        .drag_over::<gpui::ExternalPaths>(move |style, _, _, cx| {
            cx.set_global(DropHint(Some(Hint { destination, name: files_name.clone(), valid: true })));
            style
        })
        .on_drop::<gpui::ExternalPaths>(move |paths, _, cx| {
            let paths = paths.paths().to_vec();
            let _ = import_page.update(cx, |page, cx| page.import_dropped_paths(Some(destination), paths, cx));
        })
        .on_drop::<FolderTargets>(move |targets, window, cx| {
            let targets = targets.clone();
            let _ = page.update(cx, |page, cx| page.drop_onto_folder(destination, targets, window, cx));
        })
        // Refusing a drop that cannot happen keeps the cursor from promising
        // one.
        .can_drop(move |dragged, _, _| dragged.is::<gpui::ExternalPaths>() || dragged.downcast_ref::<FolderTargets>().is_some_and(|targets| targets.accepts(destination)));
    // The outline is the target's last child, drawn after everything inside
    // it, so by then the innermost target under the pointer has said where the
    // drop goes, and only that one lights up.
    if highlight { element.relative().child(components::drop_highlight::<FolderTargets>(move |cx| lands_in(destination, cx), theme)) } else { element }
}

/// Whether the drag under way would land in `destination`, and be taken.
fn lands_in(destination: DirId, cx: &App) -> bool {
    cx.try_global::<DropHint>().and_then(|hint| hint.0.as_ref()).is_some_and(|hint| hint.destination == destination && hint.valid)
}

/// Where the drag under way would land, as the innermost folder under the
/// pointer reported it, and whether it would take the drop there. Targets set
/// it while they are drawn with the drag over them; the drag preview takes it
/// as it draws itself, so it names nothing once the pointer leaves every
/// folder.
struct DropHint(Option<Hint>);

#[derive(Clone)]
struct Hint {
    destination: DirId,
    name: SharedString,
    valid: bool,
}

impl gpui::Global for DropHint {}

/// The preview that follows the pointer during a drag.
///
/// A single item keeps its shape so it is immediately clear what is being
/// moved. Rectangular and multi-item drags use a compact icon grid so the
/// whole selection remains visible without obscuring the pointer.
struct DragPreview {
    items: Vec<DragPreviewItem>,
    label: SharedString,
    /// Where inside the dragged element the drag began. GPUI paints this view
    /// at `pointer - cursor_offset`; the small nudge keeps the ghost clear of
    /// the pointer while it is being carried.
    grab: Point<Pixels>,
}

#[derive(Clone)]
pub(super) enum DragPreviewItem {
    Book(Entity<BookCardView>),
    Folder { icon: components::NavigationIcon, label: SharedString, path: Option<SharedString> },
}

impl gpui::Render for DragPreview {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
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
                .children(self.items.iter().map(|item| match item {
                    DragPreviewItem::Book(book) => div().w(px(components::BOOK_CARD_WIDTH)).flex_none().child(book.clone()).into_any_element(),
                    DragPreviewItem::Folder { icon, label, path } => components::compact_hierarchy_item(format!("drag-preview-folder-{label}"), *icon, label.clone(), path.clone(), None, None, false, false, theme).into_any_element(),
                }))
                .into_any_element()
        };
        let hint = cx.try_global::<DropHint>().and_then(|hint| hint.0.clone());
        cx.set_global(DropHint(None));
        // Says where the drop would go, or that it would go nowhere; a drop it
        // cannot make also fades what is being carried.
        let caption = hint.as_ref().map(|hint| components::drag_preview(SharedString::from(if hint.valid { format!("{} → {}", self.label, hint.name) } else { format!("Can't drop in {}", hint.name) }), theme));
        let refused = hint.is_some_and(|hint| !hint.valid);
        // Keep the preview visible against the page while making it clear that
        // it is a transient copy rather than the original card.
        div()
            .pl(self.grab.x + px(DRAG_PREVIEW_NUDGE))
            .pt(self.grab.y + px(DRAG_PREVIEW_NUDGE))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(components::SPACE_XS))
            .child(div().opacity(if refused { 0.3 } else { 0.58 }).child(preview))
            .children(caption)
    }
}

/// Lets `element` be picked up, carrying `targets`, with a preview of `items`
/// following the pointer.
pub(super) fn folder_drag_source<E: StatefulInteractiveElement>(element: E, targets: FolderTargets, items: Vec<DragPreviewItem>) -> E {
    let label = SharedString::from(describe(&targets));
    element.on_drag(targets, move |_, grab, _, cx| cx.new(|_| DragPreview { items: items.clone(), label: label.clone(), grab }))
}

fn drag_preview_items(items: &[BrowseItem], chip_icon: components::NavigationIcon) -> Vec<DragPreviewItem> {
    items
        .iter()
        .map(|item| match item {
            BrowseItem::Child { row, .. } => DragPreviewItem::Folder { icon: chip_icon, label: row.name.clone().into(), path: row.path.clone().map(SharedString::from) },
            BrowseItem::Book(book) => DragPreviewItem::Book(book.clone()),
        })
        .collect()
}

/// A child opens, splits, shows its resolved menu, and carries folder placements.
pub(super) fn child_frame(
    element: impl IntoElement, row: &BrowseRow, page: WeakEntity<BrowsePage>, source: Option<DirId>, items: Vec<BrowseItem>, in_selection: bool, chip_icon: components::NavigationIcon, theme: components::BrowserTheme, cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let location = row.id.clone();
    let split_location = location.clone();
    let open_page = page.clone();
    let menu_page = page.clone();
    let menu_items = items.clone();
    let mut frame = div()
        .id(format!("browse-chip-frame-{}", row.id))
        .w_full()
        .h_full()
        .child(element)
        .on_click(move |event, window, cx| {
            let _ = open_page.update(cx, |page, cx| {
                if event.modifiers().control && page.can_split(window) {
                    page.request_split_child(location.clone(), BrowseSplitSide::Left, cx);
                } else {
                    page.open_child(location.clone(), window, cx);
                }
            });
            cx.stop_propagation();
        })
        .on_aux_click(move |event, window, cx| {
            if !event.is_secondary() {
                return;
            }
            let split = event.modifiers().control && menu_page.upgrade().is_some_and(|page| page.read(cx).can_split(window));
            if split {
                let _ = menu_page.update(cx, |page, cx| page.request_split_child(split_location.clone(), BrowseSplitSide::Right, cx));
            } else if in_selection {
                return;
            } else {
                let _ = menu_page.update(cx, |page, cx| page.open_resolved_items_menu(menu_items.clone(), event.position(), window, cx));
            }
            cx.stop_propagation();
        });
    if let Some(source) = source {
        if let library_model::BrowseLocation::Folder(destination) = row.id {
            frame = folder_drop_target(frame, destination, row.name.clone().into(), true, page, theme);
        }
        if let Some(targets) = FolderTargets::from_items(&items, source, cx) {
            frame = folder_drag_source(frame, targets, drag_preview_items(&items, chip_icon));
        }
    }
    frame
}

pub(super) fn download_mark(row: &BrowseRow, page: WeakEntity<BrowsePage>, theme: components::BrowserTheme) -> Option<gpui::AnyElement> {
    shows_download_mark(row).then(|| {
        let location = row.id.clone();
        div()
            .id(format!("chip-download-{location}"))
            .debug_selector(move || format!("chip-download-{location}"))
            .flex_none()
            .cursor_pointer()
            .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Download remaining books to this device").build(window, cx))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click({
                let location = row.id.clone();
                move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = page.update(cx, |page, cx| page.download_child(location.clone(), cx));
                }
            })
            .child(components::download_glyph().text_color(theme.text))
            .into_any_element()
    })
}

/// The three book rails share identity, activation and cover preparation.
pub(super) fn book_child(
    book: Entity<BookCardView>, presentation: BookCardPresentation, interaction: impl Fn(&PaginatorState, &SharedString, &App) -> BookCardInteraction + 'static,
    drag: Option<Rc<dyn Fn(&PaginatorState, &SharedString, &App) -> Option<(FolderTargets, Vec<DragPreviewItem>)>>>, cx: &App,
) -> PaginatorChild {
    let item_id = book_id(book.read(cx).content_hash());
    let frame_id = item_id.clone();
    let rendered = book.clone();
    let activated = book.clone();
    PaginatorChild::new(move |state, _, cx| {
        let card = BookCardView::present(rendered.clone(), presentation, interaction(state, &frame_id, cx));
        let mut frame = div().id(frame_id.clone()).w_full().h_full().min_w_0().child(card);
        if presentation == BookCardPresentation::Section {
            frame = frame.on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation());
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
    page: WeakEntity<BrowsePage>,
    chips: Rc<Vec<BrowseRow>>,
    chip_icon: components::NavigationIcon,
    books: Rc<Vec<Entity<BookCardView>>>,
    cover_text: app_preferences::CoverText,
    // The folder being browsed. Present only for listings whose children are
    // real directories, which are the only ones that can be dragged.
    source: Option<DirId>,
    section_cards: &HashMap<library_model::BrowseLocation, Entity<SectionCard>>,
    items: BrowseItems,
    cx: &App,
) -> Vec<PaginatorGroup> {
    let mut chip_children = Vec::with_capacity(chips.len());
    let mut book_children = Vec::with_capacity(books.len());

    for chip in chips.iter() {
        let width_label = SharedString::from(chip.name.clone());
        let child_count = Some(chip.book_count);
        let show_download = shows_download_mark(chip);
        let item_id = chip_id(chip.id);
        let rendered_id = item_id.clone();
        let chip = chip.clone();
        let page = page.clone();
        let drag_items = items.clone();
        let section_card = section_cards.get(&chip.id).cloned();
        // Chips previously carried a click handler only, so the paginator had
        // nothing to invoke for them. Enter and a click now share one path.
        let activate_page = page.clone();
        let activate_location = chip.id.clone();
        chip_children.push(
            PaginatorChild::new(move |state, _, cx| {
                let focused = state.is_focused(&rendered_id) || state.is_selected(&rendered_id);
                let theme = components::browser_theme(cx);
                let item = match &section_card {
                    None => components::compact_hierarchy_item(
                        format!("browse-chip-{}", chip.id),
                        chip_icon,
                        chip.name.clone(),
                        chip.path.clone().map(SharedString::from),
                        child_count,
                        download_mark(&chip, page.clone(), theme),
                        focused,
                        state.marquee,
                        theme,
                    )
                    .debug_selector(|| format!("browse-chip-{}", chip.id))
                    .into_any_element(),
                    Some(card) => SectionCard::present(card.clone(), focused, state.marquee).into_any_element(),
                };
                let selected = state.target_ids(&rendered_id).iter().filter_map(|id| drag_items.get(id).cloned()).collect();
                let frame = child_frame(item, &chip, page.clone(), source, selected, state.is_selected(&rendered_id), chip_icon, theme, cx);
                let frame = if section_card.is_some() { frame.relative() } else { frame.h(rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM)) };
                frame.into_any_element()
            })
            .with_id(item_id)
            .with_intrinsic_width(move |window, cx| components::compact_hierarchy_item_width(chip_icon, width_label.clone(), child_count, show_download, window, cx))
            .with_activate(move |window, cx| {
                let _ = activate_page.update(cx, |page, cx| page.open_child(activate_location.clone(), window, cx));
            }),
        );
    }

    for book in books.iter() {
        let drag_items = items.clone();
        let drag = source.map(|source| {
            Rc::new(move |state: &PaginatorState, id: &SharedString, cx: &App| {
                let selected = state.target_ids(id).iter().filter_map(|id| drag_items.get(id).cloned()).collect::<Vec<_>>();
                FolderTargets::from_items(&selected, source, cx).map(|targets| (targets, drag_preview_items(&selected, chip_icon)))
            }) as Rc<dyn Fn(&PaginatorState, &SharedString, &App) -> Option<(FolderTargets, Vec<DragPreviewItem>)>>
        });
        book_children.push(book_child(book.clone(), BookCardPresentation::Grid, |state, id, _| BookCardInteraction { focused: state.is_focused(id), selected: state.is_selected(id), marquee: state.marquee }, drag, cx));
    }

    let chip_width = PaginatorWidthPolicy::pixels(px(1.0));
    let mut groups = Vec::with_capacity(2);
    if !chip_children.is_empty() {
        let policy = if section_cards.is_empty() {
            PaginatorGroupPolicy::new(chip_width, PaginatorSizing::rems(rems(components::COMPACT_HIERARCHY_ITEM_HEIGHT_REM)), px(components::COMPACT_HIERARCHY_ITEM_ROW_GAP))
                .with_gap(px(components::COMPACT_HIERARCHY_ITEM_ROW_GAP))
                .intrinsic()
                .flush_edges()
                .with_edge_inset(px(components::BOOK_CARD_PADDING))
        } else {
            browse_section_card_policy()
        };
        groups.push(PaginatorGroup::new(policy, chip_children));
    }
    if !book_children.is_empty() {
        groups.push(PaginatorGroup::new(book_card_policy(cover_text), book_children));
    }
    groups
}

const DRAG_PREVIEW_NUDGE: f32 = 12.0;

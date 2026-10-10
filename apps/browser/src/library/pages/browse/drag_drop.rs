//! Shared folder drag sources, destinations and previews.

use std::any::Any;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, Entity, ExternalPaths, Pixels, Point, SharedString, div, px};
use sync_common::DirId;
use ui_components as components;

use super::cards::{CardEvent, CardEvents};
use super::folder_actions::{FolderTargets, describe};
use super::listing::BrowseItem;
use crate::library::widgets::{BookCardView, PaginatorState};

/// A drop into a folder, regardless of where the drag began.
#[derive(Clone)]
pub(super) enum FolderDrop {
    Files(Vec<PathBuf>),
    Items(FolderTargets),
}

/// Adapts GPUI's typed drag payloads to the folder's shared drop behavior.
trait FolderDrag: 'static {
    fn accepts(&self, destination: DirId) -> bool;
    fn contents(&self) -> FolderDrop;
}

impl FolderDrag for FolderTargets {
    fn accepts(&self, destination: DirId) -> bool {
        FolderTargets::accepts(self, destination)
    }

    fn contents(&self) -> FolderDrop {
        FolderDrop::Items(self.clone())
    }
}

impl FolderDrag for ExternalPaths {
    fn accepts(&self, _: DirId) -> bool {
        true
    }

    fn contents(&self) -> FolderDrop {
        FolderDrop::Files(self.paths().to_vec())
    }
}

fn accepts_drop(dragged: &dyn Any, destination: DirId) -> bool {
    if let Some(targets) = dragged.downcast_ref::<FolderTargets>() { targets.accepts(destination) } else { dragged.downcast_ref::<ExternalPaths>().is_some_and(|paths| paths.accepts(destination)) }
}

/// Registers the same hint and event handling for either GPUI payload type.
fn bind_folder_drop<T: FolderDrag, E: InteractiveElement>(element: E, destination: DirId, name: SharedString, events: CardEvents) -> E {
    element
        // Nested targets are drawn after their parent, so the innermost
        // target has the last word about where the drag would land.
        .drag_over::<T>(move |style, dragged, _, cx| {
            cx.set_global(DropHint(Some(Hint { destination, name: name.clone(), valid: dragged.accepts(destination) })));
            style
        })
        .on_drop::<T>(move |dragged, window, cx| events(CardEvent::DropOnto(destination, dragged.contents()), window, cx))
}

/// Accepts external files through the same import flow as a folder drop.
/// Query pages use this without offering to relocate existing library items.
pub(super) fn file_drop_target<E: InteractiveElement>(element: E, destination: DirId, name: SharedString, events: CardEvents) -> E {
    bind_folder_drop::<ExternalPaths, _>(element, destination, name, events).can_drop(|dragged, _, _| dragged.is::<ExternalPaths>())
}

/// Chips, breadcrumbs and open space all accept files to import or library
/// items to move/copy, with the same destination and highlight behavior.
pub(super) fn folder_drop_target<E: ParentElement + InteractiveElement + Styled>(element: E, destination: DirId, name: SharedString, highlight: bool, events: CardEvents, theme: components::BrowserTheme) -> E {
    let element = bind_folder_drop::<FolderTargets, _>(element, destination, name.clone(), events.clone());
    let element = file_drop_target(element, destination, name, events).can_drop(move |dragged, _, _| accepts_drop(dragged, destination));
    // Draw the outline last, after nested targets have reported their hint.
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

/// What a drag carries out of the folder `source`, and the preview it shows.
pub(super) type DragPayload = (FolderTargets, Vec<DragPreviewItem>);

/// The drag a book card starts, from the paginator state and the card's id.
pub(super) type BookDrag = Rc<dyn Fn(&PaginatorState, &SharedString, &App) -> Option<DragPayload>>;

/// Picking up `items` out of `source`, if any of them can move.
pub(super) fn drag_payload(items: &[BrowseItem], source: DirId, chip_icon: components::NavigationIcon, cx: &App) -> Option<DragPayload> {
    let targets = FolderTargets::from_items(items, source, cx)?;
    let preview = items
        .iter()
        .map(|item| match item {
            BrowseItem::Child { row, .. } => DragPreviewItem::Folder { icon: chip_icon, label: row.name.clone().into(), path: row.path.clone().map(SharedString::from) },
            BrowseItem::Book(book) => DragPreviewItem::Book(book.clone()),
        })
        .collect();
    Some((targets, preview))
}

const DRAG_PREVIEW_NUDGE: f32 = 12.0;

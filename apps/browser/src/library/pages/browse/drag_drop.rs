//! Shared folder drag sources, destinations and previews.

use std::any::Any;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, ApplicationDrag, Entity, ExternalDragPayload, ExternalPaths, Pixels, Point, SharedString, WindowId, div, px};
use sync_common::DirId;
use ui_components as components;
use uuid::Uuid;

use super::cards::{CardEvent, CardEvents};
use super::folder_actions::{FolderTargets, describe};
use super::listing::BrowseItem;
use crate::library::widgets::{BookCardView, PaginatorState};

/// A drop into a folder, regardless of where the drag began.
#[derive(Clone)]
pub(super) enum FolderDrop {
    Files(Vec<PathBuf>),
    Items(LibraryDrag),
}

/// Library identity accompanies folder IDs when a drag crosses windows.
#[derive(Clone)]
pub(super) struct LibraryDrag {
    pub library_id: Uuid,
    pub targets: FolderTargets,
}

impl LibraryDrag {
    pub(super) fn accepts(&self, library_id: Uuid, destination: DirId) -> bool {
        if self.targets.is_empty() || self.targets.folders.contains(&sync_common::ROOT_DIR_ID) {
            return false;
        }
        if self.library_id == library_id {
            return self.targets.source.is_some() && self.targets.accepts(destination);
        }
        cfg!(not(target_arch = "wasm32"))
    }
}

/// Adapts GPUI's typed drag payloads to the folder's shared drop behavior.
trait FolderDrag: 'static {
    fn accepts(&self, library_id: Uuid, destination: DirId) -> bool;
    fn contents(&self) -> FolderDrop;
}

impl FolderDrag for LibraryDrag {
    fn accepts(&self, library_id: Uuid, destination: DirId) -> bool {
        LibraryDrag::accepts(self, library_id, destination)
    }

    fn contents(&self) -> FolderDrop {
        FolderDrop::Items(self.clone())
    }
}

impl FolderDrag for ExternalPaths {
    fn accepts(&self, _: Uuid, _: DirId) -> bool {
        true
    }

    fn contents(&self) -> FolderDrop {
        FolderDrop::Files(self.paths().to_vec())
    }
}

fn accepts_drop(dragged: &dyn Any, library_id: Uuid, destination: DirId) -> bool {
    if let Some(targets) = dragged.downcast_ref::<LibraryDrag>() { targets.accepts(library_id, destination) } else { dragged.downcast_ref::<ExternalPaths>().is_some_and(|paths| paths.accepts(library_id, destination)) }
}

/// Registers the same hint and event handling for either GPUI payload type.
fn bind_folder_drop<T: FolderDrag, E: InteractiveElement>(element: E, library_id: Uuid, destination: DirId, name: SharedString, events: CardEvents) -> E {
    element
        // Nested targets are drawn after their parent, so the innermost
        // target has the last word about where the drag would land.
        .drag_over::<T>(move |style, dragged, window, cx| {
            cx.set_global(DropHint(Some(Hint { window: window.window_handle().window_id(), library_id, destination, name: name.clone(), valid: dragged.accepts(library_id, destination) })));
            style
        })
        .on_drop::<T>(move |dragged, window, cx| events(CardEvent::DropOnto(destination, dragged.contents()), window, cx))
}

/// Query pages import cross-library items into root, and also accept external files.
pub(super) fn import_drop_target<E: InteractiveElement>(element: E, library_id: Uuid, destination: DirId, name: SharedString, events: CardEvents) -> E {
    let element = bind_folder_drop::<LibraryDrag, _>(element, library_id, destination, name.clone(), events.clone());
    bind_folder_drop::<ExternalPaths, _>(element, library_id, destination, name, events)
        .can_drop(move |dragged, _, _| dragged.is::<ExternalPaths>() || dragged.downcast_ref::<LibraryDrag>().is_some_and(|drag| drag.library_id != library_id && drag.accepts(library_id, destination)))
}

/// Chips, breadcrumbs and open space all accept files to import or library
/// items to move/copy, with the same destination and highlight behavior.
pub(super) fn folder_drop_target<E: ParentElement + InteractiveElement + Styled>(element: E, library_id: Uuid, destination: DirId, name: SharedString, highlight: bool, events: CardEvents, theme: components::BrowserTheme) -> E {
    let element = bind_folder_drop::<LibraryDrag, _>(element, library_id, destination, name.clone(), events.clone());
    let element = bind_folder_drop::<ExternalPaths, _>(element, library_id, destination, name, events).can_drop(move |dragged, _, _| accepts_drop(dragged, library_id, destination));
    // Draw the outline last, after nested targets have reported their hint.
    if highlight { element.relative().child(components::drop_highlight::<LibraryDrag>(move |cx| lands_in(library_id, destination, cx), theme)) } else { element }
}

/// Whether the drag under way would land in `destination`, and be taken.
fn lands_in(library_id: Uuid, destination: DirId, cx: &App) -> bool {
    cx.try_global::<DropHint>().and_then(|hint| hint.0.as_ref()).is_some_and(|hint| hint.library_id == library_id && hint.destination == destination && hint.valid)
}

/// Where the drag under way would land, as the innermost folder under the
/// pointer reported it, and whether it would take the drop there. Targets set
/// it while they are drawn with the drag over them; the drag preview takes it
/// as it draws itself, so it names nothing once the pointer leaves every
/// folder.
struct DropHint(Option<Hint>);

#[derive(Clone)]
struct Hint {
    window: WindowId,
    library_id: Uuid,
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
                .children(self.items.iter().map(|item| match item {
                    DragPreviewItem::Book(book) => div().w(px(components::BOOK_CARD_WIDTH)).flex_none().child(book.clone()).into_any_element(),
                    DragPreviewItem::Folder { icon, label, path } => components::compact_hierarchy_item(format!("drag-preview-folder-{label}"), *icon, label.clone(), path.clone(), None, None, false, false, false, theme).into_any_element(),
                }))
                .into_any_element()
        };
        let hint = cx.try_global::<DropHint>().and_then(|hint| hint.0.clone()).filter(|hint| hint.window == window.window_handle().window_id());
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
pub(super) fn folder_drag_source<E: StatefulInteractiveElement>(element: E, targets: LibraryDrag, items: Vec<DragPreviewItem>) -> E {
    let label = SharedString::from(describe(&targets.targets));
    element
        .on_drag(targets, move |_, grab, _, cx| {
            cx.set_global(DropHint(None));
            cx.new(|_| DragPreview { items: items.clone(), label: label.clone(), grab })
        })
        .external_drag_payload::<LibraryDrag>(|_, _, _| Some(ExternalDragPayload::Application(ApplicationDrag::new())))
}

/// What a drag carries out of the folder `source`, and the preview it shows.
pub(super) type DragPayload = (LibraryDrag, Vec<DragPreviewItem>);

/// The drag a book card starts, from the paginator state and the card's id.
pub(super) type BookDrag = Rc<dyn Fn(&PaginatorState, &SharedString, &App) -> Option<DragPayload>>;

/// Picking up `items` out of `source`, if any of them can move.
pub(super) fn drag_payload(items: &[BrowseItem], source: Option<DirId>, library_id: Uuid, chip_icon: components::NavigationIcon, cx: &App) -> Option<DragPayload> {
    let items: Vec<_> = items.iter().filter(|item| source.is_some() || matches!(item, BrowseItem::Book(_))).cloned().collect();
    let mut targets = FolderTargets::from_items(&items, source.unwrap_or(sync_common::ROOT_DIR_ID), cx)?;
    if source.is_none() {
        targets.source = None;
    }
    let preview = items
        .iter()
        .map(|item| match item {
            BrowseItem::Child { row, .. } => DragPreviewItem::Folder { icon: chip_icon, label: row.name.clone().into(), path: row.path.clone().map(SharedString::from) },
            BrowseItem::Book(book) => DragPreviewItem::Book(book.clone()),
        })
        .collect();
    Some((LibraryDrag { library_id, targets }, preview))
}

const DRAG_PREVIEW_NUDGE: f32 = 12.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_library_drag_from_all_books_requires_a_different_library() {
        let library_id = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        let drag = LibraryDrag { library_id, targets: FolderTargets { books: vec![sync_common::BookId::from_bytes([1; 32])], ..Default::default() } };
        assert!(!drag.accepts(library_id, sync_common::ROOT_DIR_ID));
        assert_eq!(drag.accepts(other, sync_common::ROOT_DIR_ID), cfg!(not(target_arch = "wasm32")));
    }

    #[test]
    fn cross_library_drag_rejects_empty_selections_and_library_roots() {
        let library_id = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        let mut drag = LibraryDrag { library_id, targets: FolderTargets::default() };
        assert!(!drag.accepts(other, sync_common::ROOT_DIR_ID));
        drag.targets.folders.push(sync_common::ROOT_DIR_ID);
        assert!(!drag.accepts(other, sync_common::ROOT_DIR_ID));
    }

    #[test]
    fn cross_library_drag_accepts_identical_folder_ids_without_local_no_op_checks() {
        let library_id = Uuid::from_u128(1);
        let other_library_id = Uuid::from_u128(2);
        let source = Uuid::from_u128(3);
        let destination = Uuid::from_u128(4);
        let drag = LibraryDrag { library_id, targets: FolderTargets { source: Some(source), books: vec![sync_common::BookId::from_bytes([1; 32])], ..Default::default() } };
        assert!(drag.accepts(library_id, destination));
        assert_eq!(drag.accepts(other_library_id, destination), cfg!(not(target_arch = "wasm32")));
        assert_eq!(drag.accepts(other_library_id, source), cfg!(not(target_arch = "wasm32")));
        assert!(!drag.accepts(library_id, source));
    }
}

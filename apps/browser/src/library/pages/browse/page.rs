//! Shared browse-page state and layout for folders and subjects.

use crate::services::notify_error;
use crate::NewWindowTarget;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, DismissEvent, Entity, FocusHandle, Focusable as _, IntoElement, KeyDownEvent, Pixels, Point, Render, SharedString, Subscription, WeakEntity, Window, div, px, rems};
use gpui_component::IconName;
use gpui_component::breadcrumb::BreadcrumbItem;
use gpui_component::menu::PopupMenu;
use library_model::BrowseLocation;
use sync_common::DirId;
use ui_components as components;

use super::controls::{BrowseOptionsEvent, BrowseOptionsSelection, browse_options_control, browse_options_sheet};
use super::detail::{BookDetailPage, CloseBookDetail, DetailNavigation};
use super::folder_actions::{FolderActions, FolderActionsEvent, FolderTargets};
use super::graph::{BrowseGraph, GraphTap};
use super::layout::folder_drop_target;
use super::listing::{BrowseCrumb, BrowseDestination, BrowseItem, BrowseKind, BrowseListing, BrowseResponse, can_download, chip_id};
use super::search::{BrowseSearch, SearchChanged};
use super::section_card::SectionCard;
use crate::library::services::{LatestRequest, LibraryContentsChanged, LibraryContext, LibraryDownloadChanged, LibraryScanChanged};
use crate::library::widgets::{CursorMove, LoadedCover, OpenBookDetail, Paginator, PaginatorAxis, PaginatorGroup, PaginatorPosition, mobile_context_menu};
use crate::{DismissSettings, OpenContextMenu, UndoLibraryAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseSplitSide {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub(crate) struct BrowseSplitRequest {
    pub(crate) path: Vec<BrowseCrumb>,
    pub(crate) side: BrowseSplitSide,
}

pub(crate) struct BrowseSplitClose;

/// How the page shows its location: as section cards, or as a graph whose
/// column or circle layout is chosen separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowseView {
    Cards,
    Graph,
}

/// The single context menu over items, open space, or a pending drop.
struct ContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

pub(crate) struct BrowsePage {
    ui: LibraryContext,
    kind: BrowseKind,
    destination: BrowseDestination,
    listing: BrowseListing,
    paginator: Entity<Paginator>,
    /// The cards of a sectioned listing, by the id of the chip each belongs to.
    /// Kept across refreshes so a card keeps its rails' positions.
    section_cards: HashMap<BrowseLocation, Entity<SectionCard>>,
    positions: HashMap<BrowseLocation, (PaginatorPosition, PaginatorPosition)>,
    detail: DetailNavigation,
    _detail_subscription: Option<Subscription>,
    chip_menu: Option<ContextMenu>,
    focus: FocusHandle,
    restore_focus: bool,
    search: Entity<BrowseSearch>,
    /// The compact bottom-sheet counterpart to the desktop sort/filter dropdown.
    browse_options_sheet_open: bool,
    view: BrowseView,
    graph: Entity<BrowseGraph>,
    selection: BrowseOptionsSelection,
    error: Option<SharedString>,
    request: LatestRequest,
    /// The folder commands, on a folder page only: a subject is a query, not
    /// a directory, and has nothing to rename, move or trash.
    folder_actions: Option<Entity<FolderActions>>,
    _subscriptions: Vec<Subscription>,
    split_close_enabled: bool,
}

impl gpui::EventEmitter<BrowseSplitRequest> for BrowsePage {}
impl gpui::EventEmitter<BrowseSplitClose> for BrowsePage {}

/// Lets a card ask for its book's page. Weak, because the page holds the
/// cards that hold this.
fn open_detail(cx: &mut Context<BrowsePage>) -> OpenBookDetail {
    let page = cx.entity().downgrade();
    Rc::new(move |book, cover, cx| {
        let _ = page.update(cx, |page, cx| page.show_book_detail(book, cover, cx));
    })
}

impl BrowsePage {
    pub(crate) fn new(kind: BrowseKind, ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let folder_actions = (kind == BrowseKind::Folder).then(|| cx.new(|_| FolderActions::new(ui.clone())));
        let listing = BrowseListing::new(ui.clone(), kind, open_detail(cx), folder_actions.as_ref().map(FolderActions::place_book), folder_actions.as_ref().map(FolderActions::remove_book));
        let selection = BrowseOptionsSelection::for_controls(listing.controls());
        let page = cx.entity().downgrade();
        let selectable = folder_actions.is_some();
        let paginator = cx.new(|_| {
            let paginator = Paginator::new(format!("{}-browse-paginator", kind.list_prefix()), []).flow_axis(PaginatorAxis::Vertical);
            if !selectable {
                return paginator;
            }
            paginator.selectable().on_context_menu(move |id, position, window, cx| {
                let _ = page.update(cx, |page, cx| match id {
                    Some(id) if window.modifiers().control && page.can_split(window) && page.listing.item(&id).is_some_and(|item| item.child().is_some()) => {}
                    Some(id) => {
                        let items = page.menu_items(&id, cx);
                        page.open_resolved_items_menu(items, position, window, cx);
                    }
                    None => page.open_folder_menu(position, window, cx),
                });
            })
        });
        let search = cx.new(|_| BrowseSearch::new());
        let updates = ui.updates();
        let graph = cx.new(|cx| BrowseGraph::new(if kind == BrowseKind::Subject { components::GraphLayoutKind::AdaptiveRadial } else { components::GraphLayoutKind::Columns }, format!("{}-graph-covers", kind.list_prefix()), cx));
        let mut subscriptions = vec![
            cx.observe(&graph, |_, _, cx| cx.notify()),
            cx.subscribe(&graph, |page, _, action: &GraphTap, cx| match action {
                GraphTap::Select(path) => page.open_path(path.clone(), cx),
                GraphTap::Open => page.set_view(BrowseView::Cards, cx),
            }),
            cx.observe_global::<crate::stores::CoverTextSetting>(|page, cx| page.refresh_cover_text(cx)),
            cx.subscribe(&search, |page, _, SearchChanged(query): &SearchChanged, cx| {
                page.selection.search = query.clone();
                page.reload(cx);
            }),
            cx.subscribe(&updates, |page, _, _: &LibraryContentsChanged, cx| page.refresh_contents(cx)),
            cx.subscribe(&updates, |page, _, event: &LibraryDownloadChanged, cx| {
                if matches!(event.status.state, app::DownloadState::Downloaded | app::DownloadState::NotDownloaded) {
                    page.refresh_contents(cx);
                }
            }),
            // Only repaints: the empty listing reads whether a scan is running.
            cx.subscribe(&updates, |_, _, _: &LibraryScanChanged, cx| cx.notify()),
        ];
        if let Some(actions) = &folder_actions {
            subscriptions.push(cx.subscribe(actions, |page, _, event: &FolderActionsEvent, cx| page.handle_folder_action(event.clone(), cx)));
        }
        let mut page = Self {
            ui,
            kind,
            destination: BrowseDestination::from_path(kind, Vec::new()),
            listing,
            paginator,
            section_cards: HashMap::new(),
            positions: HashMap::new(),
            detail: DetailNavigation::default(),
            _detail_subscription: None,
            chip_menu: None,
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            restore_focus: false,
            search,
            browse_options_sheet_open: false,
            view: BrowseView::Cards,
            graph,
            selection,
            error: None,
            request: LatestRequest::default(),
            folder_actions,
            _subscriptions: subscriptions,
            split_close_enabled: false,
        };
        page.reload(cx);
        page
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(self.destination.clone(), false, cx);
    }

    pub(super) fn download_child(&mut self, location: BrowseLocation, cx: &mut Context<Self>) {
        if !self.listing.child(location).is_some_and(can_download) {
            return;
        }
        let query = self.selection.backend_query(location, false);
        let operation = super::listing::download(self.ui.backend(), query);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update_in(cx, |page, window, cx| {
                if let Err(error) = result {
                    notify_error("hierarchy-download-error", format!("Could not queue downloads: {error}"), window, cx);
                }
                page.refresh_contents(cx);
            });
        })
        .detach();
    }

    /// Refreshes scanner-driven content without replacing the current page by
    /// a loading frame. Newly discovered covers can prepare in place while the
    /// already-visible book cards remain painted.
    fn refresh_contents(&mut self, cx: &mut Context<Self>) {
        self.load(self.destination.clone(), true, cx);
    }

    fn handle_folder_action(&mut self, event: FolderActionsEvent, cx: &mut Context<Self>) {
        match event {
            FolderActionsEvent::Reload => self.reload(cx),
            FolderActionsEvent::Moved(folders) => {
                if matches!(self.destination.location, BrowseLocation::Folder(id) if folders.contains(&id)) {
                    self.open_path(Vec::new(), cx);
                } else {
                    self.reload(cx);
                }
            }
        }
    }

    fn menu_items(&self, id: &SharedString, cx: &App) -> Vec<BrowseItem> {
        self.paginator.read(cx).target_ids(id).iter().filter_map(|id| self.listing.item(id).cloned()).collect()
    }

    /// Every entry point uses the same resolved rows, placements and availability.
    pub(super) fn open_resolved_items_menu(&mut self, items: Vec<BrowseItem>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let single_chip = match items.as_slice() {
            [item] => item.child(),
            _ => None,
        };
        let split_target = single_chip.filter(|_| self.can_split(window)).map(|chip| chip.id.clone());
        let download_targets: Vec<BrowseLocation> = items.iter().filter_map(BrowseItem::child).filter(|chip| can_download(chip)).map(|chip| chip.id).collect();
        let targets = self.folder_actions.as_ref().and_then(|_| if let BrowseLocation::Folder(id) = self.destination.location { Some(id) } else { None }).and_then(|source| FolderTargets::from_items(&items, source, cx));
        let library_id = *self.ui.backend().id();
        let window_targets: Vec<NewWindowTarget> = if crate::native_windows_available() {
            items
                .iter()
                .filter_map(|item| match item {
                    BrowseItem::Child { row, .. } if targets.is_some() => Some(NewWindowTarget::Folder { library_id, directory_id: (if let BrowseLocation::Folder(id) = row.id { Some(id) } else { None })?, title: row.name.clone() }),
                    BrowseItem::Child { row, .. } if single_chip.is_some() => Some(NewWindowTarget::Subject { library_id, location: row.id, label: row.name.clone() }),
                    BrowseItem::Book(book) if targets.is_some() => Some(NewWindowTarget::Book { locator: library_model::BookLocator::new(library_id, book.read(cx).content_hash()), title: book.read(cx).title().into() }),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        let folder_commands = self.folder_actions.clone().zip(targets).map(|(actions, targets)| {
            let options = actions.read(cx).item_options(targets, &items, cx);
            (actions, options)
        });
        if window_targets.is_empty() && split_target.is_none() && download_targets.is_empty() && folder_commands.is_none() {
            return;
        }
        let mobile_size = mobile_context_menu::size(window);
        let page = cx.entity();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            if !window_targets.is_empty() {
                let targets = window_targets.clone();
                menu = menu.item(components::menu_item_with_icon("Open in New Window", IconName::ExternalLink, move |_, window, cx| {
                    for target in &targets {
                        window.dispatch_action(Box::new(crate::OpenInNewWindow(target.clone())), cx);
                    }
                }));
            }
            if let Some(child_id) = split_target.clone() {
                let (left_page, left_id) = (page.clone(), child_id.clone());
                let right_page = page.clone();
                menu = menu.item(components::menu_item_with_split_actions(
                    "Open left",
                    "Open right",
                    move |_, cx| {
                        left_page.update(cx, |page, cx| {
                            page.request_split_child(left_id.clone(), BrowseSplitSide::Left, cx);
                            page.dismiss_menu(cx);
                        })
                    },
                    move |_, cx| {
                        right_page.update(cx, |page, cx| {
                            page.request_split_child(child_id.clone(), BrowseSplitSide::Right, cx);
                            page.dismiss_menu(cx);
                        })
                    },
                ));
            }
            if !download_targets.is_empty() {
                let (download_page, targets) = (page.clone(), download_targets.clone());
                menu = menu.item(components::menu_item_with_icon("Download to This Device", components::download_glyph(), move |_, _, cx| {
                    download_page.update(cx, |page, cx| {
                        for target in &targets {
                            page.download_child(target.clone(), cx);
                        }
                    })
                }));
            }
            let menu = match folder_commands.clone() {
                Some((actions, commands)) => {
                    if !window_targets.is_empty() || split_target.is_some() || !download_targets.is_empty() {
                        menu = menu.separator();
                    }
                    FolderActions::item_menu_items(actions, commands, menu)
                }
                None => menu,
            };
            mobile_context_menu::configure(menu, mobile_size)
        });
        self.show_menu(menu, position, window, cx);
    }

    fn dismiss_menu(&mut self, cx: &mut Context<Self>) -> bool {
        if self.chip_menu.take().is_none() {
            return false;
        }
        self.restore_focus = true;
        cx.notify();
        true
    }

    fn show_menu(&mut self, menu: Entity<PopupMenu>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.subscribe_in(&menu, window, |page, _, _: &DismissEvent, _, cx| {
            page.dismiss_menu(cx);
        });
        menu.focus_handle(cx).focus(window, cx);
        self.chip_menu = Some(ContextMenu { menu, position, _subscription: subscription });
        cx.notify();
    }

    /// Imports paths dropped from the desktop into `destination`, or into the
    /// folder being browsed when the drop lands on open space.
    pub(super) fn import_dropped_paths(&mut self, destination: Option<DirId>, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(parent) = destination.or(if let BrowseLocation::Folder(id) = self.destination.location { Some(id) } else { None }) else { return };
        let ui = self.ui.clone();
        #[cfg(not(target_arch = "wasm32"))]
        ui.import_dropped_paths(parent, paths, cx);
        // The desktop hands over paths this build has no filesystem for.
        #[cfg(target_arch = "wasm32")]
        let _ = (ui, parent, paths, cx);
    }

    /// Asks what a drop onto a folder should do, rather than deciding it from a
    /// modifier held at the moment of release. The question is cheap, the two
    /// answers are not interchangeable, and a mistaken copy of a folder tree is
    /// tedious to unpick.
    pub(super) fn drop_onto_folder(&mut self, destination: DirId, targets: FolderTargets, window: &mut Window, cx: &mut Context<Self>) {
        if targets.all_in_folder(destination) {
            return;
        }
        let Some(actions) = self.folder_actions.clone() else { return };
        let name = self.listing.children().iter().find(|chip| chip.id == BrowseLocation::Folder(destination)).map(|chip| chip.name.clone()).unwrap_or_else(|| "this folder".to_owned());
        let position = window.mouse_position();
        let mobile_size = mobile_context_menu::size(window);
        let menu = PopupMenu::build(window, cx, move |menu, _, _| mobile_context_menu::configure(FolderActions::drop_menu_items(actions.clone(), targets.clone(), destination, &name, menu), mobile_size));
        self.show_menu(menu, position, window, cx);
        // What was dragged is about to land elsewhere; the indices it was
        // selected under will not survive the reload either way.
        self.paginator.update(cx, |paginator, cx| paginator.clear_selection(cx));
    }

    /// Opens the folder's own commands where the listing has no item — the
    /// same menu the toolbar's Add button drops down, at the pointer.
    pub(super) fn open_folder_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(actions) = self.folder_actions.clone() else { return };
        let BrowseLocation::Folder(parent_id) = self.destination.location else { return };
        let mobile_size = mobile_context_menu::size(window);
        let menu = PopupMenu::build(window, cx, move |menu, _, _| mobile_context_menu::configure(FolderActions::add_menu_items(actions.clone(), parent_id, menu), mobile_size));
        self.show_menu(menu, position, window, cx);
    }

    /// Opens the items menu from the keyboard, anchored under the cursor's own
    /// child rather than under a pointer.
    fn open_cursor_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let paginator = self.paginator.read(cx);
        let Some(cursor) = paginator.cursor() else { return false };
        let Some(bounds) = paginator.child_bounds(cursor) else { return false };
        let position = gpui::point(bounds.left(), bounds.bottom());
        let Some(id) = paginator.child_id(cursor).cloned() else { return false };
        self.open_resolved_items_menu(self.menu_items(&id, cx), position, window, cx);
        self.chip_menu.is_some()
    }

    pub(super) fn select_option(&mut self, event: BrowseOptionsEvent, cx: &mut Context<Self>) {
        if self.selection.apply(event, self.listing.controls()) {
            self.reload(cx);
        }
    }

    /// Hides the search box and hands the keyboard back to the listing.
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.close(cx));
        self.restore_focus = true;
        cx.notify();
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.read(cx).is_open() {
            self.close_search(window, cx);
        } else {
            self.search.update(cx, |search, cx| search.open(window, cx));
            cx.notify();
        }
    }

    pub(super) fn close_browse_options_sheet(&mut self, cx: &mut Context<Self>) {
        self.browse_options_sheet_open = false;
        self.restore_focus = true;
        cx.notify();
    }

    pub(super) fn toggle_browse_options_sheet(&mut self, cx: &mut Context<Self>) {
        self.browse_options_sheet_open = !self.browse_options_sheet_open;
        cx.notify();
    }

    /// Leaves the current folder for the one above it. Already at the root is
    /// not a failure, but it is unhandled, so the key falls through.
    fn open_parent(&mut self, cx: &mut Context<Self>) -> bool {
        // A book's page sits over the folder rather than inside it, so going
        // back closes it before leaving anything.
        if self.close_book_detail(cx) {
            return true;
        }
        if self.destination.path.is_empty() {
            return false;
        }
        let mut path = self.destination.path.clone();
        path.pop();
        self.open_path(path, cx);
        true
    }

    /// Moves the keyboard cursor. On a section card the card takes the move
    /// first — entering it, or moving within it — and only a move off its edge
    /// continues in the outer listing, entering whichever card that lands on.
    fn move_cursor(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        let mut focused = false;
        if let Some(card) = self.cursor_card(cx) {
            focused = card.read(cx).is_focused();
            if card.update(cx, |card, cx| card.move_cursor(movement, cx)) {
                return true;
            }
        }
        if !self.move_outer_cursor(movement, cx) {
            // Off a focused card and off the listing too: the key is still
            // spent, and the card keeps the keyboard.
            return focused;
        }
        if let Some(card) = self.cursor_card(cx) {
            card.update(cx, |card, cx| card.enter(movement, cx));
        }
        true
    }

    fn move_outer_cursor(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        let left = self.cursor_card(cx);
        if !self.paginator.update(cx, |paginator, cx| paginator.move_cursor(movement, cx)) {
            return false;
        }
        if let Some(card) = left {
            card.update(cx, |card, cx| card.leave(cx));
        }
        true
    }

    /// The section card the outer cursor is on, in a sectioned listing.
    fn cursor_card(&self, cx: &App) -> Option<Entity<SectionCard>> {
        let index = self.paginator.read(cx).cursor()?;
        let id = self.paginator.read(cx).child_id(index)?;
        self.section_cards.iter().find(|(location, _)| chip_id(**location) == *id).map(|(_, card)| card.clone())
    }

    /// The section card holding the keyboard, if one is.
    fn focused_card(&self, cx: &App) -> Option<Entity<SectionCard>> {
        self.cursor_card(cx).filter(|card| card.read(cx).is_focused())
    }

    fn navigate_paginator(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.dismiss(window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let search_focused = self.search.read(cx).is_focused(window, cx);
        if self.view == BrowseView::Graph && !search_focused && modifiers == gpui::Modifiers::default() {
            if self.graph.update(cx, |graph, cx| graph.handle_key(key, cx)) {
                cx.stop_propagation();
                return;
            }
        }

        if !search_focused
            && let Some(card) = self.cursor_card(cx)
            && let Some(handled) = card.update(cx, |card, cx| card.handle_key(event, window, cx))
        {
            if handled {
                cx.stop_propagation();
            }
            return;
        }

        let control_arrow = modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform && !modifiers.function;
        if !search_focused && control_arrow {
            let movement = match key {
                "left" => Some(CursorMove::Left),
                "right" => Some(CursorMove::Right),
                "up" => Some(CursorMove::Up),
                "down" => Some(CursorMove::Down),
                _ => None,
            };
            if let Some(movement) = movement {
                if self.move_outer_cursor(movement, cx) {
                    cx.stop_propagation();
                }
                return;
            }
        }

        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }

        // Arrows move the cursor; the page turns as a consequence of the cursor
        // walking off it. PageUp and PageDown remain the explicit whole-page
        // jump, and are the only way to page without moving the cursor.
        let handled = match key {
            "left" if !search_focused => self.move_cursor(CursorMove::Left, cx),
            "right" if !search_focused => self.move_cursor(CursorMove::Right, cx),
            "up" if !search_focused => self.move_cursor(CursorMove::Up, cx),
            "down" if !search_focused => self.move_cursor(CursorMove::Down, cx),
            // Deferred, because this handler runs inside BrowsePage's own update
            // and a chip's activation calls back into the page to open a folder.
            // A click reaches the same callback from outside any update, which is
            // why only the key path could re-enter. The activation is also taken
            // out of the paginator before being run, for the same reason one step
            // further down: it can navigate somewhere that needs no query, and
            // come straight back to the paginator to install what it found.
            "enter" if !search_focused => {
                let paginator = self.paginator.clone();
                window.defer(cx, move |window, cx| {
                    if let Some(activate) = paginator.read(cx).cursor_activation() {
                        activate(window, cx);
                    }
                });
                true
            }
            // Backspace is guarded on the search field: while typing a filter it
            // has to keep deleting characters rather than leaving the folder.
            "backspace" if !search_focused => self.open_parent(cx),
            "pageup" | "pagedown" => self.paginator.update(cx, |paginator, cx| if key == "pagedown" { paginator.next_with_axis(PaginatorAxis::Vertical, cx) } else { paginator.previous_with_axis(PaginatorAxis::Vertical, cx) }),
            _ => false,
        };

        if handled {
            cx.stop_propagation();
        }
    }

    /// Opens the menu over the keyboard cursor, for readers who are not holding
    /// a pointer. The menu takes the keyboard from here and walks with the
    /// arrows on its own.
    fn on_open_context_menu(&mut self, _: &OpenContextMenu, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open_cursor_menu(window, cx) {
            cx.propagate();
        }
    }

    /// Takes back the last library action — the menu command, drop, or rename
    /// that this library performed, whichever page performed it.
    fn on_undo(&mut self, _: &UndoLibraryAction, _window: &mut Window, cx: &mut Context<Self>) {
        let ui = self.ui.clone();
        let Some(entry) = ui.undo().take() else {
            cx.propagate();
            return;
        };
        let label = entry.label.clone();
        let library = ui.backend().clone();
        cx.spawn(async move |page, cx| {
            let result = crate::library::services::UndoStack::apply(entry, &library).await;
            let _ = page.update_in(cx, |page, window, cx| {
                page.reload(cx);
                match result {
                    Ok(()) => crate::services::notify_info("undo", format!("Undid: {label}"), window, cx),
                    // Undo is an inverse operation, not a rollback: the library
                    // can have moved on since, and then the inverse no longer
                    // fits what is there.
                    Err(error) => notify_error("undo-error", format!("Could not undo {label}: {error}"), window, cx),
                }
            });
        })
        .detach();
    }

    /// Dismiss the active surface before changing the view beneath it.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.dismiss_menu(cx) {
            return true;
        }
        if self.browse_options_sheet_open {
            self.close_browse_options_sheet(cx);
            return true;
        }
        if let Some(actions) = &self.folder_actions
            && actions.read(cx).has_modal()
        {
            actions.update(cx, |actions, cx| actions.dismiss_modal(cx));
            self.restore_focus = !actions.read(cx).has_modal();
            cx.notify();
            return true;
        }
        if self.search.read(cx).is_open() {
            self.close_search(window, cx);
            return true;
        }
        if let Some(card) = self.focused_card(cx) {
            card.update(cx, |card, cx| card.leave(cx));
            return true;
        }
        if self.paginator.update(cx, |paginator, cx| paginator.clear_selection(cx)) {
            return true;
        }
        self.open_parent(cx)
    }

    fn on_dismiss(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dismiss(window, cx) {
            cx.propagate();
        }
    }

    /// The trail to the child `child_id`: under the current path, through its
    /// group where the group is a real folder. The backend supplies the full
    /// displayed breadcrumb trail after the destination loads.
    fn child_path(&self, child_id: BrowseLocation) -> Option<Vec<BrowseCrumb>> {
        let item = self.listing.item(&chip_id(child_id))?;
        let child = item.child()?;
        let mut path = self.destination.path.clone();
        if let BrowseItem::Child { parent: Some(parent), .. } = item {
            let parent = self.listing.child(BrowseLocation::Folder(*parent))?;
            path.push(BrowseCrumb { location: parent.id, label: parent.name.clone() });
        }
        path.push(BrowseCrumb { location: child.id, label: child.name.clone() });
        Some(path)
    }

    pub(super) fn open_child(&mut self, child_id: BrowseLocation, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.child_path(child_id) else {
            log::warn!("ignored missing {} child {child_id}", self.kind.list_prefix());
            return;
        };
        if !self.selection.search.is_empty() {
            self.selection.search.clear();
            self.search.update(cx, |search, cx| search.clear(window, cx));
        }
        self.open_path(path, cx);
    }

    fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.close_book_detail(cx);
        self.load(BrowseDestination::from_path(self.kind, path), false, cx);
    }

    fn set_view(&mut self, view: BrowseView, cx: &mut Context<Self>) {
        self.view = view;
        self.fetch_graph_path(cx);
        cx.notify();
    }

    fn toggle_view(&mut self, cx: &mut Context<Self>) {
        self.set_view(if self.view == BrowseView::Cards { BrowseView::Graph } else { BrowseView::Cards }, cx);
    }
    /// Fetches the children of the locations above this one that the graph has
    /// not seen, for the fans along its path. The location itself arrives with
    /// every load.
    fn fetch_graph_path(&mut self, cx: &mut Context<Self>) {
        if self.view != BrowseView::Graph {
            return;
        }
        self.graph.update(cx, |graph, cx| graph.fetch_path(self.kind, &self.destination, &self.selection, self.ui.backend(), cx));
    }

    /// Requests replace their predecessors; the installed frame remains painted.
    fn load(&mut self, destination: BrowseDestination, refresh: bool, cx: &mut Context<Self>) {
        if destination.location != self.destination.location
            && let Some(actions) = &self.folder_actions
        {
            actions.update(cx, |actions, cx| actions.cancel_move_selection(cx));
        }
        let location = destination.location;
        let query = self.selection.backend_query(location, true);
        #[cfg(target_os = "android")]
        log::info!("Browse request: location={} formats={:?} languages={:?} search_length={}", location, query.file_types, query.languages, query.search.len());
        let generation = self.request.begin();
        // Navigation keeps the current frame painted until the next one is
        // ready. A browse query is a local read that returns inside a frame,
        // so covering the grid in the meantime only ever produced a flash —
        // one that appeared going back up the tree and not going down it, for
        // the same query against the same database.
        let operation = super::listing::request(self.ui.backend(), query);
        self.error = None;
        self.request.install(cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| page.finish_request(generation, destination, refresh, result, cx));
        }));
    }

    fn finish_request(&mut self, generation: u64, mut destination: BrowseDestination, refresh: bool, result: Result<BrowseResponse, String>, cx: &mut Context<Self>) {
        if !self.request.is_current(generation) {
            return;
        }
        match result {
            Ok(response) => {
                let changing_location = self.destination.location != destination.location;
                if changing_location {
                    self.positions.insert(self.destination.location, (self.paginator.read(cx).position(), self.graph.read(cx).covers().read(cx).position()));
                }
                destination.path = self.listing.install(response, cx);
                self.destination = destination;
                self.graph.update(cx, |graph, _| graph.remember(self.destination.location, self.listing.children().to_vec()));
                self.fetch_graph_path(cx);
                self.error = None;
                #[cfg(target_os = "android")]
                log::info!("Browse loaded: location={} children={} books={}", self.destination.location, self.listing.children().len(), self.listing.books().len());
                let groups = self.paginator_groups(cx);
                let graph_cover_groups = self.graph.read(cx).book_groups(self.listing.books(), cx);
                let restored = changing_location.then(|| self.positions.get(&self.destination.location)).flatten().cloned();
                for (paginator, groups, position) in
                    [(self.paginator.clone(), groups, restored.as_ref().map(|positions| &positions.0)), (self.graph.read(cx).covers().clone(), graph_cover_groups, restored.as_ref().map(|positions| &positions.1))]
                {
                    paginator.update(cx, |paginator, cx| {
                        if refresh {
                            paginator.refresh_groups(groups, cx);
                        } else {
                            paginator.set_groups(groups, cx);
                        }
                        if let Some(position) = position {
                            paginator.restore_position(position, cx);
                        }
                    });
                }
            }
            Err(error) => {
                log::warn!("Browse failed for {}: {}", destination.location, error);
                self.error = Some(error.into());
            }
        }
        cx.notify();
    }

    fn breadcrumb(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let mut root = components::folder_breadcrumb_root_item(self.kind.root_label(), self.destination.path.is_empty(), theme);
        if !self.destination.path.is_empty() {
            let root_page = page.clone();
            root = root.on_click(move |_, _, cx| root_page.update(cx, |page, cx| page.open_path(Vec::new(), cx)));
        }
        let mut breadcrumb = components::folder_breadcrumb().child(Self::breadcrumb_drop_target(root, sync_common::ROOT_DIR_ID, self.kind.root_label(), page.downgrade(), theme));
        for (index, crumb) in self.destination.path.iter().enumerate() {
            let current = index + 1 == self.destination.path.len();
            let mut item = components::folder_breadcrumb_item(crumb.label.clone(), current, theme);
            if !current {
                let path = self.destination.path[..=index].to_vec();
                let path_page = page.clone();
                item = item.on_click(move |_, _, cx| path_page.update(cx, |page, cx| page.open_path(path.clone(), cx)));
            }
            if let BrowseLocation::Folder(destination) = crumb.location {
                item = Self::breadcrumb_drop_target(item, destination, crumb.label.clone().into(), page.downgrade(), theme);
            }
            breadcrumb = breadcrumb.child(item);
        }
        components::folder_breadcrumb_trail(breadcrumb).into_any_element()
    }

    /// An ancestor folder is exactly as valid a destination for a drag as one
    /// shown in the grid below it, so each breadcrumb segment takes drops, and
    /// shows it will, the way a folder chip does.
    fn breadcrumb_drop_target(item: BreadcrumbItem, destination: DirId, name: SharedString, page: WeakEntity<Self>, theme: components::BrowserTheme) -> BreadcrumbItem {
        item.with_interaction(move |item| folder_drop_target(item, destination, name.clone(), true, page.clone(), theme))
    }

    fn paginator_groups(&mut self, cx: &mut Context<Self>) -> Vec<PaginatorGroup> {
        let mut cards = HashMap::new();
        if self.uses_section_layout() {
            let page = cx.entity().downgrade();
            for section in self.listing.sections() {
                let id = section.parent.id;
                let card = match self.section_cards.remove(&id) {
                    Some(card) => {
                        card.update(cx, |card, cx| card.set_section(section, cx));
                        card
                    }
                    None => cx.new(|cx| SectionCard::new(page.clone(), format!("{}-section-{id}", self.kind.list_prefix()), self.kind, section, cx)),
                };
                cards.insert(id, card);
            }
        }
        self.section_cards = cards;
        // Only a folder listing can be dragged: its children are directories,
        // and its books sit in a folder a move can name as their source.
        let source = self.folder_actions.is_some().then(|| if let BrowseLocation::Folder(id) = self.destination.location { Some(id) } else { None }).flatten();
        super::layout::paginator_groups(cx.entity().downgrade(), self.listing.children(), self.kind.child_icon(), self.listing.books(), crate::stores::current_cover_text(cx), source, &self.section_cards, self.listing.items(), cx)
    }

    fn uses_section_layout(&self) -> bool {
        self.selection.search.is_empty()
    }

    fn navigation_heading(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        if components::uses_mobile_navigation(window) {
            let title = if self.view == BrowseView::Graph { String::new() } else { self.destination.path.last().map(|crumb| crumb.label.clone()).unwrap_or_else(|| self.kind.root_label().to_string()) };
            let back = (!components::has_native_back_button()).then(|| components::mobile_back_button(components::browser_theme(cx)).on_click(|_, window, cx| window.dispatch_action(Box::new(DismissSettings), cx)));
            components::mobile_navigation_heading(title, back)
        } else {
            components::browser_topbar_left().when(self.view == BrowseView::Cards, |heading| heading.child(self.breadcrumb(cx)))
        }
    }

    fn trash_button(&self, cx: &App) -> Option<AnyElement> {
        (self.kind == BrowseKind::Folder).then(|| {
            components::outlined_icon_button("open-browse-trash", "Trash", components::navigation_icon(components::NavigationIcon::Trash2), components::browser_theme(cx))
                .debug_selector(|| "open-browse-trash".into())
                .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::OpenTrash), cx))
                .into_any_element()
        })
    }

    fn render_topbar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let compact_graph = self.view == BrowseView::Graph && components::WindowWidthClass::for_window(window).is_compact();
        let page = cx.entity();
        let search_page = page.clone();
        let search_field = self.search.read(cx).render();
        let search_open = search_field.is_some();
        let inline_search_field = if self.view == BrowseView::Cards { search_field } else { None };
        let search_button_label = if search_open { "Hide search" } else { "Search books or authors" };
        let browse_controls = self.listing.controls().clone();
        let has_options = !browse_controls.collection_sorts.is_empty() || !browse_controls.book_sorts.is_empty() || !browse_controls.formats.is_empty() || !browse_controls.languages.is_empty() || browse_controls.supports_direct_child_books;
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let close_split_page = page.clone();
        let split_page = page.clone();
        let view_page = page.clone();
        let (view_label, view_icon) = match self.view {
            BrowseView::Cards => ("Show as graph", IconName::Network),
            BrowseView::Graph => ("Show as cards", IconName::LayoutDashboard),
        };
        let graph_layout_switch = (self.view == BrowseView::Graph).then(|| self.graph.update(cx, |graph, cx| graph.layout_control(cx)));
        // In the graph the controls float at the bottom, so their menus open upward.
        let menu_anchor = if self.view == BrowseView::Graph { gpui::Anchor::BottomRight } else { gpui::Anchor::TopRight };
        let controls = components::browser_topbar_right()
            .when(inline_search_field.is_some(), |controls| controls.flex_1())
            .children(inline_search_field.map(|field| div().flex_1().min_w(rems(8.0)).child(field)))
            .when(!compact_graph, |controls| {
                controls.child(
                    components::outlined_icon_button("toggle-browse-search", search_button_label, IconName::Search, theme)
                        .when(search_open || !self.selection.search.is_empty(), |button| button.bg(theme.accent).text_color(theme.accent_text))
                        .on_click(move |_, window, cx| search_page.update(cx, |page, cx| page.toggle_search(window, cx))),
                )
            })
            .child(components::outlined_icon_button("toggle-browse-view", view_label, view_icon, theme).on_click(move |_, _, cx| view_page.update(cx, |page, cx| page.toggle_view(cx))))
            .children(graph_layout_switch)
            .when(!compact_graph, |controls| controls.children(self.folder_actions.as_ref().zip(if let BrowseLocation::Folder(id) = self.destination.location { Some(id) } else { None }).map(|(actions, parent)| FolderActions::render_trigger(actions.clone(), parent, menu_anchor, cx))))
            .children(self.trash_button(cx))
            .when(!compact_graph && self.can_split(window), |controls| {
                controls.child(
                    components::outlined_icon_button("split-browse-page", "Open split pane", IconName::PanelLeft, theme)
                        .debug_selector(|| "split-browse-button".into())
                        .on_click(move |_, _, cx| split_page.update(cx, |page, cx| page.request_split(BrowseSplitSide::Right, cx))),
                )
            })
            .when(!compact_graph && self.split_close_enabled && self.can_split(window), |controls| {
                controls.child(components::outlined_icon_button("close-split-pane", "Close split pane", IconName::Close, theme).on_click(move |_, _, cx| close_split_page.update(cx, |page, cx| page.request_close_split(cx))))
            })
            .when(has_options, |controls| controls.child(browse_options_control(page.clone(), browse_controls, self.selection.clone(), compact, menu_anchor, theme)));
        if self.view == BrowseView::Graph {
            div().h(gpui::rems(components::TOPBAR_HEIGHT_REM)).flex_none().flex().items_center().p(px(4.0)).rounded(px(8.0)).bg(theme.page_bg).border_1().border_color(theme.rule).child(controls).into_any_element()
        } else {
            components::browser_topbar(theme).child(self.navigation_heading(window, cx)).child(controls).into_any_element()
        }
    }

    fn render_browse_options_sheet(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.browse_options_sheet_open {
            return None;
        }
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let browse_controls = self.listing.controls().clone();
        Some(browse_options_sheet(page, browse_controls, self.selection.clone(), theme))
    }

    pub(super) fn can_split(&self, window: &Window) -> bool {
        crate::split_panes_available(window)
    }

    pub(super) fn set_split_close_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.split_close_enabled = enabled;
        cx.notify();
    }

    /// Rebuilds the grid's row height for the current "cover text" setting.
    ///
    /// The cards themselves pick the change up on their own — see
    /// `BookCardView`'s own global observer — but the row height the
    /// paginator reserves for them is decided here, and has to be recomputed
    /// alongside them or the grid keeps the old gap under a card that no
    /// longer has anything in it.
    fn refresh_cover_text(&mut self, cx: &mut Context<Self>) {
        let groups = self.paginator_groups(cx);
        self.paginator.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        cx.notify();
    }

    pub(crate) fn request_split(&mut self, side: BrowseSplitSide, cx: &mut Context<Self>) {
        cx.emit(BrowseSplitRequest { path: self.destination.path.clone(), side });
    }

    pub(crate) fn request_split_child(&mut self, child_id: BrowseLocation, side: BrowseSplitSide, cx: &mut Context<Self>) {
        if let Some(path) = self.child_path(child_id) {
            cx.emit(BrowseSplitRequest { path, side });
        }
    }

    pub(crate) fn request_close_split(&mut self, cx: &mut Context<Self>) {
        cx.emit(BrowseSplitClose);
    }

    pub(crate) fn open_split_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.open_path(path, cx);
    }

    /// Shows a book's page over the grid, reusing the kept one when it is for
    /// the same book.
    pub(crate) fn show_book_detail(&mut self, book: library_model::BookCardRow, cover: Option<LoadedCover>, cx: &mut Context<Self>) {
        let detail = self.detail.cached().filter(|detail| detail.read(cx).content_hash() == book.content_hash).cloned().unwrap_or_else(|| {
            let detail = BookDetailPage::create(book, cover, self.ui.clone(), cx);
            self._detail_subscription = Some(cx.subscribe(&detail, |page, source, _: &CloseBookDetail, cx| {
                if page.detail.visible() == Some(&source) {
                    page.close_book_detail(cx);
                }
            }));
            detail
        });
        self.detail = DetailNavigation::Book(detail);
        cx.notify();
    }

    /// Opens a book's page over the folder at `path`, for a book opened from
    /// outside the grid. A grid already showing that folder is left as it is.
    pub(crate) fn open_book_detail(&mut self, book: library_model::BookCardRow, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        if path != self.destination.path {
            self.open_path(path, cx);
        }
        self.show_book_detail(book, None, cx);
    }

    /// Closes the book's page, back to the grid underneath. Returns whether one
    /// was open.
    pub(crate) fn close_book_detail(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.detail.close() {
            return false;
        }
        self.ui.notify_book_detail_closed(cx);
        self.restore_focus = true;
        cx.notify();
        true
    }

    pub(crate) fn clear_cached_book_detail(&mut self, cx: &mut Context<Self>) {
        self.restore_focus |= self.detail.visible().is_some();
        self.detail = DetailNavigation::default();
        self._detail_subscription = None;
        cx.notify();
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let listing = match self.view {
            BrowseView::Cards => self.paginator.clone().into_any_element(),
            BrowseView::Graph => {
                let controls = self.render_topbar(window, cx);
                let search = self.search.read(cx).render();
                let count = self.ui.updates().read(cx).book_count().unwrap_or(0);
                self.graph.update(cx, |graph, cx| graph.render_panel(self.kind, &self.destination, count, controls, search, !self.listing.books().is_empty(), window, cx))
            }
        };
        let paginator = self.paginator.read(cx);
        let scanning_empty_library = paginator.is_empty() && self.ui.updates().read(cx).scanning();
        let waiting_for_library = paginator.is_empty() && self.ui.updates().read(cx).loading();
        // Only the states that are genuinely a wait: a library still opening,
        // and a scan that has not yet found anything. Neither is a browse
        // request, and there is nothing painted underneath either to keep.
        let show_loading = (scanning_empty_library || waiting_for_library) && self.error.is_none();
        let loading_label = if scanning_empty_library { "Scanning library…" } else { "Loading…" };
        let mut body = div().relative().size_full().min_h_0().min_w_0();
        // Open space is the folder being browsed, and takes drops into it.
        if let BrowseLocation::Folder(destination) = self.destination.location {
            // Not outlined: it holds every other target, and the one under
            // the pointer is the one to show.
            let name = self.destination.path.last().map_or_else(|| self.kind.root_label(), |crumb| crumb.label.clone().into());
            body = folder_drop_target(body, destination, name, false, page.downgrade(), theme);
        }
        let body = body
            .child(listing)
            .when(show_loading, |body| {
                body.child(components::absolute_full_size().debug_selector(|| "browse-loading-overlay".into()).bg(theme.page_bg).flex().items_center().justify_center().child(components::browser_loading_message(loading_label, theme)))
            })
            .into_any_element();
        if let Some(error) = self.error.clone() {
            let retry_page = page.clone();
            let retry = components::topbar_action_button("retry-section", "Retry", IconName::Redo2, theme).on_click(move |_, _, cx| retry_page.update(cx, |page, cx| page.reload(cx)));
            components::browser_error_banner(error, retry, body).into_any_element()
        } else {
            body
        }
    }

    fn render_shell(&self, topbar: AnyElement, body: AnyElement, options_sheet: Option<AnyElement>, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let folder_overlay = self.folder_actions.as_ref().and_then(|actions| FolderActions::render_overlay(actions.clone(), cx));
        let chip_menu = self.chip_menu.as_ref().map(|active| mobile_context_menu::render(active.menu.clone(), active.position, window));
        components::full_size_column()
            .track_focus(&self.focus)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|page, _, window, cx| {
                    if !page.focus.contains_focused(window, cx) {
                        page.focus.focus(window, cx);
                    }
                }),
            )
            .on_action(cx.listener(Self::on_dismiss))
            .on_action(cx.listener(Self::on_open_context_menu))
            .on_action(cx.listener(Self::on_undo))
            .on_key_down(cx.listener(Self::navigate_paginator))
            .child(topbar)
            .child(components::remaining_space_column().child(body))
            .children(chip_menu)
            .children(folder_overlay)
            .children(options_sheet)
    }
}

impl Render for BrowsePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.restore_focus) {
            self.focus.focus(window, cx);
        }
        if let Some(detail) = self.detail.visible().cloned() {
            let theme = components::browser_theme(cx);
            let page = cx.entity();
            let close = components::outlined_icon_button("close-book-detail", "Close", IconName::Close, theme).on_click(move |_, _, cx| {
                page.update(cx, |page, cx| page.close_book_detail(cx));
            });
            let body = div().relative().size_full().child(detail).child(div().absolute().top(px(12.0)).right(px(12.0)).child(close)).into_any_element();
            self.render_shell(div().h(px(0.0)).flex_none().into_any_element(), body, None, window, cx)
        } else {
            let topbar = if self.view == BrowseView::Graph { div().h(px(0.0)).flex_none().into_any_element() } else { self.render_topbar(window, cx) };
            let body = self.render_body(window, cx);
            let options_sheet = self.render_browse_options_sheet(cx);
            self.render_shell(topbar, body, options_sheet, window, cx)
        }
    }
}

impl gpui::Focusable for BrowsePage {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::tests as fixtures;
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn detail_close_events_keep_the_cache_and_do_not_close_another_book(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let mut entity = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let page = fixtures::page(BrowseKind::Folder, cx);
            page.update(cx, |page, cx| page.show_book_detail(fixtures::book(), None, cx));
            entity = Some(page.clone());
            gpui_component::Root::new(page, window, cx)
        });
        window.run_until_parked();
        let page = entity.unwrap();
        let first = window.update(|_, cx| page.read(cx).detail.visible().unwrap().clone());
        window.update(|_, cx| first.update(cx, |_, cx| cx.emit(CloseBookDetail)));
        window.run_until_parked();
        window.update(|_, cx| {
            page.update(cx, |page, cx| {
                assert!(page.detail.visible().is_none());
                assert_eq!(page.detail.cached(), Some(&first));
                page.show_book_detail(fixtures::book(), None, cx);
                assert_eq!(page.detail.visible(), Some(&first));
                let mut other = fixtures::book();
                other.content_hash = sync_common::ContentHash::new(&"c".repeat(64));
                page.show_book_detail(other, None, cx);
            })
        });
        window.run_until_parked();
        window.update(|_, cx| first.update(cx, |_, cx| cx.emit(CloseBookDetail)));
        window.run_until_parked();
        let current = window.update(|_, cx| {
            let current = page.read(cx).detail.visible().expect("an old detail cannot close the current one").clone();
            assert_ne!(current, first);
            current
        });
        window.update(|_, cx| current.update(cx, |_, cx| cx.emit(CloseBookDetail)));
        window.run_until_parked();
        window.update(|_, cx| {
            page.update(cx, |page, cx| {
                assert!(page.detail.visible().is_none());
                assert_eq!(page.detail.cached(), Some(&current));
                page.clear_cached_book_detail(cx);
                assert!(page.detail.cached().is_none());
                assert!(page._detail_subscription.is_none());
            })
        });
    }

    #[gpui::test]
    fn scanning_overlay_renders_on_empty_subject_and_folder_pages(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        for kind in [BrowseKind::Folder, BrowseKind::Subject] {
            let (_, window) = cx.add_window_view(|window, cx| {
                let page = fixtures::page(kind, cx);
                page.update(cx, |page, cx| {
                    page.request.begin();
                    page.ui.updates().update(cx, |updates, cx| updates.set_importing(true, cx));
                });
                gpui_component::Root::new(page, window, cx)
            });
            window.run_until_parked();
            assert!(window.debug_bounds("browse-loading-overlay").is_some());
        }
    }

    #[gpui::test]
    fn populated_folder_and_subject_pages_switch_between_cards_and_graph(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        for kind in [BrowseKind::Folder, BrowseKind::Subject] {
            let mut entity = None;
            let splits = Rc::new(std::cell::RefCell::new(Vec::new()));
            let mut split_subscription = None;
            let (_, window) = cx.add_window_view(|window, cx| {
                let page = fixtures::page(kind, cx);
                page.update(cx, |page, cx| {
                    let (parent, child) = match kind {
                        BrowseKind::Folder => (BrowseLocation::Folder(DirId::from_u128(1)), BrowseLocation::Folder(DirId::from_u128(2))),
                        BrowseKind::Subject => (BrowseLocation::Subject(1), BrowseLocation::Subject(2)),
                    };
                    let mut section_book = fixtures::book();
                    section_book.content_hash = sync_common::ContentHash::new(&"b".repeat(64));
                    let response = BrowseResponse {
                        searched: false,
                        data: app::LibraryBrowseData {
                            contents: library_model::BrowseContents { children: vec![fixtures::row(parent)], books: vec![fixtures::book()] },
                            sections: vec![library_model::BrowseSection { parent: fixtures::row(parent), children: vec![fixtures::row(child)], books: vec![section_book] }],
                            path: Vec::new(),
                            language_counts: Vec::new(),
                            format_counts: Vec::new(),
                        },
                    };
                    let generation = page.request.begin();
                    page.finish_request(generation, page.destination.clone(), false, Ok(response), cx);
                    assert_eq!(page.section_cards.len(), 1);
                    assert_eq!(page.listing.books().len(), 1);
                });
                let requests = splits.clone();
                split_subscription = Some(page.update(cx, |_, cx| cx.subscribe(&cx.entity(), move |_, _, request: &BrowseSplitRequest, _| requests.borrow_mut().push(request.clone()))));
                entity = Some(page.clone());
                gpui_component::Root::new(page, window, cx)
            });
            window.run_until_parked();
            let page = entity.unwrap();
            let (selector, download_selector) = match kind {
                BrowseKind::Folder => ("browse-section-subject-folder:00000000-0000-0000-0000-000000000002", "chip-download-folder:00000000-0000-0000-0000-000000000002"),
                BrowseKind::Subject => ("browse-section-subject-subject:2", "chip-download-subject:2"),
            };
            let row = window.debug_bounds(selector).expect("section child is visible");
            assert!(window.debug_bounds(download_selector).is_some(), "partly downloaded section children have a download mark");
            window.simulate_click(row.center(), gpui::Modifiers { control: true, ..Default::default() });
            assert_eq!(splits.borrow().len(), 1);
            assert_eq!(splits.borrow()[0].side, BrowseSplitSide::Left);
            window.simulate_mouse_down(row.center(), gpui::MouseButton::Right, gpui::Modifiers { control: true, ..Default::default() });
            window.simulate_mouse_up(row.center(), gpui::MouseButton::Right, gpui::Modifiers { control: true, ..Default::default() });
            window.run_until_parked();
            assert_eq!(splits.borrow().len(), 2);
            assert_eq!(splits.borrow()[1].side, BrowseSplitSide::Right);
            window.simulate_mouse_down(row.center(), gpui::MouseButton::Right, gpui::Modifiers::default());
            window.simulate_mouse_up(row.center(), gpui::MouseButton::Right, gpui::Modifiers::default());
            window.run_until_parked();
            window.update(|_, cx| {
                page.update(cx, |page, cx| {
                    assert!(page.chip_menu.is_some(), "section children open the common context menu");
                    page.dismiss_menu(cx);
                })
            });
            window.update(|_, cx| page.update(cx, |page, cx| page.set_view(BrowseView::Graph, cx)));
            window.run_until_parked();
            window.update(|_, cx| {
                page.update(cx, |page, cx| {
                    page.graph.update(cx, |graph, cx| {
                        graph.handle_key("l", cx);
                    });
                    page.set_view(BrowseView::Cards, cx);
                    let card = page.section_cards.values().next().unwrap().clone();
                    card.update(cx, |card, cx| {
                        assert!(card.enter(CursorMove::Down, cx));
                        assert!(card.move_cursor(CursorMove::Right, cx));
                        assert!(card.move_cursor(CursorMove::Left, cx));
                        card.leave(cx);
                    });
                })
            });
            window.run_until_parked();
        }
    }
}

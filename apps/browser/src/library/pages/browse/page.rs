//! Shared browse-page state and layout for folders and subjects.

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, Context, DismissEvent, Entity, FocusHandle, Focusable as _, IntoElement, KeyDownEvent, Pixels, Point, Render, SharedString, Subscription, Task, Window, anchored, deferred, div, px, rems};
use gpui_component::IconName;
use gpui_component::breadcrumb::BreadcrumbItem;
use gpui_component::menu::PopupMenu;
use library_model::BrowseChipSort;
use sync_common::DirId;
use ui_components as components;

use super::adapter::{BrowseAdapter, BrowseCrumb, BrowseQuery, BrowseRequest};
use super::controls::{BrowseBookOrder, BrowseFormatFilter, BrowseOptionsEvent, BrowseOptionsSelection, SearchField, browse_options_control};
use super::folder_actions::{FolderActions, FolderActionsEvent, FolderPlacement, FolderTargets};
use crate::library::services::{LatestRequest, LibraryContentsChanged, LibraryDownloadChanged, LibraryScanChanged, LibraryUpdateBridge};
use crate::library::widgets::{CursorMove, Paginator, PaginatorAxis, PaginatorContextMenu, PaginatorDropTargetChanged, PaginatorGroup, PaginatorMarqueeChanged, PaginatorSelectionChanged};
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

#[derive(Clone)]
struct BrowseSelection {
    search: String,
    formats: Vec<BrowseFormatFilter>,
    chip_sort: Option<BrowseChipSort>,
    book_sort: Option<BrowseBookOrder>,
    languages: Vec<&'static str>,
    hide_finished: bool,
    include_direct_child_books: bool,
}

pub(super) struct BrowseSelectionState {
    pub(super) indices: BTreeSet<usize>,
}

impl BrowseSelection {
    fn new<A: BrowseAdapter>(adapter: &A) -> Self {
        Self {
            search: String::new(),
            formats: Vec::new(),
            chip_sort: adapter.controls().collection_sorts.first().copied(),
            book_sort: adapter.controls().book_sorts.first().copied(),
            languages: Vec::new(),
            hide_finished: false,
            include_direct_child_books: false,
        }
    }

    fn query(&self, location: String) -> BrowseQuery {
        BrowseQuery {
            location,
            search: self.search.clone(),
            formats: self.formats.clone(),
            chip_sort: self.chip_sort,
            book_sort: self.book_sort,
            languages: self.languages.clone(),
            hide_finished: self.hide_finished,
            include_direct_child_books: self.include_direct_child_books,
        }
    }

    fn options(&self) -> BrowseOptionsSelection {
        BrowseOptionsSelection::new(self.chip_sort, self.book_sort, self.formats.clone(), self.languages.clone(), self.hide_finished, self.include_direct_child_books)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadIntent {
    Initial,
    Reload,
    Navigate,
    Refresh,
}

impl LoadIntent {
    fn reveals_end(self) -> bool {
        false
    }
}

#[derive(Clone)]
struct PendingLoad {
    location: String,
    path: Vec<BrowseCrumb>,
    intent: LoadIntent,
}

pub(super) struct BrowsePageConfig {
    root_location: String,
    root_label: SharedString,
    list_prefix: &'static str,
    changes: Option<Entity<LibraryUpdateBridge>>,
    folder_actions: Option<Entity<FolderActions>>,
    split_enabled: bool,
}

impl BrowsePageConfig {
    pub(super) fn new(root_location: impl Into<String>, root_label: impl Into<SharedString>, list_prefix: &'static str) -> Self {
        Self { root_location: root_location.into(), root_label: root_label.into(), list_prefix, changes: None, folder_actions: None, split_enabled: false }
    }

    pub(super) fn with_changes(mut self, changes: Entity<LibraryUpdateBridge>) -> Self {
        self.changes = Some(changes);
        self
    }

    pub(super) fn with_folder_actions(mut self, folder_actions: Entity<FolderActions>) -> Self {
        self.folder_actions = Some(folder_actions);
        self
    }

    pub(super) fn with_split(mut self) -> Self {
        self.split_enabled = true;
        self
    }
}

/// What the cursor was on when a location was left.
///
/// A book is remembered by hash rather than by index, because the folder can be
/// reordered by a rescan while the reader is off looking at one of its books,
/// and an index would then point at a different book — confidently.
enum SavedCursor {
    Chip(String),
    Book(sync_common::ContentHash),
}

struct SavedChipSelection {
    cursor: SavedCursor,
    child_anchor: usize,
}

/// One open chip context menu. Chips are render closures, so the menu they open
/// lives on the page rather than on the chip, and only one is open at a time.
struct ChipContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

pub(crate) struct BrowsePage<A: BrowseAdapter> {
    root_location: String,
    root_label: SharedString,
    location: String,
    path: Vec<BrowseCrumb>,
    list_prefix: &'static str,
    adapter: A,
    paginator: Entity<Paginator>,
    /// The paginator's cursor, mirrored where the chip closures can read it.
    /// Book cards are told directly; chips are closures and have nowhere to be
    /// told, so they read this instead.
    cursor: Rc<Cell<Option<usize>>>,
    selected_chips: HashMap<String, SavedChipSelection>,
    /// The paginator's rubber-band selection, mirrored where the chip closures
    /// can read it — chips have nowhere to be told, exactly as with the cursor.
    selected: Entity<BrowseSelectionState>,
    /// The chip a drag is over, mirrored where the chip closures can read it.
    drop_target: Rc<Cell<Option<usize>>>,
    /// Whether a rubber-band drag is sweeping, mirrored for the same reason.
    /// Children drop their hover style while it is, so the band reports what it
    /// has picked up instead of every child reporting the pointer's path.
    marquee: Rc<Cell<bool>>,
    chip_menu: Option<ChipContextMenu>,
    focus: FocusHandle,
    search_field: Option<SearchField>,
    search_open: bool,
    /// Which of the two card modes the grid is in. A view choice rather than a
    /// query one, so it lives beside the search box instead of in the sort and
    /// filter menu, and changing it never reloads.
    detailed_cards: bool,
    selection: BrowseSelection,
    loaded: bool,
    error: Option<SharedString>,
    request: LatestRequest,
    search_debounce: Option<Task<()>>,
    folder_actions: Option<Entity<FolderActions>>,
    _changes: Option<Subscription>,
    _download_changes: Option<Subscription>,
    _scan_changes: Option<Subscription>,
    _search_subscription: Option<Subscription>,
    _folder_actions_subscription: Option<Subscription>,
    _paginator_selection: Option<Subscription>,
    _paginator_drop_target: Option<Subscription>,
    _paginator_marquee: Option<Subscription>,
    _paginator_menu: Option<Subscription>,
    split_enabled: bool,
    split_close_enabled: bool,
}

impl<A: BrowseAdapter> gpui::EventEmitter<BrowseSplitRequest> for BrowsePage<A> {}
impl<A: BrowseAdapter> gpui::EventEmitter<BrowseSplitClose> for BrowsePage<A> {}

impl<A: BrowseAdapter> BrowsePage<A> {
    pub(super) fn adapter_mut(&mut self) -> &mut A {
        &mut self.adapter
    }

    pub(super) fn new(adapter: A, config: BrowsePageConfig, cx: &mut Context<Self>) -> Self {
        let location = config.root_location.clone();
        let selection = BrowseSelection::new(&adapter);
        let selectable = config.folder_actions.is_some();
        let paginator = cx.new(|_| {
            let paginator = Paginator::new(format!("{}-browse-paginator", config.list_prefix), []).flow_axis(PaginatorAxis::Vertical);
            if selectable { paginator.selectable() } else { paginator }
        });
        let mut page = Self {
            root_location: config.root_location,
            root_label: config.root_label,
            location,
            path: Vec::new(),
            list_prefix: config.list_prefix,
            adapter,
            paginator,
            cursor: Rc::new(Cell::new(None)),
            selected_chips: HashMap::new(),
            selected: cx.new(|_| BrowseSelectionState { indices: BTreeSet::new() }),
            drop_target: Rc::new(Cell::new(None)),
            marquee: Rc::new(Cell::new(false)),
            chip_menu: None,
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            search_field: None,
            search_open: false,
            detailed_cards: false,
            selection,
            loaded: false,
            error: None,
            request: LatestRequest::default(),
            search_debounce: None,
            folder_actions: config.folder_actions,
            _changes: None,
            _download_changes: None,
            _scan_changes: None,
            _search_subscription: None,
            _folder_actions_subscription: None,
            _paginator_selection: None,
            _paginator_drop_target: None,
            _paginator_marquee: None,
            _paginator_menu: None,
            split_enabled: config.split_enabled,
            split_close_enabled: false,
        };
        if selectable {
            page._paginator_selection = Some(cx.subscribe(&page.paginator.clone(), |page, _, _: &PaginatorSelectionChanged, cx| page.sync_selection(cx)));
            page._paginator_drop_target = Some(cx.subscribe(&page.paginator.clone(), |page, paginator, _: &PaginatorDropTargetChanged, cx| {
                page.drop_target.set(paginator.read(cx).drop_target());
                cx.notify();
            }));
            page._paginator_marquee = Some(cx.subscribe(&page.paginator.clone(), |page, paginator, _: &PaginatorMarqueeChanged, cx| {
                let marquee = paginator.read(cx).marquee_active();
                page.marquee.set(marquee);
                // Book cards are entities and are told; chips read the cell.
                for book in page.adapter.books().iter() {
                    A::set_book_marquee(book, marquee, cx);
                }
                cx.notify();
            }));
        }
        page.load(page.path.clone(), LoadIntent::Initial, cx);
        if let Some(changes) = config.changes {
            page._download_changes = Some(cx.subscribe(&changes, |page, _, event: &LibraryDownloadChanged, cx| {
                if matches!(event.status.state, bokheim_backend::DownloadState::Downloaded | bokheim_backend::DownloadState::NotDownloaded) {
                    page.refresh_contents(cx);
                }
            }));
            page._changes = Some(cx.subscribe(&changes, |page, _, _: &LibraryContentsChanged, cx| page.refresh_contents(cx)));
            page._scan_changes = Some(cx.subscribe(&changes, |_, _, _: &LibraryScanChanged, cx| cx.notify()));
        }
        page._folder_actions_subscription = page.folder_actions.clone().map(|actions| cx.subscribe(&actions, |page, _, event, cx| page.handle_folder_action(event.clone(), cx)));
        page
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(self.path.clone(), LoadIntent::Reload, cx);
    }

    pub(super) fn download_child(&mut self, location: String, cx: &mut Context<Self>) {
        if !self.adapter.children().iter().any(|child| child.id == location && A::can_download_child(child)) {
            return;
        }
        let mut query = self.selection.query(location);
        query.search.clear();
        let Some(operation) = self.adapter.download_child(&query) else {
            return;
        };
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update_in(cx, |page, window, cx| {
                if let Err(error) = result {
                    crate::services::notify_error("hierarchy-download-error", format!("Could not queue downloads: {error}"), window, cx);
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
        if !self.adapter.refresh_on_library_changes() {
            return;
        }
        self.load(self.path.clone(), LoadIntent::Refresh, cx);
    }

    fn handle_folder_action(&mut self, event: FolderActionsEvent, cx: &mut Context<Self>) {
        match event {
            FolderActionsEvent::Reload => self.reload(cx),
            FolderActionsEvent::Renamed(name) => {
                if let Some(crumb) = self.path.last_mut() {
                    crumb.label = name;
                }
                self.reload(cx);
            }
            FolderActionsEvent::OpenRoot => self.open_path(Vec::new(), cx),
        }
    }

    /// What a menu opened over `child_index` acts on: the whole selection when
    /// that child is part of one, and otherwise just that child.
    fn menu_targets(&self, child_index: usize, cx: &App) -> Option<FolderTargets> {
        let source = DirId::parse_str(&self.location).ok()?;
        let selection = self.paginator.read(cx).selection();
        let indices: Vec<usize> = if selection.contains(&child_index) { selection.iter().copied().collect() } else { vec![child_index] };
        let chips = self.adapter.children();
        let books = self.adapter.books();
        let mut targets = FolderTargets { folders: Vec::new(), folder_parents: Default::default(), books: Vec::new(), source };
        for index in indices {
            match chips.get(index) {
                Some(chip) => {
                    // A chip that is not a directory — a subject, an author —
                    // has no placement to change, so it is quietly left out.
                    if let Ok(id) = DirId::parse_str(&chip.id) {
                        targets.folders.push(id);
                        if let Some(parent) = self.adapter.chip_groups().iter().find(|group| (group.start..group.end).contains(&index)).and_then(|group| DirId::parse_str(&group.parent.id).ok()) {
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

    /// One line describing what a discard is about to do, for its confirmation.
    fn trash_description(&self, targets: &FolderTargets) -> String {
        let folders = targets.folders.len();
        let books = targets.books.len();
        let mut what = Vec::new();
        if folders > 0 {
            what.push(if folders == 1 { "1 folder".to_owned() } else { format!("{folders} folders") });
        }
        if books > 0 {
            what.push(if books == 1 { "1 book".to_owned() } else { format!("{books} books") });
        }
        let mut description = format!("{} will be moved to Trash.", what.join(" and "));
        if folders > 0 {
            description.push_str(" A folder goes with everything inside it.");
        }
        if books > 0 {
            description.push_str(" A book leaves this folder, and reaches Trash only if this was the last folder holding it.");
        }
        description
    }

    /// Opens the folder commands over a child, or over the selection it belongs
    /// to. Only folder listings have these: an author or subject chip is a
    /// query, not a directory, so adapters without folder actions get no menu.
    pub(super) fn open_items_menu(&mut self, child_index: usize, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(actions) = self.folder_actions.clone() else { return };
        let Some(targets) = self.menu_targets(child_index, cx) else { return };
        // Renaming is a single-folder command: there is one name to type.
        let rename = (targets.folders.len() == 1 && targets.books.is_empty()).then(|| {
            let id = targets.folders[0];
            let name = self.adapter.children().iter().find(|chip| DirId::parse_str(&chip.id).ok() == Some(id)).map(|chip| chip.name.clone()).unwrap_or_default();
            (id, name)
        });
        let description = self.trash_description(&targets);
        let window_targets = self
            .adapter
            .library_context()
            .map(|ui| {
                let library_id = *ui.backend().id();
                let mut requests = targets
                    .folders
                    .iter()
                    .map(|id| {
                        let title = self.adapter.children().iter().find(|chip| DirId::parse_str(&chip.id).ok() == Some(*id)).map(|chip| chip.name.clone()).unwrap_or_default();
                        crate::NewWindowTarget::Folder { library_id, directory_id: *id, title }
                    })
                    .collect::<Vec<_>>();
                requests.extend(targets.books.iter().map(|hash| crate::NewWindowTarget::Book {
                    locator: library_model::BookLocator::new(library_id, *hash),
                    title: self.adapter.books().iter().find(|book| A::book_content_hash(book, cx) == Some(*hash)).map(|book| A::book_title(book, cx)).unwrap_or_default(),
                }));
                requests
            })
            .unwrap_or_default();
        let copy_actions = actions.clone();
        let move_actions = actions.clone();
        let trash_actions = actions.clone();
        let copy_targets = targets.clone();
        let move_targets = targets.clone();
        let trash_targets = targets;
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            if crate::native_windows_available() && !window_targets.is_empty() {
                let targets = window_targets.clone();
                menu = menu
                    .item(components::menu_item("Open in New Window", move |_, window, cx| {
                        for target in &targets {
                            window.dispatch_action(Box::new(crate::OpenInNewWindow(target.clone())), cx);
                        }
                    }))
                    .separator();
            }
            if let Some((id, name)) = rename.clone() {
                let rename_actions = actions.clone();
                menu = menu.item(components::menu_item_with_icon("Rename…", IconName::Replace, move |_, window, cx| {
                    rename_actions.update(cx, |actions, cx| actions.open_rename(id, name.clone(), window, cx));
                }));
            }
            let copy_actions = copy_actions.clone();
            let move_actions = move_actions.clone();
            let trash_actions = trash_actions.clone();
            let copy_targets = copy_targets.clone();
            let move_targets = move_targets.clone();
            let trash_targets = trash_targets.clone();
            let description = description.clone();
            menu.item(components::menu_item_with_icon("Copy to folder…", IconName::Copy, move |_, _, cx| {
                let targets = copy_targets.clone();
                copy_actions.update(cx, |actions, cx| actions.open_copy_targets(targets, cx));
            }))
            .item(components::menu_item_with_icon("Move to folder…", components::navigation_icon(components::NavigationIcon::Folder), move |_, _, cx| {
                let targets = move_targets.clone();
                move_actions.update(cx, |actions, cx| actions.open_move_targets(targets, cx));
            }))
            .separator()
            .item(components::menu_item_with_icon("Move to Trash", components::navigation_icon(components::NavigationIcon::Trash2), move |_, window, cx| {
                let targets = trash_targets.clone();
                let description = description.clone();
                trash_actions.update(cx, |actions, cx| actions.trash_targets(targets, description, window, cx));
            }))
        });
        self.show_menu(menu, position, window, cx);
    }

    fn show_menu(&mut self, menu: Entity<PopupMenu>, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.subscribe_in(&menu, window, |page, _, _: &DismissEvent, window, cx| {
            page.chip_menu = None;
            // The page had the keyboard before the menu took it, and a menu
            // dismissed by Escape must not leave the arrows dead.
            window.focus(&page.focus, cx);
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        self.chip_menu = Some(ChipContextMenu { menu, position, _subscription: subscription });
        cx.notify();
    }

    /// Imports paths dropped from the desktop into `destination`, or into the
    /// folder being browsed when the drop lands on open space.
    pub(super) fn import_dropped_paths(&mut self, destination: Option<DirId>, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(ui) = self.adapter.library_context().cloned() else { return };
        let Some(parent) = destination.or_else(|| DirId::parse_str(&self.location).ok()) else { return };
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
        let name = self.adapter.children().iter().find(|chip| DirId::parse_str(&chip.id).ok() == Some(destination)).map(|chip| chip.name.clone()).unwrap_or_else(|| "this folder".to_owned());
        let move_actions = actions.clone();
        let copy_actions = actions;
        let move_targets = targets.clone();
        let copy_targets = targets;
        let position = window.mouse_position();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            let move_actions = move_actions.clone();
            let copy_actions = copy_actions.clone();
            let move_targets = move_targets.clone();
            let copy_targets = copy_targets.clone();
            menu.item(components::menu_item_with_icon(format!("Move to {name}"), components::navigation_icon(components::NavigationIcon::Folder), move |_, _, cx| {
                let targets = move_targets.clone();
                move_actions.update(cx, |actions, cx| actions.place_targets(FolderPlacement::Move, targets, destination, cx));
            }))
            .item(components::menu_item_with_icon(format!("Copy to {name}"), IconName::Copy, move |_, _, cx| {
                let targets = copy_targets.clone();
                copy_actions.update(cx, |actions, cx| actions.place_targets(FolderPlacement::Copy, targets, destination, cx));
            }))
        });
        self.show_menu(menu, position, window, cx);
        // What was dragged is about to land elsewhere; the indices it was
        // selected under will not survive the reload either way.
        self.paginator.update(cx, |paginator, cx| paginator.clear_selection(cx));
    }

    /// Opens the folder's own commands where the listing has no item — the
    /// same menu the toolbar's Add button drops down, at the pointer.
    pub(super) fn open_folder_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(actions) = self.folder_actions.clone() else { return };
        let has_current_folder = actions.read(cx).has_current_folder();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| FolderActions::add_menu_items(actions.clone(), menu, has_current_folder));
        self.show_menu(menu, position, window, cx);
    }

    /// Opens the same menu from the keyboard, anchored under the cursor's own    /// Opens the same menu from the keyboard, anchored under the cursor's own
    /// child rather than under a pointer.
    fn open_cursor_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let paginator = self.paginator.read(cx);
        let Some(cursor) = paginator.cursor() else { return false };
        let Some(bounds) = paginator.child_bounds(cursor) else { return false };
        let position = gpui::point(bounds.left(), bounds.bottom());
        self.open_items_menu(cursor, position, window, cx);
        self.chip_menu.is_some()
    }

    /// Subscribes to the paginator's menu requests. Deferred to the first
    /// render because opening a menu needs a window, and the page is built
    /// without one.
    fn watch_for_menu_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._paginator_menu.is_some() || self.folder_actions.is_none() {
            return;
        }
        self._paginator_menu = Some(cx.subscribe_in(&self.paginator.clone(), window, |page, _, event: &PaginatorContextMenu, window, cx| match event.child_index {
            Some(child_index) if window.modifiers().control && page.can_split(window) && child_index < page.adapter.children().len() => {}
            Some(child_index) => page.open_items_menu(child_index, event.position, window, cx),
            None => page.open_folder_menu(event.position, window, cx),
        }));
    }

    /// Mirrors the paginator's selection out to the things that draw it, the
    /// way [`Self::sync_cursor`] mirrors the cursor.
    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.paginator.read(cx).selection().clone();
        let chip_count = self.adapter.children().len();
        self.selected.update(cx, |state, _| state.indices = selection.clone());
        for (index, book) in self.adapter.books().iter().enumerate() {
            A::set_book_selected(book, selection.contains(&(index + chip_count)), cx);
        }
        cx.notify();
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        let Some(search_field) = &self.search_field else {
            return;
        };
        let search = search_field.query(cx);
        if self.selection.search == search {
            return;
        }
        self.selection.search = search;
        self.search_debounce.take();
        if !self.selection.search.is_empty() && self.selection.search.chars().count() < A::minimum_search_length() {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_millis(250));
        self.search_debounce = Some(cx.spawn(async move |page, cx| {
            timer.await;
            let _ = page.update(cx, |page, cx| {
                page.search_debounce.take();
                page.reload(cx);
            });
        }));
    }

    fn set_format(&mut self, format: BrowseFormatFilter, cx: &mut Context<Self>) {
        if format != BrowseFormatFilter::Any && !self.adapter.controls().formats.contains(&format) {
            return;
        }
        if format == BrowseFormatFilter::Any {
            if self.selection.formats.is_empty() {
                return;
            }
            self.selection.formats.clear();
        } else if let Some(index) = self.selection.formats.iter().position(|selected| *selected == format) {
            self.selection.formats.remove(index);
        } else {
            self.selection.formats.push(format);
        }
        self.reload(cx);
    }

    fn set_chip_sort(&mut self, chip_sort: BrowseChipSort, cx: &mut Context<Self>) {
        if !self.adapter.controls().collection_sorts.contains(&chip_sort) || self.selection.chip_sort == Some(chip_sort) {
            return;
        }
        self.selection.chip_sort = Some(chip_sort);
        self.reload(cx);
    }

    fn set_book_sort(&mut self, book_sort: BrowseBookOrder, cx: &mut Context<Self>) {
        if !self.adapter.controls().book_sorts.contains(&book_sort) || self.selection.book_sort == Some(book_sort) {
            return;
        }
        self.selection.book_sort = Some(book_sort);
        self.reload(cx);
    }

    fn set_language(&mut self, language: Option<&'static str>, cx: &mut Context<Self>) {
        if language.is_some_and(|language| !self.adapter.controls().languages.contains(&language)) {
            return;
        }
        match language {
            None => {
                if self.selection.languages.is_empty() {
                    return;
                }
                self.selection.languages.clear();
            }
            Some(language) => {
                if let Some(index) = self.selection.languages.iter().position(|selected| *selected == language) {
                    self.selection.languages.remove(index);
                } else {
                    self.selection.languages.push(language);
                }
            }
        }
        self.reload(cx);
    }

    fn select_option(&mut self, event: BrowseOptionsEvent, cx: &mut Context<Self>) {
        match event {
            BrowseOptionsEvent::SelectChipSort(sort) => self.set_chip_sort(sort, cx),
            BrowseOptionsEvent::SelectBookSort(sort) => self.set_book_sort(sort, cx),
            BrowseOptionsEvent::ToggleFormat(format) => self.set_format(format, cx),
            BrowseOptionsEvent::ToggleLanguage(language) => self.set_language(language, cx),
            BrowseOptionsEvent::ToggleHideFinished => self.toggle_hide_finished(cx),
            BrowseOptionsEvent::ToggleDirectChildBooks => {
                self.selection.include_direct_child_books = !self.selection.include_direct_child_books;
                self.reload(cx);
            }
        }
    }

    fn toggle_hide_finished(&mut self, cx: &mut Context<Self>) {
        self.selection.hide_finished = !self.selection.hide_finished;
        self.reload(cx);
    }

    fn ensure_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_field.is_some() {
            return;
        }
        let (search_field, subscription) = SearchField::new(Some("Search books or authors"), |page: &mut Self, cx| page.search_changed(cx), window, cx);
        self.search_field = Some(search_field);
        self._search_subscription = Some(subscription);
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_search_field(window, cx);
        self.search_open = true;
        if let Some(search) = &self.search_field {
            search.focus(window, cx);
        }
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    /// Leaves the current folder for the one above it. Already at the root is
    /// not a failure, but it is unhandled, so the key falls through.
    fn open_parent(&mut self, cx: &mut Context<Self>) -> bool {
        if self.path.is_empty() {
            return false;
        }
        let mut path = self.path.clone();
        path.pop();
        self.open_path(path, cx);
        true
    }

    fn move_cursor(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        if !self.paginator.update(cx, |paginator, cx| paginator.move_cursor(movement, cx)) {
            return false;
        }
        self.sync_cursor(cx);
        true
    }

    fn previous_page(&mut self, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        self.paginator.update(cx, |paginator, cx| paginator.previous_with_axis(axis, cx))
    }

    fn next_page(&mut self, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        self.paginator.update(cx, |paginator, cx| paginator.next_with_axis(axis, cx))
    }

    fn navigate_paginator(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open && event.keystroke.key == "escape" {
            self.close_search(window, cx);
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }

        let search_focused = self.search_field.as_ref().is_some_and(|search| search.is_focused(window, cx));
        // Arrows move the cursor; the page turns as a consequence of the cursor
        // walking off it. PageUp and PageDown remain the explicit whole-page
        // jump, and are the only way to page without moving the cursor.
        let handled = match event.keystroke.key.as_str() {
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
            "pageup" => self.previous_page(PaginatorAxis::Vertical, cx),
            "pagedown" => self.next_page(PaginatorAxis::Vertical, cx),
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
        let Some(ui) = self.adapter.library_context().cloned() else {
            cx.propagate();
            return;
        };
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
                    Err(error) => crate::services::notify_error("undo-error", format!("Could not undo {label}: {error}"), window, cx),
                }
            });
        })
        .detach();
    }

    fn on_dismiss(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        // A selection is the shallowest thing Escape can dismiss.
        if self.paginator.update(cx, |paginator, cx| paginator.clear_selection(cx)) {
            return;
        }
        if let Some(actions) = &self.folder_actions {
            if actions.read(cx).has_modal() {
                actions.update(cx, |actions, cx| actions.dismiss_modal(cx));
                return;
            }
        }
        if self.search_open {
            self.close_search(window, cx);
            return;
        }
        if self.path.is_empty() {
            cx.propagate();
            return;
        }

        let mut parent = self.path.clone();
        parent.pop();
        self.open_path(parent, cx);
    }

    pub(super) fn open_child(&mut self, child_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(child) = self.adapter.children().iter().chain(self.adapter.chip_groups().iter().map(|group| &group.parent)).find(|child| child.id == child_id).cloned() else {
            log::warn!("ignored missing {} child {child_id}", self.list_prefix);
            return;
        };
        let direct_path = || {
            let mut path = self.path.clone();
            if self.adapter.include_group_in_path()
                && let Some(group) = self.adapter.chip_groups().iter().find(|group| self.adapter.children()[group.start..group.end].iter().any(|row| row.id == child.id))
            {
                path.push(BrowseCrumb { location: group.parent.id.clone(), label: group.parent.name.clone() });
            }
            path.push(BrowseCrumb { location: child.id.clone(), label: child.name.clone() });
            path
        };
        let path = if self.selection.search.is_empty() {
            direct_path()
        } else {
            match self.adapter.path_to(&child) {
                Ok(Some(path)) => path,
                Ok(None) => direct_path(),
                Err(error) => {
                    log::warn!("failed to resolve full {} path for {}: {error}", self.list_prefix, child.id);
                    direct_path()
                }
            }
        };
        if !self.selection.search.is_empty() {
            self.selection.search.clear();
            if let Some(search) = &self.search_field {
                search.clear(window, cx);
            }
        }
        self.load(path, LoadIntent::Navigate, cx);
    }

    fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.load(path, LoadIntent::Navigate, cx);
    }

    /// Goes one step deeper into something that is not a place to browse — a
    /// single book — so the trail ends on it and every way back out is the one
    /// that already exists.
    pub(super) fn open_leaf(&mut self, crumb: BrowseCrumb, cx: &mut Context<Self>) {
        let mut path = self.path.clone();
        path.push(crumb);
        self.open_path(path, cx);
    }

    fn load(&mut self, path: Vec<BrowseCrumb>, intent: LoadIntent, cx: &mut Context<Self>) {
        let location = path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| self.root_location.clone());
        if location != self.location {
            if let Some(actions) = &self.folder_actions {
                actions.update(cx, |actions, cx| actions.cancel_move_selection(cx));
            }
        }
        let query = self.selection.query(location.clone());
        #[cfg(target_os = "android")]
        log::info!("Browse request: location={} formats={:?} languages={:?} search_length={}", location, query.formats, query.languages, query.search.len());
        let pending = PendingLoad { location, path, intent };
        let generation = self.request.begin();
        match self.adapter.request(&query) {
            BrowseRequest::Ready(response) => self.finish_request(generation, pending, Ok(response), cx),
            // Navigation keeps the current frame painted until the next one is
            // ready. A browse query is a local read that returns inside a
            // frame, so covering the grid in the meantime only ever produced a
            // flash — one that appeared going back up the tree and not going
            // down it, for the same query against the same database.
            BrowseRequest::Async(operation) => {
                self.error = None;
                self.request.install(cx.spawn(async move |page, cx| {
                    let result = operation.await;
                    let _ = page.update(cx, |page, cx| page.finish_request(generation, pending, result, cx));
                }));
            }
        }
    }

    fn finish_request(&mut self, generation: u64, pending: PendingLoad, result: Result<A::Response, String>, cx: &mut Context<Self>) {
        if !self.request.is_current(generation) {
            return;
        }
        self.loaded = true;
        match result {
            Ok(response) => {
                let changing_location = self.location != pending.location;
                if changing_location {
                    let (cursor, child_anchor) = {
                        let paginator = self.paginator.read(cx);
                        (paginator.cursor(), paginator.child_anchor())
                    };
                    // Books are declared after chips, so a cursor past the last
                    // chip is on a book — which is the case that matters when
                    // the destination being left for is that book's own page.
                    let children = self.adapter.children();
                    let saved = cursor.and_then(|index| match children.get(index) {
                        Some(chip) => Some(SavedCursor::Chip(chip.id.clone())),
                        None => {
                            let book = self.adapter.books();
                            let book = book.get(index.checked_sub(children.len())?)?;
                            A::book_content_hash(book, cx).map(SavedCursor::Book)
                        }
                    });
                    match saved {
                        Some(cursor) => {
                            self.selected_chips.insert(self.location.clone(), SavedChipSelection { cursor, child_anchor });
                        }
                        None => {
                            self.selected_chips.remove(&self.location);
                        }
                    }
                }
                let authoritative_path = self.adapter.install(response, cx);
                self.location = pending.location;
                self.path = authoritative_path.unwrap_or(pending.path);
                self.error = None;
                // Only where a folder command could act. A location that is not
                // a folder — a book's own page — leaves these pointed at the
                // folder it was opened from, which is where adding or importing
                // still belongs. Telling them otherwise was rejected anyway, and
                // logged on every refresh.
                if let Some(actions) = &self.folder_actions
                    && DirId::parse_str(&self.location).is_ok()
                {
                    let name = self.path.last().map(|crumb| crumb.label.clone());
                    actions.update(cx, |actions, cx| actions.set_location(&self.location, name, cx));
                }
                #[cfg(target_os = "android")]
                log::info!("Browse loaded: location={} children={} books={}", self.location, self.adapter.children().len(), self.adapter.books().len());
                self.sync_card_mode(cx);
                let groups = self.paginator_groups(cx);
                // Refresh in place; navigation restores a remembered chip and
                // page anchor after installing the destination's children.
                let preserve_position = matches!(pending.intent, LoadIntent::Refresh);
                let restored_cursor = changing_location.then(|| self.selected_chips.get(&self.location)).flatten().and_then(|saved| {
                    let index = match &saved.cursor {
                        SavedCursor::Chip(chip_id) => self.adapter.children().iter().position(|chip| &chip.id == chip_id)?,
                        SavedCursor::Book(content_hash) => self.adapter.children().len() + self.adapter.books().iter().position(|book| A::book_content_hash(book, cx) == Some(*content_hash))?,
                    };
                    Some((index, saved.child_anchor))
                });
                self.paginator.update(cx, |paginator, cx| {
                    if preserve_position {
                        paginator.refresh_groups(groups, cx);
                    } else {
                        paginator.set_groups(groups, cx);
                    }
                    if pending.intent.reveals_end() {
                        paginator.show_end(cx);
                    }
                    if let Some((index, child_anchor)) = restored_cursor {
                        paginator.restore_cursor(index, child_anchor, cx);
                    }
                });
                // Replacing groups resets the keyboard cursor; refreshing can
                // clamp it. Publish that state to chip and book highlights too.
                self.sync_cursor(cx);
            }
            Err(error) => {
                log::warn!("Browse failed for {}: {}", pending.location, error);
                self.error = Some(error.into());
            }
        }
        cx.notify();
    }

    fn breadcrumb(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let mut breadcrumb = components::folder_breadcrumb();
        if self.path.is_empty() {
            breadcrumb = breadcrumb.child(self.breadcrumb_drop_target(components::folder_breadcrumb_root_item(self.root_label.clone(), true, theme), sync_common::ROOT_DIR_ID, page.clone()));
        } else {
            let root_page = page.clone();
            let root = components::folder_breadcrumb_root_item(self.root_label.clone(), false, theme).on_click(move |_, _, cx| {
                root_page.update(cx, |page, cx| page.open_path(Vec::new(), cx));
            });
            breadcrumb = breadcrumb.child(self.breadcrumb_drop_target(root, sync_common::ROOT_DIR_ID, page.clone()));
        }
        for (index, crumb) in self.path.iter().enumerate() {
            let current = index + 1 == self.path.len();
            let item = components::folder_breadcrumb_item(crumb.label.clone(), current, theme);
            let destination = DirId::parse_str(&crumb.location).ok();
            if current {
                if let Some(destination) = destination {
                    breadcrumb = breadcrumb.child(self.breadcrumb_drop_target(item, destination, page.clone()));
                } else {
                    breadcrumb = breadcrumb.child(item);
                }
            } else {
                let path = self.path[..=index].to_vec();
                let path_page = page.clone();
                let item = item.on_click(move |_, _, cx| {
                    path_page.update(cx, |page, cx| page.open_path(path.clone(), cx));
                });
                if let Some(destination) = destination {
                    breadcrumb = breadcrumb.child(self.breadcrumb_drop_target(item, destination, page.clone()));
                } else {
                    breadcrumb = breadcrumb.child(item);
                }
            }
        }
        components::folder_breadcrumb_trail(breadcrumb).into_any_element()
    }

    fn breadcrumb_drop_target(&self, item: BreadcrumbItem, _destination: DirId, _page: Entity<Self>) -> BreadcrumbItem {
        // BreadcrumbItem is intentionally click-only in the component library.
        // Folder drops remain available on folder cards and the page body.
        item
    }

    fn paginator_groups(&self, cx: &mut Context<Self>) -> Vec<PaginatorGroup> {
        // Only a folder listing can be dragged: its children are directories,
        // and its books sit in a folder a move can name as their source.
        let source = self.folder_actions.is_some().then(|| DirId::parse_str(&self.location).ok()).flatten();
        super::layout::paginator_groups::<A>(
            cx.entity().downgrade(),
            self.adapter.children(),
            self.adapter.chip_groups(),
            self.adapter.child_icon(),
            self.adapter.books(),
            self.detailed_cards,
            self.cursor.clone(),
            self.selected.clone(),
            self.drop_target.clone(),
            self.marquee.clone(),
            source,
            self.paginator.downgrade(),
        )
    }

    /// Tells every card which mode it is in. Cards outlive a load — a book still
    /// in the folder keeps its entity — but new ones arrive at the default, so
    /// this runs after each install as well as on the switch itself.
    fn sync_card_mode(&mut self, cx: &mut Context<Self>) {
        for book in self.adapter.books().iter() {
            A::set_book_detailed(book, self.detailed_cards, cx);
        }
    }

    /// Mirrors the paginator's cursor out to the things that draw it: the
    /// shared cell the chip closures read, and each book card in turn.
    ///
    /// Books are declared after chips, so the flat child index of a book is its
    /// position plus the chip count.
    fn sync_cursor(&mut self, cx: &mut Context<Self>) {
        let cursor = self.paginator.read(cx).cursor();
        self.cursor.set(cursor);
        let chip_count = self.adapter.children().len();
        let focused_book = cursor.and_then(|index| index.checked_sub(chip_count));
        for (index, book) in self.adapter.books().iter().enumerate() {
            A::set_book_focused(book, focused_book == Some(index), cx);
        }
    }

    fn navigation_heading(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        if components::uses_mobile_navigation(window) {
            let title = self.path.last().map(|crumb| crumb.label.clone()).unwrap_or_else(|| self.root_label.to_string());
            let back = components::mobile_back_button(components::browser_theme(cx)).on_click(|_, window, cx| window.dispatch_action(Box::new(DismissSettings), cx));
            components::mobile_navigation_heading(title, back)
        } else {
            components::browser_topbar_left().child(self.breadcrumb(cx))
        }
    }

    fn render_topbar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let search_page = page.clone();
        let search_button_label = if self.search_open { "Hide search" } else { "Search books or authors" };
        let browse_controls = self.adapter.controls().clone();
        let has_options = !browse_controls.collection_sorts.is_empty() || !browse_controls.book_sorts.is_empty() || !browse_controls.formats.is_empty() || !browse_controls.languages.is_empty() || browse_controls.supports_direct_child_books;
        let close_split_page = page.clone();
        let split_page = page.clone();
        let controls = components::browser_topbar_right()
            .when(self.search_open, |controls| controls.flex_1())
            .children(self.search_open.then(|| div().flex_1().min_w(rems(8.0)).child(self.search_field.as_ref().expect("search field initialized above").render())))
            .child(
                components::topbar_icon_action_button("toggle-browse-search", search_button_label, IconName::Search, theme)
                    .when(self.search_open || !self.selection.search.is_empty(), |button| button.bg(theme.accent).text_color(theme.accent_text))
                    .on_click(move |_, window, cx| search_page.update(cx, |page, cx| page.toggle_search(window, cx))),
            )
            .children(self.folder_actions.as_ref().map(|actions| FolderActions::render_trigger(actions.clone(), cx)))
            .when(self.can_split(window), |controls| {
                controls.child(
                    components::topbar_icon_action_button("split-browse-page", "Open split pane", IconName::PanelLeft, theme)
                        .debug_selector(|| "split-browse-button".into())
                        .on_click(move |_, _, cx| split_page.update(cx, |page, cx| page.request_split(BrowseSplitSide::Right, cx))),
                )
            })
            .when(self.split_close_enabled && self.can_split(window), |controls| {
                controls.child(components::topbar_icon_action_button("close-split-pane", "Close split pane", IconName::Close, theme).on_click(move |_, _, cx| close_split_page.update(cx, |page, cx| page.request_close_split(cx))))
            })
            .when(has_options, |controls| controls.child(browse_options_control(page.clone(), browse_controls, self.selection.options(), |page: &mut Self, event, cx| page.select_option(event, cx), theme)));
        components::browser_topbar(theme).child(self.navigation_heading(window, cx)).child(controls).into_any_element()
    }

    pub(super) fn can_split(&self, window: &Window) -> bool {
        self.split_enabled && crate::split_panes_available(window)
    }

    pub(super) fn set_split_close_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.split_close_enabled = enabled;
        cx.notify();
    }

    pub(crate) fn set_library_view(&mut self, detailed: bool, cx: &mut Context<Self>) {
        if !A::supports_detailed_cards() || self.detailed_cards == detailed {
            return;
        }
        self.detailed_cards = detailed;
        self.sync_card_mode(cx);
        let groups = self.paginator_groups(cx);
        self.paginator.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        self.sync_cursor(cx);
        cx.notify();
    }

    pub(crate) fn request_split(&mut self, side: BrowseSplitSide, cx: &mut Context<Self>) {
        if !self.split_enabled {
            return;
        }
        cx.emit(BrowseSplitRequest { path: self.path.clone(), side });
    }

    pub(crate) fn request_split_child(&mut self, child_id: String, side: BrowseSplitSide, cx: &mut Context<Self>) {
        if !self.split_enabled {
            return;
        }
        let Some(child) = self.adapter.children().iter().chain(self.adapter.chip_groups().iter().map(|group| &group.parent)).find(|child| child.id == child_id).cloned() else { return };
        let mut path = self.path.clone();
        if self.selection.search.is_empty() {
            if self.adapter.include_group_in_path()
                && let Some(group) = self.adapter.chip_groups().iter().find(|group| self.adapter.children()[group.start..group.end].iter().any(|row| row.id == child.id))
            {
                path.push(BrowseCrumb { location: group.parent.id.clone(), label: group.parent.name.clone() });
            }
            path.push(BrowseCrumb { location: child.id, label: child.name });
        } else if let Ok(Some(resolved)) = self.adapter.path_to(&child) {
            path = resolved;
        } else {
            path.push(BrowseCrumb { location: child.id, label: child.name });
        }
        cx.emit(BrowseSplitRequest { path, side });
    }

    pub(crate) fn request_close_split(&mut self, cx: &mut Context<Self>) {
        if self.split_enabled {
            cx.emit(BrowseSplitClose);
        }
    }

    pub(crate) fn open_split_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.open_path(path, cx);
    }

    pub(crate) fn open_book_detail(&mut self, book: library_model::BookCardRow, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        if !self.adapter.has_cached_book_detail(book.content_hash, cx) {
            self.adapter.prepare_book_detail(book.clone(), None);
        }
        self.adapter.show_cached_book_detail();
        let mut path = path;
        path.push(BrowseCrumb { location: super::library::book_location(book.content_hash), label: book.title });
        self.open_path(path, cx);
    }

    pub(crate) fn hide_cached_book_detail(&mut self, cx: &mut Context<Self>) {
        self.adapter.hide_cached_book_detail();
        cx.notify();
    }

    pub(crate) fn clear_cached_book_detail(&mut self, cx: &mut Context<Self>) {
        self.adapter.clear_cached_book_detail();
        cx.notify();
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let paginator = self.paginator.read(cx);
        let scanning_empty_library = paginator.is_empty() && self.folder_actions.as_ref().is_some_and(|actions| actions.read(cx).scanner_running(cx));
        let waiting_for_library = paginator.is_empty() && self.folder_actions.as_ref().is_some_and(|actions| actions.read(cx).library_loading(cx));
        // Only the states that are genuinely a wait: a library still opening,
        // and a scan that has not yet found anything. Neither is a browse
        // request, and there is nothing painted underneath either to keep.
        let show_loading = (scanning_empty_library || waiting_for_library) && self.error.is_none();
        let loading_label = if scanning_empty_library { "Scanning library…" } else { "Loading…" };
        let import_page = page.clone();
        let folder_drop_page = page.clone();
        let folder_drop_destination = DirId::parse_str(&self.location).ok();
        let folder_can_drop_destination = folder_drop_destination.clone();
        let body = div()
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .on_drop::<gpui::ExternalPaths>(move |paths, _, cx| {
                let paths = paths.paths().to_vec();
                import_page.update(cx, |page, cx| page.import_dropped_paths(None, paths, cx));
            })
            .on_drop::<FolderTargets>(move |targets, window, cx| {
                let Some(destination) = folder_drop_destination.clone() else { return };
                folder_drop_page.update(cx, |page, cx| page.drop_onto_folder(destination, targets.clone(), window, cx));
            })
            .can_drop(move |dragged, _, _| {
                let Some(destination) = folder_can_drop_destination else { return false };
                dragged.is::<gpui::ExternalPaths>() || dragged.downcast_ref::<FolderTargets>().is_some_and(|targets| !targets.all_in_folder(destination) && !targets.folders.contains(&destination))
            })
            .child(self.paginator.clone())
            .when(show_loading, |body| body.child(components::absolute_full_size().bg(theme.page_bg).flex().items_center().justify_center().child(components::browser_loading_message(loading_label, theme))))
            .into_any_element();
        if let Some(error) = self.error.clone() {
            let retry_page = page.clone();
            let retry = components::topbar_action_button("retry-section", "Retry", IconName::Redo2, theme).on_click(move |_, _, cx| retry_page.update(cx, |page, cx| page.reload(cx)));
            components::browser_error_banner(error, retry, body).into_any_element()
        } else {
            body
        }
    }

    fn render_shell(&self, topbar: AnyElement, body: AnyElement, cx: &mut Context<Self>) -> gpui::Div {
        let folder_overlay = self.folder_actions.as_ref().and_then(|actions| FolderActions::render_overlay(actions.clone(), cx));
        let chip_menu = self.chip_menu.as_ref().map(|active| deferred(anchored().position(active.position).snap_to_window_with_margin(px(8.0)).anchor(Anchor::TopLeft).child(active.menu.clone())).with_priority(1).into_any_element());
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
    }
}

impl<A: BrowseAdapter> Render for BrowsePage<A> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch_for_menu_requests(window, cx);
        if let Some(body) = self.adapter.custom_body() {
            let theme = components::browser_theme(cx);
            let page = cx.entity();
            let close = components::topbar_icon_action_button("close-book-detail", "Close", IconName::Close, theme).on_click(move |_, _, cx| {
                page.update(cx, |page, cx| {
                    page.hide_cached_book_detail(cx);
                    if !page.path.is_empty() {
                        let mut parent = page.path.clone();
                        parent.pop();
                        page.open_path(parent, cx);
                    }
                });
            });
            let body = div().relative().size_full().child(body).child(div().absolute().top(px(12.0)).right(px(12.0)).child(close)).into_any_element();
            self.render_shell(div().h(px(0.0)).flex_none().into_any_element(), body, cx)
        } else {
            let topbar = self.render_topbar(window, cx);
            let body = self.render_body(cx);
            self.render_shell(topbar, body, cx)
        }
    }
}

impl<A: BrowseAdapter> gpui::Focusable for BrowsePage<A> {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
#[path = "grouping_tests.rs"]
mod grouping_tests;

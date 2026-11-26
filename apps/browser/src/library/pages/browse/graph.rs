//! The graph view of a browse page: independently expanded branches around
//! the location being browsed, on an infinite canvas.
//!
//! The location being browsed is the graph's selection. Tapping or navigating
//! to a node browses it; arrow navigation also centres that node in the camera.

use ui_text::text;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::library::widgets::{BookCardPresentation, Paginator, PaginatorAxis, PaginatorEvent, PaginatorGroup, PaginatorPosition, graph_cover_policy, new_paginator};
use gpui::prelude::*;
use gpui::{Context, div};
use gpui_component::{Disableable, IconName, Theme};
use std::time::{Duration, Instant};

use gpui::{AnyElement, App, Bounds, Entity, FocusHandle, IntoElement, MouseUpEvent, Pixels, Point, SharedString, Size, Subscription, Task, Window, point, px, size};
use library_model::{BrowseHierarchy, BrowseLocation, BrowsePathSegment, LibraryBrowseQuery};
use ui_components::{self as components, CanvasCamera, GraphLabel, GraphLayout, GraphTree, InfiniteCanvas, LayoutParams};

use super::listing::BrowseKind;
use super::navigator::{BrowseNavigator, Loaded};

const MIN_ZOOM: f32 = 0.3;
const MAX_ZOOM: f32 = 2.5;
const MOVE_DURATION: Duration = Duration::from_millis(100);

/// What a tap on the graph asks the page to do.
#[derive(Clone)]
pub(super) enum GraphTap {
    /// Browse this trail's location.
    Select(Vec<BrowsePathSegment>),
    /// Enter opens the selection in the cards view.
    Open,
}

/// A visible node keeps its stable database identity and packed parent link.
struct GraphEntry {
    label: GraphLabel,
    name: SharedString,
    location: BrowseLocation,
    parent: Option<usize>,
    depth: usize,
    branch: usize,
}

/// A layout and the entry for each of its nodes.
struct Drawn {
    layout: Arc<GraphLayout>,
    entries: Arc<Vec<GraphEntry>>,
}

/// Matches and their ancestor paths, independent of the saved expansion state.
struct GraphFilter {
    visible: Vec<bool>,
    has_matches: bool,
}

impl GraphFilter {
    fn new(hierarchy: &BrowseHierarchy, query: &str) -> Self {
        let mut visible: Vec<_> = hierarchy.nodes.iter().map(|node| node.row.name.to_lowercase().contains(query)).collect();
        let has_matches = visible.iter().any(|matched| *matched);
        for index in (0..visible.len()).rev() {
            if visible[index]
                && let Some(parent) = hierarchy.nodes[index].parent
            {
                visible[parent] = true;
            }
        }
        if let Some(root) = visible.first_mut() {
            *root = true;
        }
        Self { visible, has_matches }
    }
}

/// Structural and typographic inputs to layout.
#[derive(Clone, PartialEq)]
struct GraphKey {
    revision: u64,
    root: BrowseLocation,
    root_label: String,
    path: Vec<BrowsePathSegment>,
    compact: bool,
    rem_size: Pixels,
    font_family: SharedString,
}

/// The settled layout for a key, which the drawn one animates towards.
struct CachedGraph {
    key: GraphKey,
    target: Drawn,
}

struct NodeMotion {
    origin: Point<f32>,
    appearing: bool,
}

struct GraphMotion {
    started: Instant,
    nodes: HashMap<BrowseLocation, NodeMotion>,
    from_camera: CanvasCamera,
    last_camera: CanvasCamera,
    target_camera: CanvasCamera,
    selected_key: BrowseLocation,
    anchor: Point<Pixels>,
    departing: Arc<GraphLayout>,
    /// The entries the departing layout's nodes index.
    departing_entries: Arc<Vec<GraphEntry>>,
    ghost_opacity: f32,
}

pub(super) struct BrowseGraph {
    canvas: Entity<InfiniteCanvas>,
    focus: FocusHandle,
    navigator: Entity<BrowseNavigator>,
    /// The location's own books, beside the graph.
    covers: Entity<Paginator>,
    covers_focus: FocusHandle,
    covers_focus_subscription: Option<Subscription>,
    cover_positions: HashMap<BrowseLocation, PaginatorPosition>,
    sheet_expanded: bool,
    sidebar_width: f32,
    sidebar_drag: Option<(f32, f32)>,
    /// One completed snapshot, independent of the current selection.
    hierarchy: Option<Arc<BrowseHierarchy>>,
    search: String,
    filter: Option<GraphFilter>,
    source_revision: u64,
    expanded: HashSet<BrowseLocation>,
    /// Preserve visibility before the first click selects and opens a node.
    node_click: Option<(BrowseLocation, bool)>,
    tree_query: Option<LibraryBrowseQuery>,
    expand_all: bool,
    hierarchy_request: Option<Task<()>>,
    hierarchy_error: Option<SharedString>,
    target: Option<CachedGraph>,
    layout_revision: u64,
    layout_key: Option<GraphKey>,
    failed_layout_key: Option<GraphKey>,
    layout_request: Option<Task<()>>,
    layout_error: Option<SharedString>,
    drawn: Option<Drawn>,
    /// The next arrow-key selection to centre after its children have loaded.
    center_for: Option<BrowseLocation>,
    /// The selection the camera was last placed for.
    placed_for: Option<BrowseLocation>,
    motion: Option<GraphMotion>,
    _loaded: Subscription,
    _search: Subscription,
    _covers_subscription: Subscription,
}

impl gpui::EventEmitter<GraphTap> for BrowseGraph {}

impl BrowseGraph {
    pub(super) fn new(navigator: Entity<BrowseNavigator>, cx: &mut Context<Self>) -> Self {
        let covers_id = format!("{}-graph-covers", navigator.read(cx).kind.list_prefix());
        let loaded = cx.subscribe(&navigator, |graph, _, loaded: &Loaded, cx| graph.install(loaded, cx));
        let search = cx.observe(&navigator, |graph, _, cx| graph.sync_search(cx));
        let covers = cx.new(|_| new_paginator(covers_id, []).flow_axis(PaginatorAxis::Vertical));
        let covers_subscription = cx.subscribe(&covers, |graph, covers, event: &PaginatorEvent, cx| {
            if *event != PaginatorEvent::PageChanged {
                return;
            }
            let flow = covers.read(cx);
            let Some(range) = flow.visible_child_range() else { return };
            let range = range.start..flow.adjacent_child_range(true).map_or(range.end, |next| next.end.max(range.end));
            let sections = graph.navigator.read(cx).listing.page.as_ref().map_or(0, |page| page.heading_count());
            graph.navigator.update(cx, |navigator, cx| navigator.load_page_range(range.start + sections..range.end + sections, 0, cx));
        });
        Self {
            navigator,
            focus: cx.focus_handle(),
            covers,
            covers_focus: cx.focus_handle(),
            covers_focus_subscription: None,
            cover_positions: HashMap::new(),
            sheet_expanded: false,
            sidebar_width: 380.0,
            sidebar_drag: None,
            canvas: cx.new(|_| InfiniteCanvas::new(MIN_ZOOM, MAX_ZOOM)),
            hierarchy: None,
            search: String::new(),
            filter: None,
            source_revision: 0,
            expanded: HashSet::new(),
            node_click: None,
            tree_query: None,
            expand_all: false,
            hierarchy_request: None,
            hierarchy_error: None,
            target: None,
            layout_revision: 0,
            layout_key: None,
            failed_layout_key: None,
            layout_request: None,
            layout_error: None,
            drawn: None,
            center_for: None,
            placed_for: None,
            motion: None,
            _loaded: loaded,
            _search: search,
            _covers_subscription: covers_subscription,
        }
    }

    /// Install the current books and invalidate topology when the source changes.
    fn install(&mut self, loaded: &Loaded, cx: &mut Context<Self>) {
        if let Some(left) = loaded.left {
            self.cover_positions.insert(left, self.covers.read(cx).position());
        }
        let navigator = self.navigator.read(cx);
        let location = navigator.destination.location;
        let root = navigator.root();
        let query = navigator.child_query(root);
        let path = navigator.destination.path.clone();
        let source_revision = navigator.listing.revision;
        let books = navigator.listing.books.clone();
        let page = navigator.listing.page.clone();
        let book_indices = navigator.listing.book_indices.clone();
        let covers = super::layout::direct_book_children(books, page.as_ref(), &book_indices, BookCardPresentation::GraphCover, None, cx);
        let groups = if covers.is_empty() { Vec::new() } else { vec![PaginatorGroup::from_slots(graph_cover_policy(), covers)] };
        let query_changed = self.tree_query.as_ref() != Some(&query);
        if query_changed || loaded.refresh && self.source_revision != source_revision {
            if self.tree_query.as_ref().is_some_and(|previous| previous.location != root) {
                self.expanded.clear();
                self.expand_all = false;
            }
            self.hierarchy = None;
            self.filter = None;
            self.hierarchy_request = None;
            self.hierarchy_error = None;
            self.tree_query = Some(query);
            self.invalidate_layout();
        }
        if query_changed {
            self.node_click = None;
            self.expanded.insert(root);
            self.expanded.extend(path.iter().map(|crumb| crumb.id));
        }
        self.source_revision = source_revision;
        let restored = loaded.left.and_then(|_| self.cover_positions.get(&location).cloned());
        self.covers.update(cx, |covers, cx| covers.install_groups(groups, loaded.refresh, restored.as_ref(), cx));
        self.select_visible(cx);
        cx.notify();
    }

    fn sync_search(&mut self, cx: &mut Context<Self>) {
        let search = self.navigator.read(cx).graph_search().trim().to_lowercase();
        if self.search == search {
            return;
        }
        self.search = search;
        self.filter_hierarchy(cx);
    }

    fn filter_hierarchy(&mut self, cx: &mut Context<Self>) {
        self.filter = self.hierarchy.as_ref().filter(|_| !self.search.is_empty()).map(|hierarchy| GraphFilter::new(hierarchy, &self.search));
        self.node_click = None;
        self.center_for = None;
        self.invalidate_layout();
        self.select_visible(cx);
        cx.notify();
    }

    /// A filtered-out selection falls back along its own ancestor path.
    fn selection_fallback(&self, path: &[BrowsePathSegment]) -> Option<Vec<BrowsePathSegment>> {
        let (Some(hierarchy), Some(filter)) = (&self.hierarchy, &self.filter) else { return None };
        let location = path.last()?.id;
        let mut index = hierarchy.nodes.iter().position(|node| node.row.id == location)?;
        if filter.visible[index] {
            return None;
        }
        while !filter.visible[index] {
            index = hierarchy.nodes[index].parent?;
        }
        Some(hierarchy.path(index))
    }

    fn select_visible(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.selection_fallback(&self.navigator.read(cx).destination.path) {
            self.center_for = None;
            self.motion = None;
            cx.emit(GraphTap::Select(path));
        }
    }

    /// Publish the entire hierarchy together; expansion only changes visibility.
    fn load_hierarchy(&mut self, cx: &mut Context<Self>) {
        if self.hierarchy.is_some() {
            return;
        }
        if self.hierarchy_request.is_some() || self.hierarchy_error.is_some() {
            return;
        }
        let Some(query) = self.tree_query.clone() else { return };
        let request = self.navigator.read(cx).graph_hierarchy_request(query.location);
        self.hierarchy_request = Some(cx.spawn(async move |graph, cx| {
            let result = request.await;
            let _ = graph.update(cx, |graph, cx| {
                if graph.tree_query.as_ref() != Some(&query) {
                    return;
                }
                graph.hierarchy_request = None;
                match result {
                    Ok(hierarchy) => {
                        graph.hierarchy = Some(Arc::new(hierarchy));
                        graph.filter_hierarchy(cx);
                    }
                    Err(error) => {
                        log::warn!("Could not load graph hierarchy: {error}");
                        graph.hierarchy_error = Some(components::user_facing_error(&error, text!(browse::could_not_load_this_page_please_try_again)).into());
                    }
                }
                cx.notify();
            });
        }));
    }

    fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.expand_all = true;
        self.hierarchy_error = None;
        self.load_hierarchy(cx);
        self.invalidate_layout();
        cx.notify();
    }

    fn collapse_others(&mut self, cx: &mut Context<Self>) {
        self.expand_all = false;
        self.hierarchy_request = None;
        self.hierarchy_error = None;
        let navigator = self.navigator.read(cx);
        self.expanded = std::iter::once(navigator.root()).chain(navigator.destination.path.iter().map(|crumb| crumb.id)).collect();
        self.invalidate_layout();
        self.load_hierarchy(cx);
        cx.notify();
    }

    fn toggle_subtree(&mut self, location: BrowseLocation, collapse: bool, cx: &mut Context<Self>) {
        if !self.search.is_empty() {
            return;
        }
        let Some(hierarchy) = self.hierarchy.clone() else { return };
        let Some(root) = hierarchy.nodes.iter().position(|node| node.row.id == location) else { return };
        if hierarchy.children(root).is_empty() {
            return;
        }
        // Keep other branches open when collapsing within an expanded graph.
        if self.expand_all {
            self.expanded.extend(hierarchy.nodes.iter().enumerate().filter(|(index, _)| !hierarchy.children(*index).is_empty()).map(|(_, node)| node.row.id));
            self.expand_all = false;
        }
        let mut pending = vec![root];
        while let Some(index) = pending.pop() {
            let node = &hierarchy.nodes[index];
            let children = hierarchy.children(index);
            if collapse {
                self.expanded.remove(&node.row.id);
            } else if !children.is_empty() {
                self.expanded.insert(node.row.id);
            }
            pending.extend(children);
        }
        self.center_for = None;
        self.invalidate_layout();
        cx.notify();
    }

    fn click_node(&mut self, location: BrowseLocation, click_count: usize, cx: &mut Context<Self>) {
        if click_count == 2 {
            if let Some((location, children_visible)) = self.node_click.take() {
                self.toggle_subtree(location, children_visible, cx);
            }
            return;
        }
        if click_count != 1 {
            return;
        }
        let Some(drawn) = &self.drawn else { return };
        let Some(index) = drawn.entries.iter().position(|entry| entry.location == location) else { return };
        // The root keeps its first level visible, so it toggles deeper levels.
        let children_visible = drawn.layout.nodes().iter().any(|node| if index == 0 { drawn.entries[node.node].depth > 1 } else { node.parent == Some(index) });
        self.node_click = Some((location, children_visible));
        if let Some(tap) = self.activate(index) {
            cx.emit(tap);
        }
    }

    pub(super) fn expansion_controls(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .gap(px(components::SPACE_XS))
            .child(
                components::outlined_icon_button("expand-graph-trees", text!(browse::expand_all_trees), IconName::Maximize)
                    .disabled(!self.search.is_empty() || self.expand_all && self.hierarchy_error.is_none())
                    .loading(self.hierarchy_request.is_some())
                    .on_click(cx.listener(|graph, _, _, cx| graph.expand_all(cx))),
            )
            .child(components::outlined_icon_button("collapse-other-graph-trees", text!(browse::collapse_other_trees), IconName::Minimize).disabled(!self.search.is_empty()).on_click(cx.listener(|graph, _, _, cx| graph.collapse_others(cx))))
            .into_any_element()
    }

    fn set_sheet_expanded(&mut self, expanded: bool) -> bool {
        let changed = self.sheet_expanded != expanded;
        self.sheet_expanded = expanded;
        changed
    }

    fn resize_sidebar(&mut self, x: f32, pressed: bool, rem_size: Pixels) -> bool {
        let Some((start_x, start_width)) = self.sidebar_drag else { return false };
        if !pressed {
            self.sidebar_drag = None;
            return false;
        }
        let minimum = f32::from(rem_size) * 17.5;
        let width = (start_width + start_x - x).max(minimum);
        if (self.sidebar_width - width).abs() < 0.5 {
            return false;
        }
        self.sidebar_width = width;
        true
    }

    pub(super) fn render_panel(&mut self, controls: AnyElement, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let navigator = self.navigator.read(cx);
        let (kind, path, has_books) = (navigator.kind, navigator.destination.path.clone(), navigator.listing.page.as_ref().map_or(!navigator.listing.books.is_empty(), |page| page.book_total > 0));
        let graph = self.render(kind, &path, window, cx);
        let focus = self.focus.clone();
        let graph = div().size_full().track_focus(&self.focus).on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| focus.focus(window, cx)).child(graph);
        let graph = crate::library::pages::with_retry_banner(
            graph.into_any_element(),
            self.layout_error.clone().or_else(|| self.hierarchy_error.clone()),
            "retry-graph-expansion",
            cx.listener(|graph, _, _, cx| {
                graph.hierarchy_error = None;
                graph.invalidate_layout();
                graph.load_hierarchy(cx);
                cx.notify();
            }),
        );
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        if self.covers_focus_subscription.is_none() {
            self.covers_focus_subscription = Some(cx.on_focus_in(&self.covers_focus.clone(), window, |graph, _, cx| {
                if graph.set_sheet_expanded(true) {
                    cx.notify();
                }
            }));
        }
        let covers = div().id("graph-covers-focus").track_focus(&self.covers_focus).size_full().child(self.covers.clone()).into_any_element();
        let graph_area = div().relative().flex_1().min_h_0().min_w_0().child(graph).child(div().absolute().right(px(8.0)).top(px(8.0)).child(controls)).when(self.filter.as_ref().is_some_and(|filter| !filter.has_matches), |area| {
            area.child(div().absolute().left(px(components::CONTENT_INSET)).bottom(px(components::CONTENT_INSET)).text_color(theme.text_muted).child(text!(browse::no_matches)))
        });
        // The grid holds the selection's own books, so with none it is
        // not drawn at all rather than drawn empty.
        if !has_books {
            return graph_area.size_full().into_any_element();
        }
        if compact {
            let expanded = self.sheet_expanded;
            let cover_sheet = div()
                .relative()
                .w_full()
                .when(expanded, |sheet| sheet.flex_1().min_h_0())
                .when(!expanded, |sheet| sheet.h(gpui::rems(7.3125)).flex_none())
                .bg(theme.page_bg)
                .border_t_1()
                .border_color(theme.rule)
                .flex()
                .flex_col()
                .child(div().flex_1().min_h_0().child(covers))
                .when(!expanded, |sheet| {
                    sheet.child(
                        div()
                            .id("graph-cover-sheet-peek")
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .on_click(cx.listener(|graph, _, _, cx| {
                                if graph.set_sheet_expanded(true) {
                                    cx.notify();
                                }
                            }))
                            .on_scroll_wheel(cx.listener(|graph, event: &gpui::ScrollWheelEvent, _, cx| {
                                if let gpui::ScrollDelta::Pixels(delta) = event.delta
                                    && f32::from(delta.y) < -3.0
                                    && graph.set_sheet_expanded(true)
                                {
                                    cx.notify();
                                }
                                cx.stop_propagation();
                            })),
                    )
                })
                // The handle floats over the top of the grid.
                .child(
                    div()
                        .id("graph-cover-sheet-handle")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().w(px(32.0)).h(px(3.0)).rounded(px(2.0)).bg(theme.rule))
                        .on_click(cx.listener(|graph, _, _, cx| {
                            graph.set_sheet_expanded(!graph.sheet_expanded);
                            cx.notify();
                        }))
                        .on_scroll_wheel(cx.listener(|graph, event: &gpui::ScrollWheelEvent, _, cx| {
                            if let gpui::ScrollDelta::Pixels(delta) = event.delta
                                && f32::from(delta.y) > 3.0
                                && graph.set_sheet_expanded(false)
                            {
                                cx.notify();
                            }
                        })),
                );
            div().size_full().flex().flex_col().child(graph_area).child(cover_sheet).into_any_element()
        } else {
            let sidebar = div().size_full().min_h_0().flex().flex_col().child(div().flex_1().min_h_0().child(covers));
            let sidebar_width = self.sidebar_width;
            div()
                .relative()
                .size_full()
                .flex()
                .flex_row()
                .child(graph_area)
                .child(div().relative().flex_shrink_1().w(px(sidebar_width)).min_w_0().min_h_0().child(sidebar).child(
                    div().id("graph-sidebar-resize-handle").absolute().top_0().left(px(-4.0)).h_full().w(px(9.0)).cursor_col_resize().flex().justify_center().child(div().h_full().w(px(1.0)).bg(theme.rule)).on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|graph, event: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            graph.sidebar_drag = Some((f32::from(event.position.x), graph.sidebar_width));
                        }),
                    ),
                ))
                .on_mouse_move(cx.listener(|graph, event: &gpui::MouseMoveEvent, window, cx| {
                    if graph.resize_sidebar(f32::from(event.position.x), event.pressed_button == Some(gpui::MouseButton::Left), window.rem_size()) {
                        cx.notify();
                    }
                }))
                .on_mouse_up(gpui::MouseButton::Left, cx.listener(|graph, _, _, _| graph.sidebar_drag = None))
                .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(|graph, _, _, _| graph.sidebar_drag = None))
                .into_any_element()
        }
    }

    fn invalidate_layout(&mut self) {
        self.layout_revision = self.layout_revision.wrapping_add(1);
        self.layout_key = None;
        self.failed_layout_key = None;
        self.layout_error = None;
        self.target = None;
        self.motion = None;
    }

    pub(super) fn handle_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if let Some(action) = self.key(key) {
            if matches!(key, "left" | "right" | "up" | "down")
                && let GraphTap::Select(path) = &action
            {
                self.center_for = Some(path.last().map_or_else(|| self.navigator.read(cx).root(), |crumb| crumb.id));
            }
            cx.emit(action);
            return true;
        }
        false
    }

    /// Arrows walk the hierarchy; Up/Down cross expanded branches at the same
    /// depth and on the same side. Page Up/Down move to the parent/first child.
    /// Enter opens the selected location in cards.
    pub(super) fn key(&mut self, key: &str) -> Option<GraphTap> {
        let drawn = self.drawn.as_ref()?;
        // Use settled geometry only when its node identities match the drawn
        // graph. Animation must not change the navigation order or tree side.
        let layout = self.target.as_ref().filter(|target| Arc::ptr_eq(&target.target.entries, &drawn.entries)).map_or(&drawn.layout, |target| &target.target.layout);
        let selection = *layout.path().last()?;
        let selected_node = layout.node(selection)?;
        let parent = selected_node.parent;
        if key == "pageup" {
            return self.activate(parent?);
        }
        if key == "pagedown" {
            let child = layout.nodes().iter().find(|placed| placed.parent == Some(selection))?.node;
            return self.activate(child);
        }
        if matches!(key, "up" | "down") {
            parent?;
            let root_x = layout.node(0)?.center.x;
            // Every descendant belongs to its top-level branch's side, even
            // when its own label crosses the root's vertical axis.
            let branch_sides: HashMap<_, _> = layout.nodes().iter().filter(|placed| placed.parent == Some(0)).map(|placed| (placed.node, placed.center.x < root_x)).collect();
            let side_of = |node: usize| branch_sides.get(&drawn.entries[node].branch).copied();
            let side = side_of(selection)?;
            let depth = drawn.entries[selection].depth;
            let mut peers: Vec<_> = layout.nodes().iter().filter(|placed| drawn.entries[placed.node].depth == depth && side_of(placed.node) == Some(side)).collect();
            peers.sort_by(|a, b| a.center.y.total_cmp(&b.center.y).then_with(|| a.center.x.total_cmp(&b.center.x)).then_with(|| drawn.entries[a.node].location.cmp(&drawn.entries[b.node].location)));
            let index = peers.iter().position(|placed| placed.node == selection)?;
            let next = if key == "down" { (index + 1) % peers.len() } else { (index + peers.len() - 1) % peers.len() };
            return self.activate(peers[next].node);
        }
        let side = if selected_node.angle.cos() < 0.0 { -1.0 } else { 1.0 };
        let first_child = |direction: f32| layout.nodes().iter().find(|placed| placed.parent == Some(selection) && placed.angle.cos() * direction > 0.0).map(|placed| placed.node);
        let node = match key {
            "left" if selection == 0 => first_child(-1.0)?,
            "right" if selection == 0 => first_child(1.0)?,
            "left" if side < 0.0 => first_child(side)?,
            "right" if side < 0.0 => parent?,
            "left" => parent?,
            "right" => first_child(side)?,
            "enter" => return Some(GraphTap::Open),
            _ => return None,
        };
        self.activate(node)
    }

    fn activate(&mut self, node: usize) -> Option<GraphTap> {
        let drawn = self.drawn.as_ref()?;
        drawn.layout.node(node)?;
        // A pending or rejected layout can leave the previous selection on
        // screen. Clicking it must still allow returning to that location.
        let trail = entry_trail(&drawn.entries, node);
        if self.layout_key.as_ref().map_or(Some(&node) == drawn.layout.path().last(), |key| key.path == trail) {
            return None;
        }
        let location = drawn.entries[node].location;
        if self.search.is_empty() {
            self.expanded.insert(location);
        }
        self.center_for = None;
        self.motion = None;
        Some(GraphTap::Select(trail))
    }

    /// The graph for `path` below `root`. Until every column along the path is
    /// known, the last graph drawn stays up.
    pub(super) fn render(&mut self, kind: BrowseKind, path: &[BrowsePathSegment], window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.load_hierarchy(cx);
        let path = self.selection_fallback(path).unwrap_or_else(|| path.to_vec());
        let (root, root_label) = (kind.root_location(), kind.root_label());
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let params = if compact { LayoutParams::compact() } else { LayoutParams::desktop() };
        let font_family = Theme::global(cx).font_family.clone();
        let key = GraphKey { revision: self.layout_revision, root, root_label: root_label.to_string(), path: path.clone(), compact, rem_size: window.rem_size(), font_family };
        self.layout_key = Some(key.clone());
        let needs_layout = self.target.as_ref().is_none_or(|target| target.key != key) && self.failed_layout_key.as_ref() != Some(&key);
        // Keep a single layout in flight. New inputs replace the desired key;
        // a stale result is discarded before the newest graph is scheduled.
        if needs_layout && self.layout_request.is_none() && self.hierarchy_request.is_none() && self.hierarchy_error.is_none() {
            if let Some((tree, entries, selection, root_sides)) = graph_tree(root, &root_label, &path, self.hierarchy.as_deref(), &self.expanded, self.expand_all, self.filter.as_ref()) {
                // Font measurement belongs to the UI thread. Only topology and
                // measured boxes enter the background layout worker.
                let sizes: Vec<Size<f32>> = entries
                    .iter()
                    .map(|entry| {
                        let measured = entry.label.measure(window, cx);
                        size(f32::from(measured.width), f32::from(measured.height))
                    })
                    .collect();
                self.layout_error = None;
                let layout = cx.background_executor().spawn(async move { components::graph_layout(&tree, selection, &root_sides, &|node, _| sizes[node], &params) });
                self.layout_request = Some(cx.spawn(async move |graph, cx| {
                    let result = layout.await;
                    let _ = graph.update(cx, |graph, cx| {
                        graph.layout_request = None;
                        if graph.layout_key.as_ref() == Some(&key) {
                            match result {
                                Ok(layout) => {
                                    graph.failed_layout_key = None;
                                    graph.target = Some(CachedGraph { key, target: Drawn { layout: Arc::new(layout), entries: Arc::new(entries) } });
                                }
                                Err(error) => {
                                    log::error!("Could not lay out browse graph: {error}");
                                    graph.failed_layout_key = Some(key);
                                    graph.layout_error = Some(text!(browse::could_not_arrange_graph).into());
                                }
                            }
                        }
                        cx.notify();
                    });
                }));
            }
        }
        let target_changed = self.target.as_ref().is_some_and(|target| self.drawn.as_ref().is_none_or(|drawn| !Arc::ptr_eq(&target.target.entries, &drawn.entries)));
        if let Some(CachedGraph { key, target, .. }) = &self.target
            && self.layout_key.as_ref() == Some(key)
        {
            let layout = Arc::clone(&target.layout);
            let entries = Arc::clone(&target.entries);
            let location = path.last().map_or(root, |crumb| crumb.id);
            let center_new = self.center_for == Some(location);
            if self.placed_for.is_none() || center_new {
                self.place_camera(&layout, &entries, location, window, cx);
            }
            if target_changed || self.motion.is_some() || self.drawn.is_none() {
                let layout = Arc::new(self.animated_layout(&layout, &entries, window, cx));
                self.drawn = Some(Drawn { layout, entries });
            }
        }
        let Some(drawn) = &self.drawn else {
            return components::infinite_canvas(&self.canvas).background(components::browser_theme(cx).page_bg).into_any_element();
        };
        let ghost = self.motion.as_ref().filter(|motion| !motion.departing.nodes().is_empty() && motion.ghost_opacity > 0.0);
        let graph = cx.weak_entity();
        // A tap browses; a double-click toggles the subtree. A pan or a fading
        // node does not activate anything.
        let entries = Arc::clone(&drawn.entries);
        let departing_entries = ghost.map(|motion| Arc::clone(&motion.departing_entries));
        let canvas = self.canvas.clone();
        let node = move |placed: &components::PlacedNode, departing: bool| {
            let entries = if departing { departing_entries.as_ref() } else { Some(&entries) };
            let Some(entry) = entries.and_then(|entries| entries.get(placed.node)) else { return div().into_any_element() };
            let label = div().id(SharedString::from(format!("graph-node-{}", entry.location))).size_full().child(entry.label.view(placed.role));
            if departing {
                return label.into_any_element();
            }
            let (graph, canvas, location) = (graph.clone(), canvas.clone(), entry.location);
            label
                .on_click(move |event, _, cx| {
                    if canvas.read(cx).panned() {
                        return;
                    }
                    let _ = graph.update(cx, |graph, cx| {
                        graph.click_node(location, event.click_count(), cx);
                    });
                })
                .into_any_element()
        };
        div()
            .size_full()
            // Recentering may move the first-clicked label away before the
            // second release. Complete that double-click on the canvas too.
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|graph, event: &MouseUpEvent, _, cx| {
                    if event.click_count == 2
                        && !graph.canvas.read(cx).panned()
                        && let Some((location, _)) = graph.node_click
                    {
                        graph.click_node(location, 2, cx);
                    }
                }),
            )
            .child(components::graph_canvas(&self.canvas, Arc::clone(&drawn.layout), ghost.map(|motion| (Arc::clone(&motion.departing), motion.ghost_opacity)), node, cx))
            .into_any_element()
    }

    /// Centres an arrow-key selection, or frames the initial selection.
    /// Camera placement waits for the canvas's first layout.
    fn place_camera(&mut self, layout: &GraphLayout, entries: &[GraphEntry], location: BrowseLocation, window: &Window, cx: &mut App) {
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let center_new = self.center_for == Some(location);
        let Some(&selection) = layout.path().last() else {
            self.motion = None;
            return;
        };
        let Some(entry) = entries.get(selection) else {
            self.motion = None;
            return;
        };
        let Some(bounds) = self.canvas.read(cx).bounds else {
            window.request_animation_frame();
            return;
        };
        let camera = if center_new {
            let anchor = point(bounds.size.width / 2.0, bounds.size.height / 2.0);
            let zoom = self.canvas.read(cx).camera.zoom;
            let Some(camera) = components::anchored_camera(layout, selection, anchor, zoom) else {
                self.motion = None;
                return;
            };
            camera
        } else {
            components::framing_camera(layout, Bounds::new(point(px(0.0), px(0.0)), bounds.size), compact, 1.0)
        };
        let from_camera = self.canvas.read(cx).camera;
        let selected_key = &entry.location;
        let previous = self.drawn.as_ref().and_then(|drawn| drawn.layout.nodes().iter().find(|placed| drawn.entries.get(placed.node).is_some_and(|entry| entry.location == *selected_key)).map(|placed| (drawn, placed.center)));
        if center_new && !cx.reduce_motion() {
            if let Some((drawn, old_center)) = previous {
                let old_centers: HashMap<BrowseLocation, Point<f32>> = drawn.layout.nodes().iter().filter_map(|placed| drawn.entries.get(placed.node).map(|entry| (entry.location, placed.center))).collect();
                let nodes = layout
                    .nodes()
                    .iter()
                    .filter_map(|placed| {
                        let entry = entries.get(placed.node)?;
                        let origin = old_centers.get(&entry.location).copied().or_else(|| placed.parent.and_then(|parent| entries.get(parent)).and_then(|parent| old_centers.get(&parent.location).copied())).unwrap_or(placed.center);
                        Some((entry.location, NodeMotion { origin, appearing: !old_centers.contains_key(&entry.location) }))
                    })
                    .collect();
                let anchor = from_camera.to_screen(point(px(old_center.x), px(old_center.y)));
                let visible: HashSet<BrowseLocation> = entries.iter().map(|entry| entry.location).collect();
                let mut departing = GraphLayout::clone(&drawn.layout);
                departing.retain_nodes(|placed| drawn.entries.get(placed.node).is_some_and(|entry| !visible.contains(&entry.location)));
                departing.retain_edges(|edge| drawn.entries.get(edge.to).is_some_and(|entry| !visible.contains(&entry.location)));
                self.motion = Some(GraphMotion {
                    started: Instant::now(),
                    nodes,
                    from_camera,
                    last_camera: from_camera,
                    target_camera: camera,
                    selected_key: *selected_key,
                    anchor,
                    departing: Arc::new(departing),
                    departing_entries: Arc::clone(&drawn.entries),
                    ghost_opacity: 1.0,
                });
            } else {
                self.motion = None;
                self.canvas.update(cx, |canvas, _| canvas.camera = camera);
            }
        } else {
            self.motion = None;
            self.canvas.update(cx, |canvas, _| canvas.camera = camera);
        }
        self.center_for = None;
        self.placed_for = Some(location);
    }

    fn animated_layout(&mut self, layout: &GraphLayout, entries: &[GraphEntry], window: &Window, cx: &mut App) -> GraphLayout {
        if cx.reduce_motion() {
            if let Some(motion) = self.motion.take() {
                self.canvas.update(cx, |canvas, _| canvas.camera = motion.target_camera);
            }
            return layout.clone();
        }
        let Some(motion) = &mut self.motion else {
            return layout.clone();
        };
        let Some((&selection, entry)) = layout.path().last().and_then(|selection| entries.get(*selection).map(|entry| (selection, entry))) else {
            self.motion = None;
            return layout.clone();
        };
        if entry.location != motion.selected_key || self.canvas.read(cx).camera != motion.last_camera {
            // A wheel, pinch, or drag takes control of the camera immediately.
            self.motion = None;
            return layout.clone();
        }
        let elapsed = motion.started.elapsed();
        let progress = (elapsed.as_secs_f32() / MOVE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        motion.ghost_opacity = 1.0 - progress;
        let posed = layout.posed(|placed| {
            let entry = entries.get(placed.node)?;
            let node = motion.nodes.get(&entry.location)?;
            let center = point(node.origin.x + (placed.center.x - node.origin.x) * progress, node.origin.y + (placed.center.y - node.origin.y) * progress);
            Some((center, if node.appearing { progress } else { 1.0 }))
        });
        let (Some(selected), Some(target)) = (posed.node(selection), layout.node(selection)) else {
            self.motion = None;
            return layout.clone();
        };
        let (selected, target) = (selected.center, target.center);
        let end = motion.target_camera.to_screen(point(px(target.x), px(target.y)));
        let zoom = motion.from_camera.zoom + (motion.target_camera.zoom - motion.from_camera.zoom) * progress;
        let screen = point(motion.anchor.x + (end.x - motion.anchor.x) * progress, motion.anchor.y + (end.y - motion.anchor.y) * progress);
        let camera = CanvasCamera { pan: point(screen.x - px(selected.x * zoom), screen.y - px(selected.y * zoom)), zoom };
        self.canvas.update(cx, |canvas, _| canvas.camera = camera);
        motion.last_camera = camera;
        if progress < 1.0 {
            window.request_animation_frame();
            posed
        } else {
            self.motion = None;
            layout.clone()
        }
    }
}

fn entry_trail(entries: &[GraphEntry], mut node: usize) -> Vec<BrowsePathSegment> {
    let mut trail = Vec::new();
    while let Some(parent) = entries[node].parent {
        let entry = &entries[node];
        trail.push(BrowsePathSegment { id: entry.location, name: entry.name.to_string() });
        node = parent;
    }
    trail.reverse();
    trail
}

/// Project expanded branches from one completed hierarchy into breadth-first order.
fn graph_tree(
    root: BrowseLocation, root_label: &str, path: &[BrowsePathSegment], hierarchy: Option<&BrowseHierarchy>, expanded: &HashSet<BrowseLocation>, expand_all: bool, filter: Option<&GraphFilter>,
) -> Option<(GraphTree, Vec<GraphEntry>, usize, Vec<bool>)> {
    let hierarchy = hierarchy.filter(|hierarchy| hierarchy.nodes.first().is_some_and(|node| node.row.id == root))?;
    // Sort complete root trees by book count, then alternate sides before
    // filtering visibility so expansion and search keep their assignments.
    let mut branches: Vec<_> = hierarchy.children(0).collect();
    branches.sort_unstable_by(|a, b| hierarchy.nodes[*b].row.book_count.cmp(&hierarchy.nodes[*a].row.book_count).then_with(|| a.cmp(b)));
    let mut root_sides = Vec::new();
    let selected_path: HashSet<_> = std::iter::once(root).chain(path.iter().map(|crumb| crumb.id)).collect();
    let selected = path.last().map_or(root, |crumb| crumb.id);
    let children_visible = |location| filter.is_some() || location == root || expand_all || expanded.contains(&location) || location != selected && selected_path.contains(&location);
    let label = |index: usize, name: SharedString| {
        let node = &hierarchy.nodes[index];
        let count = if children_visible(node.row.id) {
            let below: usize = hierarchy.children(index).filter(|child| filter.is_none_or(|filter| filter.visible[*child])).map(|child| hierarchy.nodes[child].row.book_count).sum();
            node.row.book_count.saturating_sub(below)
        } else {
            node.row.book_count
        };
        GraphLabel::new(name, Some(count))
    };
    let mut parents = vec![None];
    let name: SharedString = root_label.to_owned().into();
    let mut entries = vec![GraphEntry { label: label(0, name.clone()), name, location: root, parent: None, depth: 0, branch: 0 }];
    let mut sources = vec![0];
    let mut parent = 0;
    while parent < entries.len() {
        let location = entries[parent].location;
        if children_visible(location) {
            let children = hierarchy.children(sources[parent]);
            for position in 0..children.len() {
                let child = if parent == 0 { branches[position] } else { children.start + position };
                if filter.is_some_and(|filter| !filter.visible[child]) {
                    continue;
                }
                let row = &hierarchy.nodes[child].row;
                if filter.is_none() && row.book_count == 0 && !matches!(root, BrowseLocation::Folder(_)) && !selected_path.contains(&row.id) {
                    continue;
                }
                if parent == 0 {
                    root_sides.push(position % 2 == 0);
                }
                let index = entries.len();
                sources.push(child);
                parents.push(Some(parent));
                let name: SharedString = row.name.clone().into();
                entries.push(GraphEntry { label: label(child, name.clone()), name, location: row.id, parent: Some(parent), depth: entries[parent].depth + 1, branch: if parent == 0 { index } else { entries[parent].branch } });
            }
        }
        parent += 1;
    }
    let selection = entries.iter().position(|entry| entry.location == selected)?;
    let mut ancestor = selection;
    for crumb in path.iter().rev() {
        if entries[ancestor].location != crumb.id {
            return None;
        }
        ancestor = entries[ancestor].parent?;
    }
    if ancestor != 0 {
        return None;
    }
    match GraphTree::new(parents) {
        Ok(tree) => Some((tree, entries, selection, root_sides)),
        Err(error) => {
            log::error!("Could not build browse graph: {error}");
            None
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests {
    use super::*;
    use crate::library::pages::browse::listing::BrowseListing;
    use crate::navigation::LibrarySession;
    use gpui::{AnyWindowHandle, EmptyView, TestAppContext};
    use library_model::{BrowseHierarchyNode, BrowseRow};
    use std::rc::Rc;

    struct GraphFixture {
        graph: Entity<BrowseGraph>,
        kind: BrowseKind,
        path: Vec<BrowsePathSegment>,
    }

    impl Render for GraphFixture {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.graph.update(cx, |graph, cx| graph.render(self.kind, &self.path, window, cx))
        }
    }

    fn draw_graph(graph: &Entity<BrowseGraph>, kind: BrowseKind, path: &[BrowsePathSegment], window: &mut Window, cx: &mut App) {
        window.replace_root(cx, |_, _| GraphFixture { graph: graph.clone(), kind, path: path.to_vec() });
        let _ = window.draw(cx);
    }

    fn row(id: i64) -> BrowseRow {
        BrowseRow { id: BrowseLocation::Subject(id), name: format!("Subject {id}"), path: None, book_count: 1, downloaded_book_count: 0, requested_book_count: None, size_bytes: None }
    }

    #[gpui::test]
    async fn refreshed_graph_replaces_motion_for_the_previous_layout(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library, _app_client) = LibrarySession::test_context(cx).await;
        let window: AnyWindowHandle = cx.add_window(|_, _| EmptyView).into();
        let kind = BrowseKind::Subject;
        let root = kind.root_location();
        let selected = BrowseLocation::Subject(1);
        let path = [BrowsePathSegment { id: selected, name: "Subject 1".into() }];

        // Adding, removing and reordering nodes invalidates animation indices.
        for refreshed in [vec![row(3), row(4), row(5)], vec![row(3)], vec![row(4), row(3)]] {
            let listing = BrowseListing::new(library.clone(), kind, Rc::new(|_, _, _| {}), false);
            let navigator = cx.new(|cx| BrowseNavigator::new(library.clone(), kind, listing, cx));
            let graph = cx.new(|cx| BrowseGraph::new(navigator.clone(), cx));
            let snapshot = |children: Vec<BrowseRow>| {
                let end = children.len() + 3;
                let mut nodes =
                    vec![BrowseHierarchyNode { row: row(0), parent: None, first_child: 1 }, BrowseHierarchyNode { row: row(1), parent: Some(0), first_child: 3 }, BrowseHierarchyNode { row: row(2), parent: Some(0), first_child: end }];
                nodes.extend(children.into_iter().map(|row| BrowseHierarchyNode { row, parent: Some(1), first_child: end }));
                Arc::new(BrowseHierarchy::new(nodes).unwrap())
            };
            window
                .update(cx, |_, window, cx| {
                    graph.update(cx, |graph, cx| {
                        graph.canvas.update(cx, |canvas, _| canvas.bounds = Some(Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)))));
                        graph.install(&Loaded { left: None, refresh: false }, cx);
                        graph.hierarchy = Some(snapshot(vec![row(3), row(4)]));
                    });
                    draw_graph(&graph, kind, &[], window, cx);
                })
                .unwrap();
            if let Some(request) = graph.update(cx, |graph, _| graph.layout_request.take()) {
                request.await;
            }
            window
                .update(cx, |_, window, cx| {
                    draw_graph(&graph, kind, &[], window, cx);
                    navigator.update(cx, |navigator, _| navigator.destination.location = selected);
                    graph.update(cx, |graph, cx| {
                        graph.install(&Loaded { left: Some(root), refresh: false }, cx);
                        graph.center_for = Some(selected);
                    });
                    draw_graph(&graph, kind, &path, window, cx);
                })
                .unwrap();
            if let Some(request) = graph.update(cx, |graph, _| graph.layout_request.take()) {
                request.await;
            }
            window
                .update(cx, |_, window, cx| {
                    draw_graph(&graph, kind, &path, window, cx);
                    let started = graph.read(cx).motion.as_ref().expect("navigation starts an animation").started;
                    draw_graph(&graph, kind, &path, window, cx);
                    assert_eq!(graph.read(cx).motion.as_ref().unwrap().started, started, "an unchanged target keeps its animation");
                    graph.update(cx, |graph, _| {
                        graph.hierarchy = Some(snapshot(refreshed.clone()));
                        graph.invalidate_layout();
                    });
                    draw_graph(&graph, kind, &path, window, cx);
                })
                .unwrap();
            if let Some(request) = graph.update(cx, |graph, _| graph.layout_request.take()) {
                request.await;
            }
            window
                .update(cx, |_, window, cx| {
                    let camera = graph.read(cx).canvas.read(cx).camera;
                    draw_graph(&graph, kind, &path, window, cx);
                    let state = graph.read(cx);
                    assert!(state.motion.is_none(), "a replacement layout must discard the old animation");
                    assert_eq!(state.canvas.read(cx).camera, camera, "a refresh preserves the current camera");
                    let drawn = state.drawn.as_ref().unwrap();
                    let children: Vec<_> = drawn.layout.nodes().iter().filter(|node| node.parent == Some(1)).map(|node| drawn.entries[node.node].location).collect();
                    assert_eq!(children, refreshed.iter().map(|row| row.id).collect::<Vec<_>>());
                })
                .unwrap();
        }
    }
}

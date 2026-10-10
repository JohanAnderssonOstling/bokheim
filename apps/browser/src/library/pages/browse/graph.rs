//! The graph view of a browse page: the root, the path to the location being
//! browsed, and the children of each, on an infinite canvas.
//!
//! The location being browsed is the graph's selection. Tapping or navigating
//! to a node browses it; the graph then moves that node to the camera centre.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::library::widgets::{BookCardInteraction, BookCardPresentation, Paginator, PaginatorAxis, PaginatorGroup, PaginatorPosition, graph_cover_policy};
use gpui::prelude::*;
use gpui::{Context, div};
use gpui_component::IconName;
use std::time::{Duration, Instant};

use gpui::{AnyElement, App, Bounds, Entity, FocusHandle, IntoElement, Pixels, Point, SharedString, Size, Subscription, Window, point, px, size};
use library_model::{BrowseLocation, BrowseRow};
use ui_components::{self as components, CanvasCamera, GraphLayout, GraphLayoutKind, GraphTree, InfiniteCanvas, LayoutParams, NodeRole};

use super::listing::{BrowseCrumb, BrowseKind};
use super::navigator::{BrowseNavigator, Loaded};

const MIN_ZOOM: f32 = 0.3;
const MAX_ZOOM: f32 = 2.5;
const PIN_DURATION: Duration = Duration::from_millis(60);
const MOVE_DURATION: Duration = Duration::from_millis(300);

/// What a tap on the graph asks the page to do.
#[derive(Clone)]
pub(super) enum GraphTap {
    /// Browse this trail's location.
    Select(Vec<BrowseCrumb>),
    /// The selection was tapped again: open it in the cards view.
    Open,
}

/// One node of the graph: what its chip shows, the trail that browses it, and
/// the key that follows it from one layout to the next.
struct GraphEntry {
    name: String,
    books: usize,
    trail: Vec<BrowseCrumb>,
    key: String,
}

impl GraphEntry {
    /// The count a label shows: books in this location and beneath it, if any.
    fn count(&self) -> Option<usize> {
        (self.books > 0).then_some(self.books)
    }
}

/// A layout and the entry for each of its nodes.
struct Drawn {
    layout: Arc<GraphLayout>,
    entries: Arc<Vec<GraphEntry>>,
}

/// Everything a layout is computed from: the graph is laid out again only
/// when one of these changes.
#[derive(PartialEq)]
struct GraphKey {
    root: BrowseLocation,
    root_label: String,
    root_book_count: usize,
    path: Vec<BrowseCrumb>,
    compact: bool,
    rem_size: Pixels,
    kind: GraphLayoutKind,
}

/// The settled layout for a key, which the drawn one animates towards.
struct CachedGraph {
    key: GraphKey,
    target: Drawn,
}

struct GraphMotion {
    started: Instant,
    from: Vec<Point<f32>>,
    appearing: Vec<bool>,
    from_camera: CanvasCamera,
    last_camera: CanvasCamera,
    target_camera: CanvasCamera,
    selected_key: String,
    anchor: Point<Pixels>,
    departing: GraphLayout,
    /// The entries the departing layout's nodes index.
    departing_entries: Arc<Vec<GraphEntry>>,
    ghost_opacity: f32,
}

pub(super) struct BrowseGraph {
    canvas: Entity<InfiniteCanvas>,
    navigator: Entity<BrowseNavigator>,
    /// The location's own books, beside the graph.
    covers: Entity<Paginator>,
    covers_focus: FocusHandle,
    covers_focus_subscription: Option<Subscription>,
    cover_positions: HashMap<BrowseLocation, PaginatorPosition>,
    sheet_expanded: bool,
    sidebar_width: f32,
    sidebar_drag: Option<(f32, f32)>,
    layout_kind: GraphLayoutKind,
    /// The children of the page's location and of every location above it.
    children: HashMap<BrowseLocation, Vec<BrowseRow>>,
    target: Option<CachedGraph>,
    drawn: Option<Drawn>,
    /// The next selected location to centre after its children have loaded.
    center_for: Option<BrowseLocation>,
    /// The selection the camera was last placed for.
    placed_for: Option<BrowseLocation>,
    motion: Option<GraphMotion>,
    _loaded: Subscription,
}

impl gpui::EventEmitter<GraphTap> for BrowseGraph {}

impl BrowseGraph {
    pub(super) fn new(navigator: Entity<BrowseNavigator>, layout_kind: GraphLayoutKind, cx: &mut Context<Self>) -> Self {
        let covers_id = format!("{}-graph-covers", navigator.read(cx).kind.list_prefix());
        let loaded = cx.subscribe(&navigator, |graph, _, loaded: &Loaded, cx| graph.install(loaded, cx));
        Self {
            navigator,
            covers: cx.new(|_| Paginator::new(covers_id, []).flow_axis(PaginatorAxis::Vertical)),
            covers_focus: cx.focus_handle(),
            covers_focus_subscription: None,
            cover_positions: HashMap::new(),
            sheet_expanded: false,
            sidebar_width: 380.0,
            sidebar_drag: None,
            canvas: cx.new(|_| InfiniteCanvas::new(MIN_ZOOM, MAX_ZOOM)),
            layout_kind,
            children: HashMap::new(),
            target: None,
            drawn: None,
            center_for: None,
            placed_for: None,
            motion: None,
            _loaded: loaded,
        }
    }

    /// Takes the navigator's new listing: the children of its location and of
    /// every location above it, and the location's books for the covers.
    fn install(&mut self, loaded: &Loaded, cx: &mut Context<Self>) {
        if let Some(left) = loaded.left {
            self.cover_positions.insert(left, self.covers.read(cx).position());
        }
        let navigator = self.navigator.read(cx);
        let location = navigator.destination.location;
        let mut children: HashMap<_, _> = navigator.listing.ancestors.iter().cloned().collect();
        children.insert(location, navigator.listing.children.to_vec());
        let books = navigator.listing.books.clone();
        let groups = if books.is_empty() { Vec::new() } else { vec![PaginatorGroup::new(graph_cover_policy(), books.iter().cloned().map(|book| super::layout::book_child(book, BookCardPresentation::GraphCover, |_, _, _| BookCardInteraction::default(), None, cx)))] };
        self.children = children;
        self.target = None;
        let restored = loaded.left.and_then(|_| self.cover_positions.get(&location).cloned());
        let refresh = loaded.refresh;
        self.covers.update(cx, |covers, cx| {
            if refresh {
                covers.refresh_groups(groups, cx);
            } else {
                covers.set_groups(groups, cx);
            }
            if let Some(position) = &restored {
                covers.restore_position(position, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn layout_control(&self, cx: &Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let (icon, next_label) = match self.layout_kind() {
            components::GraphLayoutKind::Columns => (IconName::GraphColumns, "Switch to circle graph"),
            components::GraphLayoutKind::AdaptiveRadial => (IconName::GraphCircle, "Switch to column graph"),
        };
        components::outlined_icon_button("toggle-graph-layout", next_label, icon, theme)
            .on_click(cx.listener(|graph, _, _, cx| {
                graph.toggle_layout();
                cx.notify();
            }))
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
        let width = (start_width + start_x - x).clamp(minimum, 720.0_f32.max(minimum));
        if (self.sidebar_width - width).abs() < 0.5 {
            return false;
        }
        self.sidebar_width = width;
        true
    }

    pub(super) fn render_panel(
        &mut self, root_book_count: usize, controls: AnyElement, search_field: Option<gpui::Div>, window: &mut Window, cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = components::browser_theme(cx);
        let navigator = self.navigator.read(cx);
        let (kind, path, has_books) = (navigator.kind, navigator.destination.path.clone(), !navigator.listing.books.is_empty());
        let graph = self.render(kind, root_book_count, &path, window, cx);
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        if self.covers_focus_subscription.is_none() {
            self.covers_focus_subscription = Some(cx.on_focus_in(&self.covers_focus.clone(), window, |graph, _, cx| {
                if graph.set_sheet_expanded(true) { cx.notify(); }
            }));
        }
        let covers = div().id("graph-covers-focus").track_focus(&self.covers_focus).size_full().child(self.covers.clone()).into_any_element();
        let graph_area = div().relative().flex_1().min_h_0().min_w_0().child(graph).child(div().absolute().right(px(8.0)).bottom(px(8.0)).child(controls));
        // The grid holds the selection's own books, so with none it is
        // not drawn at all rather than drawn empty. An open search keeps
        // it: the field lives there, and a query with no matches yet
        // must not take the field away mid-word.
        let has_covers = has_books || search_field.is_some();
        if !has_covers {
            return graph_area.size_full().into_any_element();
        }
        if compact {
            let expanded = self.sheet_expanded;
            let cover_sheet = div()
                .relative()
                .w_full()
                .when(expanded, |sheet| sheet.flex_1().min_h_0())
                .when(!expanded, |sheet| sheet.h(px(117.0)).flex_none())
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
            let sidebar = div()
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .children(search_field.map(|field| div().w_full().px(px(components::CONTENT_INSET)).pt(px(components::CONTENT_INSET)).pb(px(components::CONTENT_INSET)).child(field)))
                .child(div().flex_1().min_h_0().child(covers));
            let sidebar_width = self.sidebar_width;
            div()
                .relative()
                .size_full()
                .flex()
                .flex_row()
                .child(graph_area)
                .child(div().relative().flex_none().w(px(sidebar_width)).max_w(gpui::relative(0.65)).min_h_0().child(sidebar).child(
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

    pub(super) fn toggle_layout(&mut self) -> bool {
        self.set_layout_kind(match self.layout_kind {
            GraphLayoutKind::Columns => GraphLayoutKind::AdaptiveRadial,
            GraphLayoutKind::AdaptiveRadial => GraphLayoutKind::Columns,
        });
        true
    }

    pub(super) fn handle_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if key == "l" {
            self.toggle_layout();
            cx.notify();
            return true;
        }
        if let Some(action) = self.key(key) {
            cx.emit(action);
            return true;
        }
        matches!(key, "left" | "right" | "up" | "down" | "enter" | "backspace" | "pageup" | "pagedown")
    }

    pub(super) fn layout_kind(&self) -> GraphLayoutKind {
        self.layout_kind
    }

    pub(super) fn set_layout_kind(&mut self, kind: GraphLayoutKind) {
        if self.layout_kind == kind {
            return;
        }
        self.layout_kind = kind;
        let root = self.target.take().map(|target| target.key.root);
        self.placed_for = None;
        self.center_for = self.drawn.as_ref().and_then(|drawn| drawn.layout.path.last().and_then(|&id| drawn.entries[id].trail.last().map(|crumb| crumb.location).or(root)));
    }

    /// Arrows walk the hierarchy and its ordered column. Page Up/Down (also
    /// sent by Android's volume buttons) move to the parent/first child.
    /// Enter opens the selected location in cards.
    pub(super) fn key(&mut self, key: &str) -> Option<GraphTap> {
        let drawn = self.drawn.as_ref()?;
        let selection = *drawn.layout.path.last()?;
        let selected_node = drawn.layout.node(selection)?;
        let parent = selected_node.parent;
        if key == "pageup" {
            return self.activate(parent?);
        }
        if key == "pagedown" {
            let child = drawn.layout.nodes.iter().find(|placed| placed.parent == Some(selection))?.node;
            return self.activate(child);
        }
        if drawn.layout.kind == GraphLayoutKind::AdaptiveRadial {
            let children = || drawn.layout.nodes.iter().find(|placed| placed.parent == Some(selection)).map(|placed| placed.node);
            let node = match key {
                "left" => parent?,
                "right" => children()?,
                "up" | "down" => {
                    let parent = parent?;
                    let siblings: Vec<usize> = drawn.layout.nodes.iter().filter(|placed| placed.parent == Some(parent)).map(|placed| placed.node).collect();
                    let index = siblings.iter().position(|&node| node == selection)?;
                    siblings[if key == "down" { (index + 1) % siblings.len() } else { (index + siblings.len() - 1) % siblings.len() }]
                }
                "enter" => return Some(GraphTap::Open),
                _ => return None,
            };
            return self.activate(node);
        }
        let side = if selected_node.angle.cos() < 0.0 { -1.0 } else { 1.0 };
        let first_child = |direction: f32| drawn.layout.nodes.iter().find(|placed| placed.parent == Some(selection) && placed.angle.cos() * direction > 0.0).map(|placed| placed.node);
        let node = match key {
            "left" if selection == 0 => first_child(-1.0)?,
            "right" if selection == 0 => first_child(1.0)?,
            "left" if side < 0.0 => first_child(side)?,
            "right" if side < 0.0 => parent?,
            "left" => parent?,
            "right" => first_child(side)?,
            "up" | "down" => {
                let siblings: Vec<usize> = drawn.layout.nodes.iter().filter(|placed| placed.parent == parent && parent.is_some() && placed.angle.cos() * side > 0.0).map(|placed| placed.node).collect();
                let index = siblings.iter().position(|&node| node == selection)?;
                let next = if key == "down" { (index + 1) % siblings.len() } else { (index + siblings.len() - 1) % siblings.len() };
                siblings[next]
            }
            "enter" => return Some(GraphTap::Open),
            _ => return None,
        };
        if node == selection {
            return None;
        }
        self.activate(node)
    }

    fn activate(&mut self, node: usize) -> Option<GraphTap> {
        let drawn = self.drawn.as_ref()?;
        if Some(&node) == drawn.layout.path.last() {
            return Some(GraphTap::Open);
        }
        drawn.layout.node(node)?;
        let trail = drawn.entries[node].trail.clone();
        let location = trail.last().map(|crumb| crumb.location).or(self.target.as_ref().map(|target| target.key.root));
        self.center_for = location;
        self.motion = None;
        Some(GraphTap::Select(trail))
    }

    /// The graph for `path` below `root`. Until every column along the path is
    /// known, the last graph drawn stays up.
    pub(super) fn render(&mut self, kind: BrowseKind, root_book_count: usize, path: &[BrowseCrumb], window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (root, root_label) = (kind.root_location(), kind.root_label());
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let params = if compact { LayoutParams::compact() } else { LayoutParams::desktop() };
        let key = GraphKey { root, root_label: root_label.to_string(), root_book_count, path: path.to_vec(), compact, rem_size: window.rem_size(), kind: self.layout_kind };
        let target_changed = !self.target.as_ref().is_some_and(|target| target.key == key);
        if target_changed {
            self.target = None;
            if let Some((tree, entries, selection)) = graph_tree(root, &root_label, root_book_count, path, &self.children) {
                // Each label is measured in every role, since the layout picks
                // the role and fits column slots to all of them.
                let sizes: HashMap<(usize, NodeRole), Size<f32>> = entries
                    .iter()
                    .enumerate()
                    .flat_map(|(node, entry)| NodeRole::ALL.map(|role| (node, role, entry)))
                    .map(|(node, role, entry)| {
                        let measured = components::graph_label_size(role, &entry.name, entry.count(), compact, window, cx);
                        ((node, role), size(f32::from(measured.width), f32::from(measured.height)))
                    })
                    .collect();
                let layout = components::graph_layout(self.layout_kind, &tree, selection, &|node, role| sizes[&(node, role)], &params);
                self.target = Some(CachedGraph { key, target: Drawn { layout: Arc::new(layout), entries: Arc::new(entries) } });
            }
        }
        if let Some(CachedGraph { target, .. }) = &self.target {
            let layout = Arc::clone(&target.layout);
            let entries = Arc::clone(&target.entries);
            let location = path.last().map_or(root, |crumb| crumb.location);
            let center_new = self.center_for == Some(location);
            if self.placed_for != Some(location) || center_new {
                self.place_camera(&layout, &entries, location, window, cx);
            }
            if target_changed || self.motion.is_some() || self.drawn.is_none() {
                let layout = Arc::new(self.animated_layout(&layout, &entries, window, cx));
                self.drawn = Some(Drawn { layout, entries });
            }
        }
        let Some(drawn) = &self.drawn else {
            return components::infinite_canvas(&self.canvas).into_any_element();
        };
        let ghost = self.motion.as_ref().filter(|motion| !motion.departing.nodes.is_empty() && motion.ghost_opacity > 0.0);
        let theme = components::browser_theme(cx);
        let graph = cx.weak_entity();
        // A tap browses the node, unless the press that ended on it was a pan;
        // a fading node only shows.
        let node = |placed: &components::PlacedNode, departing: bool| {
            let entries = if departing { &ghost.expect("a departing node belongs to the motion").departing_entries } else { &drawn.entries };
            let entry = &entries[placed.node];
            let label = components::graph_label(SharedString::from(format!("graph-node-{}", entry.key)), placed.role, &entry.name, entry.count(), compact, theme);
            if departing {
                return label.into_any_element();
            }
            let (graph, canvas, index) = (graph.clone(), self.canvas.clone(), placed.node);
            label.on_click(move |_, _, cx| {
                if canvas.read(cx).panned() {
                    return;
                }
                let _ = graph.update(cx, |graph, cx| {
                    if let Some(tap) = graph.activate(index) {
                        cx.emit(tap);
                    }
                });
            })
            .into_any_element()
        };
        components::graph_canvas(&self.canvas, &drawn.layout, ghost.map(|motion| (&motion.departing, motion.ghost_opacity)), node, cx).into_any_element()
    }

    /// Centres a newly selected node, or frames the initial selection.
    /// Camera placement waits for the canvas's first layout.
    fn place_camera(&mut self, layout: &GraphLayout, entries: &[GraphEntry], location: BrowseLocation, window: &Window, cx: &mut App) {
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let center_new = self.center_for == Some(location);
        let selection = *layout.path.last().expect("the path always holds the root");
        let Some(bounds) = self.canvas.read(cx).bounds else {
            window.request_animation_frame();
            return;
        };
        let camera = if center_new {
            let anchor = point(bounds.size.width / 2.0, bounds.size.height / 2.0);
            let zoom = self.canvas.read(cx).camera.zoom;
            components::anchored_camera(layout, selection, anchor, zoom).expect("the selected node is placed")
        } else {
            components::framing_camera(layout, Bounds::new(point(px(0.0), px(0.0)), bounds.size), compact, 1.0)
        };
        let from_camera = self.canvas.read(cx).camera;
        let selected_key = &entries[selection].key;
        let previous = self.drawn.as_ref().and_then(|drawn| drawn.layout.nodes.iter().find(|placed| drawn.entries[placed.node].key == *selected_key).map(|placed| (drawn, placed.center)));
        if center_new && !cx.reduce_motion() && !cfg!(feature = "kobo") {
            if let Some((drawn, old_center)) = previous {
                let old_centers: HashMap<&str, Point<f32>> = drawn.layout.nodes.iter().map(|placed| (drawn.entries[placed.node].key.as_str(), placed.center)).collect();
                let mut from = vec![point(0.0, 0.0); entries.len()];
                let mut appearing = vec![false; entries.len()];
                for placed in &layout.nodes {
                    let key = entries[placed.node].key.as_str();
                    from[placed.node] = old_centers.get(key).copied().or_else(|| placed.parent.and_then(|parent| old_centers.get(entries[parent].key.as_str()).copied())).unwrap_or(placed.center);
                    appearing[placed.node] = !old_centers.contains_key(key);
                }
                let anchor = from_camera.to_screen(point(px(old_center.x), px(old_center.y)));
                let visible: HashSet<&str> = entries.iter().map(|entry| entry.key.as_str()).collect();
                let mut departing = GraphLayout::clone(&drawn.layout);
                departing.retain_nodes(|placed| !visible.contains(drawn.entries[placed.node].key.as_str()));
                Arc::make_mut(&mut departing.edges).retain(|edge| !visible.contains(drawn.entries[edge.to].key.as_str()));
                self.motion = Some(GraphMotion { started: Instant::now(), from, appearing, from_camera, last_camera: from_camera, target_camera: camera, selected_key: selected_key.clone(), anchor, departing, departing_entries: Arc::clone(&drawn.entries), ghost_opacity: 1.0 });
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
        let Some(motion) = &mut self.motion else {
            return layout.clone();
        };
        let selection = *layout.path.last().expect("the path always holds the root");
        if entries[selection].key != motion.selected_key || self.canvas.read(cx).camera != motion.last_camera {
            // A wheel, pinch, or drag takes control of the camera immediately.
            self.motion = None;
            return layout.clone();
        }
        let elapsed = motion.started.elapsed();
        motion.ghost_opacity = 1.0 - (elapsed.as_secs_f32() / 0.12).clamp(0.0, 1.0);
        let progress = (elapsed.saturating_sub(PIN_DURATION).as_secs_f32() / MOVE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let mut centers = vec![point(0.0, 0.0); entries.len()];
        let mut opacity = vec![1.0; entries.len()];
        for placed in &layout.nodes {
            let old = motion.from[placed.node];
            centers[placed.node] = point(old.x + (placed.center.x - old.x) * progress, old.y + (placed.center.y - old.y) * progress);
            if motion.appearing[placed.node] {
                opacity[placed.node] = progress;
            }
        }
        let posed = layout.with_pose(&centers, &opacity);
        let selected = posed.node(selection).expect("selected node is placed").center;
        let target = layout.node(selection).expect("selected node is placed").center;
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

fn trail_key(root: BrowseLocation, trail: &[BrowseCrumb]) -> String {
    let mut key = root.to_string();
    for crumb in trail {
        key.push('\0');
        key.push_str(&crumb.location.to_string());
    }
    key
}

/// The tree the graph draws: the root, the children of the root and of every
/// location on `path`, and the index of the path's last location. `None` while
/// any of those locations' children are unknown.
fn graph_tree(root: BrowseLocation, root_label: &str, root_book_count: usize, path: &[BrowseCrumb], children: &HashMap<BrowseLocation, Vec<BrowseRow>>) -> Option<(GraphTree, Vec<GraphEntry>, usize)> {
    let entry = |name: &str, books: usize, trail: Vec<BrowseCrumb>| GraphEntry { name: name.to_owned(), books, key: trail_key(root, &trail), trail };
    let mut parents = vec![None];
    let mut entries = vec![entry(root_label, root_book_count, Vec::new())];
    let mut parent = 0;
    let mut location = root;
    for depth in 0..=path.len() {
        let next = path.get(depth);
        let mut next_index = None;
        for row in children.get(&location)? {
            if next.is_some_and(|crumb| crumb.location == row.id) {
                next_index = Some(entries.len());
            }
            let trail = [entries[parent].trail.clone(), vec![BrowseCrumb { location: row.id, label: row.name.clone() }]].concat();
            parents.push(Some(parent));
            entries.push(entry(&row.name, row.book_count, trail));
        }
        let Some(crumb) = next else { break };
        // A location reached some other way than through its parent's children
        // — a search hit's trail — still belongs on the path.
        parent = next_index.unwrap_or_else(|| {
            let trail = [entries[parent].trail.clone(), vec![crumb.clone()]].concat();
            parents.push(Some(parent));
            entries.push(entry(&crumb.label, 0, trail));
            entries.len() - 1
        });
        location = crumb.location;
    }
    Some((GraphTree::new(parents), entries, parent))
}

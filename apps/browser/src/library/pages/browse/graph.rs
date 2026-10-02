//! The graph view of a browse page: the root, the path to the location being
//! browsed, and the children of each, on an infinite canvas.
//!
//! The location being browsed is the graph's selection. Tapping or navigating
//! to a node browses it; the graph then moves that node to the camera centre.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use super::controls::BrowseOptionsSelection;
use crate::library::widgets::{BookCardInteraction, BookCardPresentation, BookCardView, Paginator, PaginatorAxis, PaginatorGroup, graph_cover_policy};
use gpui::prelude::*;
use gpui::{Context, div};
use gpui_component::IconName;
use std::time::{Duration, Instant};

use gpui::{AnyElement, App, Bounds, Entity, IntoElement, Pixels, Point, Window, point, px};
use library_model::{BrowseLocation, BrowseRow};
use ui_components::{self as components, CanvasCamera, GraphLayout, GraphLayoutKind, GraphMetrics, GraphNode, GraphTree, GraphType, InfiniteCanvas, LayoutParams};

use super::listing::{BrowseCrumb, BrowseDestination, BrowseKind};

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

/// A drawn layout and, for each of its nodes, the trail that browses it.
struct Drawn {
    layout: GraphLayout,
    trails: Arc<Vec<Vec<BrowseCrumb>>>,
    keys: Arc<Vec<String>>,
}

struct CachedGraph {
    root: BrowseLocation,
    root_label: String,
    root_book_count: usize,
    path: Vec<BrowseCrumb>,
    compact: bool,
    rem_size: Pixels,
    kind: GraphLayoutKind,
    layout: Arc<GraphLayout>,
    trails: Arc<Vec<Vec<BrowseCrumb>>>,
    keys: Arc<Vec<String>>,
}

impl CachedGraph {
    fn matches(&self, root: BrowseLocation, root_label: &str, root_book_count: usize, path: &[BrowseCrumb], compact: bool, rem_size: Pixels, kind: GraphLayoutKind) -> bool {
        self.root == root && self.root_label == root_label && self.root_book_count == root_book_count && self.path == path && self.compact == compact && self.rem_size == rem_size && self.kind == kind
    }
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
    ghost_opacity: f32,
}

pub(super) struct BrowseGraph {
    canvas: Entity<InfiniteCanvas>,
    covers: Entity<Paginator>,
    sheet_expanded: bool,
    sidebar_width: f32,
    sidebar_drag: Option<(f32, f32)>,
    layout_kind: GraphLayoutKind,
    /// The children of every location loaded, so the path and its sibling fans
    /// draw without asking again.
    children: HashMap<BrowseLocation, Vec<BrowseRow>>,
    /// Locations whose children are being fetched.
    fetching: HashSet<BrowseLocation>,
    target: Option<CachedGraph>,
    drawn: Option<Drawn>,
    hovered: Option<usize>,
    /// The next selected location to centre after its children have loaded.
    center_for: Option<BrowseLocation>,
    /// The selection the camera was last placed for.
    placed_for: Option<BrowseLocation>,
    motion: Option<GraphMotion>,
}

impl gpui::EventEmitter<GraphTap> for BrowseGraph {}

impl BrowseGraph {
    pub(super) fn new(layout_kind: GraphLayoutKind, covers_id: String, cx: &mut App) -> Self {
        Self {
            covers: cx.new(|_| Paginator::new(covers_id, []).flow_axis(PaginatorAxis::Vertical)),
            sheet_expanded: false,
            sidebar_width: 380.0,
            sidebar_drag: None,
            canvas: cx.new(|_| InfiniteCanvas::new(MIN_ZOOM, MAX_ZOOM)),
            layout_kind,
            children: HashMap::new(),
            fetching: HashSet::new(),
            target: None,
            drawn: None,
            hovered: None,
            center_for: None,
            placed_for: None,
            motion: None,
        }
    }

    pub(super) fn book_groups(&self, books: Rc<Vec<Entity<BookCardView>>>, cx: &App) -> Vec<PaginatorGroup> {
        if books.is_empty() {
            return Vec::new();
        }
        let children = books.iter().cloned().map(|book| super::layout::book_child(book, BookCardPresentation::GraphCover, |_, _, _| BookCardInteraction::default(), None, cx));
        vec![PaginatorGroup::new(graph_cover_policy(), children)]
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

    pub(super) fn covers(&self) -> Entity<Paginator> {
        self.covers.clone()
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
        &mut self, kind: BrowseKind, destination: &BrowseDestination, root_book_count: usize, controls: AnyElement, search_field: Option<gpui::Div>, has_books: bool, window: &mut Window, cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = components::browser_theme(cx);
        let on_tap = cx.listener(|graph, world: &Point<Pixels>, _, cx| {
            if let Some(action) = graph.tap(*world, cx) {
                cx.emit(action);
            }
        });
        let on_hover = cx.listener(|graph, world: &Option<Point<Pixels>>, _, cx| {
            if graph.hover(*world, cx) {
                cx.notify();
            }
        });
        let graph = self.render(kind.root_location(), &kind.root_label(), root_book_count, &destination.path, move |world, window, cx| on_tap(&world, window, cx), move |world, window, cx| on_hover(&world, window, cx), window, cx);
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let covers = self.covers.clone().into_any_element();
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
                                if let gpui::ScrollDelta::Pixels(delta) = event.delta {
                                    if f32::from(delta.y) < -3.0 {
                                        if graph.set_sheet_expanded(true) {
                                            cx.notify();
                                        }
                                    }
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
                            if let gpui::ScrollDelta::Pixels(delta) = event.delta {
                                if f32::from(delta.y) > 3.0 {
                                    if graph.set_sheet_expanded(false) {
                                        cx.notify();
                                    }
                                }
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

    pub(super) fn fetch_path(&mut self, kind: BrowseKind, destination: &BrowseDestination, selection: &BrowseOptionsSelection, library: &library_backend::LibraryClient, cx: &mut Context<Self>) {
        for location in self.take_missing(kind.root_location(), &destination.path) {
            let query = selection.backend_query(location, false);
            let operation = super::listing::request(library, query);
            cx.spawn(async move |graph, cx| {
                let result = operation.await;
                let _ = graph.update(cx, |graph, cx| {
                    match result {
                        Ok(response) => graph.remember(location, response.data.sections.into_iter().map(|section| section.parent).collect()),
                        Err(error) => {
                            log::warn!("Graph fetch failed for {location}: {error}");
                            graph.forget_fetch(location);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
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
        let root = self.target.take().map(|target| target.root);
        self.placed_for = None;
        self.hovered = None;
        self.center_for = self.drawn.as_ref().and_then(|drawn| drawn.layout.path.last().and_then(|&id| drawn.trails[id].last().map(|crumb| crumb.location).or(root)));
    }

    pub(super) fn remember(&mut self, location: BrowseLocation, children: Vec<BrowseRow>) {
        self.fetching.remove(&location);
        self.children.insert(location, children);
        self.target = None;
    }

    /// Locations on the way to the selection whose children are neither known
    /// nor being fetched. Each is marked as being fetched.
    pub(super) fn take_missing(&mut self, root: BrowseLocation, path: &[BrowseCrumb]) -> Vec<BrowseLocation> {
        let locations = std::iter::once(root).chain(path.iter().map(|crumb| crumb.location));
        let missing: Vec<BrowseLocation> = locations.filter(|location| !self.children.contains_key(location) && !self.fetching.contains(location)).collect();
        self.fetching.extend(missing.iter().copied());
        missing
    }

    /// A fetch that failed can be tried again the next time the graph is drawn.
    pub(super) fn forget_fetch(&mut self, location: BrowseLocation) {
        self.fetching.remove(&location);
    }

    pub(super) fn tap(&mut self, world: Point<Pixels>, cx: &App) -> Option<GraphTap> {
        let drawn = self.drawn.as_ref()?;
        let camera = self.canvas.read(cx).camera;
        let node = drawn.layout.hit_test(point(f32::from(world.x), f32::from(world.y)), camera.zoom)?;
        self.activate(node)
    }

    /// Updates the label under the pointer. Repaint only when it changes.
    pub(super) fn hover(&mut self, world: Option<Point<Pixels>>, cx: &App) -> bool {
        let hovered = world.and_then(|world| {
            let drawn = self.drawn.as_ref()?;
            let zoom = self.canvas.read(cx).camera.zoom;
            drawn.layout.hit_test(point(f32::from(world.x), f32::from(world.y)), zoom)
        });
        if self.hovered == hovered {
            return false;
        }
        self.hovered = hovered;
        true
    }

    /// Arrows walk the hierarchy and its ordered column. Page Up/Down (also
    /// sent by Android's volume buttons) move to the parent/first child.
    /// Enter opens the selected location in cards.
    pub(super) fn key(&mut self, key: &str) -> Option<GraphTap> {
        let drawn = self.drawn.as_ref()?;
        let selection = *drawn.layout.path.last()?;
        let selected_label = drawn.layout.label(selection)?;
        let parent = selected_label.parent;
        if key == "pageup" {
            return self.activate(parent?);
        }
        if key == "pagedown" {
            let child = drawn.layout.labels.iter().find(|label| label.parent == Some(selection))?.node;
            return self.activate(child);
        }
        if drawn.layout.kind == GraphLayoutKind::AdaptiveRadial {
            let children = || drawn.layout.labels.iter().find(|label| label.parent == Some(selection)).map(|label| label.node);
            let node = match key {
                "left" => parent?,
                "right" => children()?,
                "up" | "down" => {
                    let parent = parent?;
                    let siblings: Vec<usize> = drawn.layout.labels.iter().filter(|label| label.parent == Some(parent)).map(|label| label.node).collect();
                    let index = siblings.iter().position(|&node| node == selection)?;
                    siblings[if key == "down" { (index + 1) % siblings.len() } else { (index + siblings.len() - 1) % siblings.len() }]
                }
                "enter" => return Some(GraphTap::Open),
                _ => return None,
            };
            return self.activate(node);
        }
        let side = if selected_label.angle.cos() < 0.0 { -1.0 } else { 1.0 };
        let first_child = |direction: f32| drawn.layout.labels.iter().find(|label| label.parent == Some(selection) && label.angle.cos() * direction > 0.0).map(|label| label.node);
        let node = match key {
            "left" if selection == 0 => first_child(-1.0)?,
            "right" if selection == 0 => first_child(1.0)?,
            "left" if side < 0.0 => first_child(side)?,
            "right" if side < 0.0 => parent?,
            "left" => parent?,
            "right" => first_child(side)?,
            "up" | "down" => {
                let siblings: Vec<usize> = drawn.layout.labels.iter().filter(|label| label.parent == parent && parent.is_some() && label.angle.cos() * side > 0.0).map(|label| label.node).collect();
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
        drawn.layout.label(node)?;
        let trail = drawn.trails[node].clone();
        let location = trail.last().map(|crumb| crumb.location).or(self.target.as_ref().map(|target| target.root));
        self.hovered = None;
        self.center_for = location;
        self.motion = None;
        Some(GraphTap::Select(trail))
    }

    /// The graph for `path` below `root`. Until every column along the path is
    /// known, the last graph drawn stays up.
    pub(super) fn render(
        &mut self, root: BrowseLocation, root_label: &str, root_book_count: usize, path: &[BrowseCrumb], on_tap: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static, on_hover: impl Fn(Option<Point<Pixels>>, &mut Window, &mut App) + 'static,
        window: &mut Window, cx: &mut App,
    ) -> AnyElement {
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let (params, sizes) = if compact { (LayoutParams::compact(), GraphType::compact()) } else { (LayoutParams::desktop(), GraphType::desktop()) };
        let target_changed = !self.target.as_ref().is_some_and(|target| target.matches(root, root_label, root_book_count, path, compact, window.rem_size(), self.layout_kind));
        if target_changed {
            self.target = graph_tree(root, root_label, root_book_count, path, &self.children).map(|(tree, trails, selection)| {
                let keys: Vec<String> = trails.iter().map(|trail| trail_key(root, trail)).collect();
                let metrics = GraphMetrics { window, sizes };
                let layout = match self.layout_kind {
                    GraphLayoutKind::Columns => components::graph_layout(&tree, selection, &params, &metrics),
                    GraphLayoutKind::AdaptiveRadial => components::radial_graph_layout(&tree, selection, &params, &metrics),
                };
                CachedGraph {
                    root,
                    root_label: root_label.to_owned(),
                    root_book_count,
                    path: path.to_vec(),
                    compact,
                    rem_size: window.rem_size(),
                    kind: self.layout_kind,
                    layout: Arc::new(layout),
                    trails: Arc::new(trails),
                    keys: Arc::new(keys),
                }
            });
        }
        if let Some(target) = &self.target {
            let layout = Arc::clone(&target.layout);
            let trails = Arc::clone(&target.trails);
            let keys = Arc::clone(&target.keys);
            let location = path.last().map_or(root, |crumb| crumb.location);
            let center_new = self.center_for == Some(location);
            if self.placed_for != Some(location) || center_new {
                self.place_camera(&layout, &keys, location, compact, center_new, window, cx);
            }
            if target_changed || self.motion.is_some() || self.drawn.is_none() {
                let layout = self.animated_layout(&layout, &keys, window, cx);
                self.drawn = Some(Drawn { layout, trails, keys });
            }
        }
        let Some(drawn) = &self.drawn else {
            return components::infinite_canvas(&self.canvas).into_any_element();
        };
        let ghost = self.motion.as_ref().and_then(|motion| {
            if motion.departing.labels.is_empty() || motion.ghost_opacity <= 0.0 {
                return None;
            }
            Some((&motion.departing, motion.ghost_opacity))
        });
        components::graph_scene(&self.canvas, &drawn.layout, ghost, self.hovered, sizes, window, cx).on_tap(on_tap).on_hover(on_hover).into_any_element()
    }

    /// Centres a newly selected node, or frames the initial selection.
    /// Camera placement waits for the canvas's first layout.
    fn place_camera(&mut self, layout: &GraphLayout, keys: &[String], location: BrowseLocation, compact: bool, center_new: bool, window: &Window, cx: &mut App) {
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
        let selected_key = &keys[selection];
        let previous = self.drawn.as_ref().and_then(|drawn| drawn.layout.labels.iter().find(|label| drawn.keys[label.node] == *selected_key).map(|label| (drawn, label.center)));
        if center_new && !cx.reduce_motion() && !cfg!(feature = "kobo") {
            if let Some((drawn, old_center)) = previous {
                let old_centers: HashMap<&str, Point<f32>> = drawn.layout.labels.iter().map(|label| (drawn.keys[label.node].as_str(), label.center)).collect();
                let mut from = vec![point(0.0, 0.0); keys.len()];
                let mut appearing = vec![false; keys.len()];
                for label in &layout.labels {
                    let key = keys[label.node].as_str();
                    from[label.node] = old_centers.get(key).copied().or_else(|| label.parent.and_then(|parent| old_centers.get(keys[parent].as_str()).copied())).unwrap_or(label.center);
                    appearing[label.node] = !old_centers.contains_key(key);
                }
                let anchor = from_camera.to_screen(point(px(old_center.x), px(old_center.y)));
                let visible: HashSet<&str> = keys.iter().map(String::as_str).collect();
                let mut departing = drawn.layout.clone();
                departing.retain_labels(|label| !visible.contains(drawn.keys[label.node].as_str()));
                Arc::make_mut(&mut departing.edges).retain(|edge| !visible.contains(drawn.keys[edge.to].as_str()));
                self.motion = Some(GraphMotion { started: Instant::now(), from, appearing, from_camera, last_camera: from_camera, target_camera: camera, selected_key: selected_key.clone(), anchor, departing, ghost_opacity: 1.0 });
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

    fn animated_layout(&mut self, layout: &GraphLayout, keys: &[String], window: &Window, cx: &mut App) -> GraphLayout {
        let Some(motion) = &mut self.motion else {
            return layout.clone();
        };
        let selection = *layout.path.last().expect("the path always holds the root");
        if keys[selection] != motion.selected_key || self.canvas.read(cx).camera != motion.last_camera {
            // A wheel, pinch, or drag takes control of the camera immediately.
            self.motion = None;
            return layout.clone();
        }
        let elapsed = motion.started.elapsed();
        motion.ghost_opacity = 1.0 - (elapsed.as_secs_f32() / 0.12).clamp(0.0, 1.0);
        let progress = (elapsed.saturating_sub(PIN_DURATION).as_secs_f32() / MOVE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let mut centers = vec![point(0.0, 0.0); keys.len()];
        let mut opacity = vec![1.0; keys.len()];
        for label in &layout.labels {
            let old = motion.from[label.node];
            centers[label.node] = point(old.x + (label.center.x - old.x) * progress, old.y + (label.center.y - old.y) * progress);
            if motion.appearing[label.node] {
                opacity[label.node] = progress;
            }
        }
        let posed = layout.with_pose(&centers, &opacity);
        let selected = posed.label(selection).expect("selected node is placed").center;
        let target = layout.label(selection).expect("selected node is placed").center;
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
fn graph_tree(root: BrowseLocation, root_label: &str, root_book_count: usize, path: &[BrowseCrumb], children: &HashMap<BrowseLocation, Vec<BrowseRow>>) -> Option<(GraphTree, Vec<Vec<BrowseCrumb>>, usize)> {
    let node = |name: &str, parent, books| GraphNode { name: name.to_owned(), parent, children: Vec::new(), books };
    let mut nodes = vec![node(root_label, None, root_book_count)];
    let mut trails: Vec<Vec<BrowseCrumb>> = vec![Vec::new()];
    let mut parent = 0;
    let mut location = root;
    for depth in 0..=path.len() {
        let next = path.get(depth);
        let mut next_index = None;
        for row in children.get(&location)? {
            if next.is_some_and(|crumb| crumb.location == row.id) {
                next_index = Some(nodes.len());
            }
            nodes.push(node(&row.name, Some(parent), row.book_count));
            trails.push([trails[parent].clone(), vec![BrowseCrumb { location: row.id, label: row.name.clone() }]].concat());
        }
        let Some(crumb) = next else { break };
        // A location reached some other way than through its parent's children
        // — a search hit's trail — still belongs on the path.
        parent = next_index.unwrap_or_else(|| {
            nodes.push(node(&crumb.label, Some(parent), 0));
            trails.push([trails[parent].clone(), vec![crumb.clone()]].concat());
            nodes.len() - 1
        });
        location = crumb.location;
    }
    Some((GraphTree::new(nodes), trails, parent))
}

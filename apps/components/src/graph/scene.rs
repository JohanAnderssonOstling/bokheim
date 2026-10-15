//! Draws a graph layout on an infinite canvas: the edges painted beneath, and
//! the caller's own element for each node over its box.

use std::sync::Arc;

use gpui::{AnyElement, App, BorderStyle, Bounds, Entity, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px, quad, size};

use super::layout::{GraphEdge, GraphLayout, GraphLayoutKind, PlacedNode};
use crate::{BrowserTheme, InfiniteCanvas, InfiniteCanvasView, browser_theme, infinite_canvas};

const E_INK: bool = cfg!(feature = "kobo");

/// The graph on `canvas`. `departing` is a layout fading out, drawn beneath at
/// its opacity. `node` draws one node, told whether it is from the departing
/// layout; it is placed and sized to the node's box.
pub fn graph_canvas(canvas: &Entity<InfiniteCanvas>, layout: Arc<GraphLayout>, departing: Option<(Arc<GraphLayout>, f32)>, node: impl Fn(&PlacedNode, bool) -> AnyElement + 'static, cx: &App) -> InfiniteCanvasView {
    let theme = browser_theme(cx);
    infinite_canvas(canvas).visible_content(move |viewport, camera| {
        // Keep antialiased strokes at the clip edge. The margin is in screen
        // pixels, so zooming out does not clip a fixed-width connector.
        let viewport = viewport.dilate(2.0 / camera.zoom);
        let place = |placed: &PlacedNode, departing: bool, opacity: f32| {
            let bounds = placed.bounds;
            div().absolute().left(px(bounds.origin.x)).top(px(bounds.origin.y)).w(px(bounds.size.width)).h(px(bounds.size.height)).opacity(placed.opacity * opacity).child(node(placed, departing))
        };
        let visible = |placed: &&PlacedNode| placed.opacity > 0.0 && viewport.intersects(&placed.bounds);
        let mut scene = div().size_full();
        if let Some((departing, opacity)) = departing.filter(|(_, opacity)| *opacity > 0.0) {
            scene = scene.child(edges(&departing, theme, camera.zoom, opacity, viewport)).children(departing.nodes.iter().filter(visible).map(|placed| place(placed, true, opacity)));
        }
        scene.child(edges(&layout, theme, camera.zoom, 1.0, viewport)).children(layout.nodes.iter().filter(visible).map(|placed| place(placed, false, 1.0))).into_any_element()
    })
}

/// Both column Bezier control points lie within the endpoint rectangle, so this
/// is a conservative bound for curves as well as radial and straight edges.
/// Check the connector itself: it can cross the viewport with both nodes offscreen.
fn edge_visible(edge: &GraphEdge, viewport: Bounds<f32>) -> bool {
    edge.opacity > 0.0 && viewport.intersects(&Bounds::new(point(edge.start.x.min(edge.end.x), edge.start.y.min(edge.end.y)), size((edge.end.x - edge.start.x).abs(), (edge.end.y - edge.start.y).abs())))
}

/// Curved connectors that keep their screen width at any zoom.
fn edges(layout: &GraphLayout, theme: BrowserTheme, zoom: f32, opacity: f32, viewport: Bounds<f32>) -> impl IntoElement {
    let edges = layout.edges.clone();
    let radial = layout.kind == GraphLayoutKind::AdaptiveRadial;
    let (width, path_width): (f32, f32) = if E_INK { (1.0, 1.8) } else { (1.0, 1.5) };
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            for edge in edges.iter().filter(|edge| edge_visible(edge, viewport)) {
                let (stroke, color) = if edge.on_path { (path_width, theme.text_muted) } else { (width, theme.rule) };
                let color = color.opacity(edge.opacity * opacity);
                if radial {
                    let mut builder = PathBuilder::stroke(px(stroke / zoom));
                    builder.move_to(bounds.origin + point(px(edge.start.x), px(edge.start.y)));
                    builder.line_to(bounds.origin + point(px(edge.end.x), px(edge.end.y)));
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, color);
                    }
                    continue;
                }
                if (edge.end.y - edge.start.y).abs() <= 0.5 {
                    let left = edge.start.x.min(edge.end.x);
                    let right = edge.start.x.max(edge.end.x);
                    let thickness = stroke.max(1.0_f32) / zoom;
                    let bar = gpui::Bounds::new(bounds.origin + point(px(left), px(edge.start.y - thickness / 2.0)), size(px(right - left), px(thickness)));
                    window.paint_quad(quad(bar, px(0.0), color, px(0.0), color, BorderStyle::Solid));
                    continue;
                }
                let mut builder = PathBuilder::stroke(px(stroke / zoom));
                builder.move_to(bounds.origin + point(px(edge.start.x), px(edge.start.y)));
                let middle = (edge.start.x + edge.end.x) / 2.0;
                builder.cubic_bezier_to(bounds.origin + point(px(edge.end.x), px(edge.end.y)), bounds.origin + point(px(middle), px(edge.start.y)), bounds.origin + point(px(middle), px(edge.end.y)));
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanvasCamera, GraphTree, LayoutParams, graph_layout};
    use gpui::{AppContext, Context, InteractiveElement, Modifiers, Render, StatefulInteractiveElement, TestAppContext, VisualTestContext, Window};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct GraphFixture {
        camera: Entity<InfiniteCanvas>,
        layout: Arc<GraphLayout>,
        departing: Arc<GraphLayout>,
        width: f32,
        built: Rc<RefCell<Vec<(usize, bool)>>>,
        clicks: Rc<Cell<usize>>,
    }

    impl Render for GraphFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let built = self.built.clone();
            let clicks = self.clicks.clone();
            div().pl(px(30.0)).pt(px(20.0)).child(div().w(px(self.width)).h(px(100.0)).child(graph_canvas(
                &self.camera,
                self.layout.clone(),
                Some((self.departing.clone(), 0.5)),
                move |node, departing| {
                    built.borrow_mut().push((node.node, departing));
                    let clicks = clicks.clone();
                    div().id(("test-graph-node", node.node)).size_full().on_click(move |_, _, _| clicks.set(clicks.get() + 1)).into_any_element()
                },
                cx,
            )))
        }
    }

    #[gpui::test]
    fn graph_culls_before_building_nodes_using_current_bounds_camera_and_ghost_pose(cx: &mut TestAppContext) {
        let camera = cx.new(|_| InfiniteCanvas::new(0.1, 4.0));
        let tree = GraphTree::new(vec![None, Some(0), Some(0)]);
        let layout = graph_layout(GraphLayoutKind::Columns, &tree, 0, &|_, _| size(20.0, 20.0), &LayoutParams::desktop());
        let centers = [point(50.0, 50.0), point(250.0, 50.0), point(650.0, 50.0)];
        let layout = Arc::new(layout.with_pose(&centers, &[1.0; 3]));
        let departing = Arc::new(layout.with_pose(&[point(-200.0, 50.0), point(75.0, 50.0), point(800.0, 50.0)], &[1.0; 3]));
        let built = Rc::new(RefCell::new(Vec::new()));
        let clicks = Rc::new(Cell::new(0));
        let root = cx.open_window(size(px(1000.0), px(400.0)), |_, _| GraphFixture { camera: camera.clone(), layout, departing, width: 100.0, built: built.clone(), clicks: clicks.clone() });
        let mut visual = VisualTestContext::from_window(root.into(), cx);
        // Opening a test window draws it once, before any previous bounds exist.
        assert_eq!(*built.borrow(), [(1, true), (0, false)]);
        visual.simulate_click(point(px(80.0), px(70.0)), Modifiers::none());
        assert_eq!(clicks.get(), 1, "the deferred scene retains transformed hit testing");

        built.borrow_mut().clear();
        root.update(cx, |root, _, cx| {
            root.width = 300.0;
            cx.notify();
        })
        .unwrap();
        assert_eq!(*built.borrow(), [(1, true), (0, false), (1, false)], "newly visible nodes must appear in the resize frame");

        built.borrow_mut().clear();
        camera.update(cx, |canvas, cx| {
            canvas.camera = CanvasCamera { pan: point(px(-200.0), px(0.0)), zoom: 1.0 };
            cx.notify();
        });
        assert_eq!(*built.borrow(), [(1, false)], "pan must cull in world coordinates");
        visual.simulate_click(point(px(80.0), px(70.0)), Modifiers::none());
        assert_eq!(clicks.get(), 2, "panning must move hitboxes with the visible node");

        built.borrow_mut().clear();
        camera.update(cx, |canvas, cx| {
            canvas.camera.zoom = 0.5;
            cx.notify();
        });
        assert_eq!(*built.borrow(), [(2, true), (2, false)], "zoom changes both world origin and extent");
    }

    #[test]
    fn connector_culling_keeps_crossing_curves_and_strokes_at_the_clip_edge() {
        let viewport = Bounds::new(point(0.0, 0.0), size(100.0, 100.0)).dilate(2.0);
        let edge = |start, end| GraphEdge { from: 0, to: 1, start, end, on_path: false, opacity: 1.0 };
        assert!(edge_visible(&edge(point(-50.0, 50.0), point(150.0, 50.0)), viewport));
        assert!(edge_visible(&edge(point(50.0, -50.0), point(50.0, 150.0)), viewport));
        assert!(edge_visible(&edge(point(-50.0, -50.0), point(150.0, 150.0)), viewport));
        assert!(edge_visible(&edge(point(-50.0, -1.0), point(150.0, -1.0)), viewport));
        assert!(!edge_visible(&edge(point(-50.0, -10.0), point(150.0, -10.0)), viewport));
        assert!(!edge_visible(&edge(point(150.0, 50.0), point(200.0, 50.0)), viewport));
    }
}

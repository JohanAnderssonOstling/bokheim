//! Draws a graph layout on an infinite canvas: the edges painted beneath, and
//! the caller's own element for each node over its box.

use gpui::{AnyElement, App, BorderStyle, Entity, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px, quad, size};

use super::layout::{GraphLayout, GraphLayoutKind, PlacedNode};
use crate::{BrowserTheme, InfiniteCanvas, InfiniteCanvasView, browser_theme, infinite_canvas};

const E_INK: bool = cfg!(feature = "kobo");

/// The graph on `canvas`. `departing` is a layout fading out, drawn beneath at
/// its opacity. `node` draws one node, told whether it is from the departing
/// layout; it is placed and sized to the node's box.
pub fn graph_canvas(canvas: &Entity<InfiniteCanvas>, layout: &GraphLayout, departing: Option<(&GraphLayout, f32)>, node: impl Fn(&PlacedNode, bool) -> AnyElement, cx: &App) -> InfiniteCanvasView {
    let theme = browser_theme(cx);
    let zoom = canvas.read(cx).camera.zoom;
    let place = |placed: &PlacedNode, departing: bool, opacity: f32| {
        let bounds = placed.bounds;
        div().absolute().left(px(bounds.origin.x)).top(px(bounds.origin.y)).w(px(bounds.size.width)).h(px(bounds.size.height)).opacity(placed.opacity * opacity).child(node(placed, departing))
    };
    let mut view = infinite_canvas(canvas);
    if let Some((departing, opacity)) = departing {
        view = view.child(edges(departing, theme, zoom, opacity)).children(departing.nodes.iter().map(|placed| place(placed, true, opacity)));
    }
    view.child(edges(layout, theme, zoom, 1.0)).children(layout.nodes.iter().map(|placed| place(placed, false, 1.0)))
}

/// Curved connectors that keep their screen width at any zoom.
fn edges(layout: &GraphLayout, theme: BrowserTheme, zoom: f32, opacity: f32) -> impl IntoElement {
    let edges = layout.edges.clone();
    let radial = layout.kind == GraphLayoutKind::AdaptiveRadial;
    let (width, path_width): (f32, f32) = if E_INK { (1.0, 1.8) } else { (1.0, 1.5) };
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            for edge in edges.iter() {
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
                builder.cubic_bezier_to(
                    bounds.origin + point(px(edge.end.x), px(edge.end.y)),
                    bounds.origin + point(px(middle), px(edge.start.y)),
                    bounds.origin + point(px(middle), px(edge.end.y)),
                );
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

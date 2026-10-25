//! Where to put the canvas camera for a graph layout.

use gpui::{Bounds, Pixels, Point, point, px};

use super::layout::GraphLayout;
use crate::CanvasCamera;

/// Screen margin kept between the framed children and the visible area.
const MARGIN: f32 = 14.0;

/// Frames the selection and its children in `area`, the part of the canvas that is
/// not covered by overlays, in canvas-relative pixels. `user_zoom` scales the fit.
///
/// On compact screens the selection sits back from the centre, opposite the
/// direction its column opens, and its children stay on screen where they fit.
pub fn framing_camera(layout: &GraphLayout, area: Bounds<Pixels>, compact: bool, user_zoom: f32) -> CanvasCamera {
    let Some((&selection, s)) = layout.path.last().and_then(|selection| layout.node(*selection).map(|node| (selection, node))) else {
        return CanvasCamera { pan: area.center(), zoom: user_zoom };
    };
    let fan = layout.nodes.iter().filter(|node| node.node == selection || node.parent == Some(selection));
    let (x0, y0, x1, y1) = fan.fold((f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY), |(x0, y0, x1, y1), node| {
        let h = node.bounds;
        (x0.min(h.origin.x), y0.min(h.origin.y), x1.max(h.origin.x + h.size.width), y1.max(h.origin.y + h.size.height))
    });

    let (left, top) = (f32::from(area.origin.x), f32::from(area.origin.y));
    let (width, height) = (f32::from(area.size.width), f32::from(area.size.height));
    let (mut ax, mut ay) = (left + width / 2.0, top + height / 2.0);
    if compact && selection != 0 {
        let k = width.min(height) * 0.26;
        ax -= s.angle.cos() * k;
        ay -= s.angle.sin() * k;
    }

    let (sx, sy) = (s.center.x, s.center.y);
    let fits =
        [(x1 > sx).then(|| (left + width - MARGIN - ax) / (x1 - sx)), (x0 < sx).then(|| (ax - left - MARGIN) / (sx - x0)), (y1 > sy).then(|| (top + height - MARGIN - ay) / (y1 - sy)), (y0 < sy).then(|| (ay - top - MARGIN) / (sy - y0))];
    let (min_fit, max_fit) = if compact { (0.85, 1.1) } else { (0.45, 1.0) };
    let fit = fits.into_iter().flatten().fold(max_fit, f32::min).max(min_fit);
    let zoom = fit * user_zoom;

    let (mut tx, mut ty) = (ax - sx * zoom, ay - sy * zoom);
    if compact {
        tx = keep_visible(x0, x1, tx, left, width, zoom);
        ty = keep_visible(y0, y1, ty, top, height, zoom);
    }
    CanvasCamera { pan: point(px(tx), px(ty)), zoom }
}

/// Keeps `node` at the canvas-relative `anchor` at the given zoom, so a tapped node
/// stays under the finger after the layout changes around it.
pub fn anchored_camera(layout: &GraphLayout, node: usize, anchor: Point<Pixels>, zoom: f32) -> Option<CanvasCamera> {
    let placed = layout.node(node)?;
    Some(CanvasCamera { pan: point(anchor.x - px(placed.center.x * zoom), anchor.y - px(placed.center.y * zoom)), zoom })
}

/// Shifts one axis of the pan so the world span `lo..hi` stays inside the screen
/// span `start..start + size`, or centres it when it cannot fit.
fn keep_visible(lo: f32, hi: f32, offset: f32, start: f32, size: f32, zoom: f32) -> f32 {
    let min_offset = start + MARGIN - lo * zoom;
    let max_offset = start + size - MARGIN - hi * zoom;
    if min_offset <= max_offset { offset.clamp(min_offset, max_offset) } else { start + size / 2.0 - (lo + hi) * zoom / 2.0 }
}

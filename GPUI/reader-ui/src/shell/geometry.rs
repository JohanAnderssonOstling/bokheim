//! Window geometry persistence, shared by every reader window.

use gpui::{Context, Subscription, Window};

use crate::settings::{ReaderSettings, ReaderWindowGeometry};

/// Persists this window's position and size whenever the user moves or resizes
/// it. Repeated notifications for an unchanged geometry are ignored.
///
/// The returned subscription must be retained by the view; dropping it stops
/// the tracking.
pub(crate) fn observe<V: 'static>(window: &mut Window, cx: &mut Context<V>) -> Subscription {
    cx.observe_window_bounds(window, |_, window, cx| {
        let bounds = window.bounds();
        ReaderSettings::set_geometry(cx, ReaderWindowGeometry { x: f32::from(bounds.origin.x), y: f32::from(bounds.origin.y), width: f32::from(bounds.size.width), height: f32::from(bounds.size.height) });
    })
}

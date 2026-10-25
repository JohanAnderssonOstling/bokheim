//! Shared window size classes for adaptive GPUI layouts.

use gpui::Window;

pub const MEDIUM_WINDOW_WIDTH: f32 = 600.0;
pub const EXPANDED_WINDOW_WIDTH: f32 = 840.0;

/// Mobile app navigation also serves compact web and desktop windows.
/// Kobo browsing keeps the sidebar; its reader uses touch controls separately.
pub fn uses_mobile_navigation(window: &Window) -> bool {
    !cfg!(feature = "kobo") && (has_native_back_button() || WindowWidthClass::for_window(window).is_compact())
}

/// Touch readers use tappable bars and sheets even when browsing uses a rail.
/// Kobo has no pointer hover or OS Back gesture to reveal desktop controls.
pub fn uses_mobile_reader_controls(window: &Window) -> bool {
    cfg!(feature = "kobo") || crate::touch_ui() || uses_mobile_navigation(window)
}

/// Native mobile builds have a real OS back button/gesture (wired to the same
/// dismiss/return actions as the on-screen one), so the mobile navigation
/// heading skips its own back button there. Compact web and desktop windows
/// share the mobile heading layout but have no such OS-level back to fall back
/// on, so they keep the on-screen button.
pub fn has_native_back_button() -> bool {
    cfg!(any(target_os = "android", target_os = "ios", all(feature = "mobile", not(feature = "kobo"))))
}

/// How far up from the window's bottom edge the phone's own gestures start:
/// going home, the app switcher, and bringing back system bars a reader has
/// hidden. In pixels, because the gesture area is the device's, not the UI
/// font's.
const BOTTOM_GESTURE_EDGE: f32 = 48.0;

/// Whether a touch at `position` began in the bottom gesture area. A reading
/// surface ignores swipes that start there, so a swipe up meant for the
/// system does not also turn the page.
pub fn in_bottom_gesture_edge(position: gpui::Point<gpui::Pixels>, window: &Window) -> bool {
    position.y >= window.viewport_size().height - window.insets().safe_area.bottom.max(gpui::px(BOTTOM_GESTURE_EDGE))
}

/// How far a swipe must travel to turn a page: a share of the screen along
/// the swipe, kept within a range so a small phone still needs a deliberate
/// swipe and a tablet does not need a long one.
const SWIPE_VIEWPORT_FRACTION: f32 = 0.12;
const SWIPE_MINIMUM_DISTANCE: f32 = 48.0;
const SWIPE_MAXIMUM_DISTANCE: f32 = 120.0;

/// The distance a swipe along an axis `extent` pixels long must travel.
pub fn swipe_threshold(extent: f32) -> f32 {
    (extent * SWIPE_VIEWPORT_FRACTION).clamp(SWIPE_MINIMUM_DISTANCE, SWIPE_MAXIMUM_DISTANCE)
}

/// Which way a finished swipe turns the page, from the finger's whole travel.
/// The longer of the two axes decides, so a swipe that is mostly sideways is
/// sideways however much it drifts up or down. Up or left goes forward. `None`
/// when it travelled less than `swipe_threshold` along that axis.
pub fn swipe_turns_forward(travel: gpui::Point<f32>, window: &Window) -> Option<bool> {
    let viewport = window.viewport_size();
    let (along, extent) = if travel.x.abs() > travel.y.abs() { (travel.x, viewport.width) } else { (travel.y, viewport.height) };
    (along.abs() >= swipe_threshold(f32::from(extent))).then_some(along < 0.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowWidthClass {
    Compact,
    Medium,
    Expanded,
}

impl WindowWidthClass {
    pub fn for_window(window: &Window) -> Self {
        Self::for_width(f32::from(window.viewport_size().width))
    }

    pub fn for_width(width: f32) -> Self {
        if width < MEDIUM_WINDOW_WIDTH {
            Self::Compact
        } else if width < EXPANDED_WINDOW_WIDTH {
            Self::Medium
        } else {
            Self::Expanded
        }
    }

    pub fn is_compact(self) -> bool {
        self == Self::Compact
    }
}

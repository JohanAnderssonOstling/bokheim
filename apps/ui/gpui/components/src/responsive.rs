//! Shared window size classes for adaptive GPUI layouts.

use gpui::Window;

pub const MEDIUM_WINDOW_WIDTH: f32 = 600.0;
pub const EXPANDED_WINDOW_WIDTH: f32 = 840.0;

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

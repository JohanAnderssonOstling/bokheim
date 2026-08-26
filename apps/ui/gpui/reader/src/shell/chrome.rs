//! Window chrome shared by the readers.
//!
//! What is shared here is composition, not appearance: how a sidebar is placed
//! beside a document and how dragging its divider is persisted. The panels
//! themselves stay with each reader, because an EPUB annotation and a PDF
//! annotation genuinely offer different actions.

use gpui::prelude::*;
use gpui::{AnyElement, App, IntoElement, Window, div, px};
use ui_components::WindowWidthClass;

use crate::settings::ReaderSettings;

/// Smallest divider drag worth persisting, in pixels. Resize events arrive on
/// every mouse move; sub-pixel jitter must not write to disk.
const RESIZE_EPSILON: f32 = 1.0;

/// Places `sidebar` beside `document`, with a draggable divider whose width is
/// remembered across windows and sessions.
///
/// On Kobo the divider is dropped: the screen is too narrow to show both, so
/// the sidebar overlays the document instead of splitting it.
pub(crate) fn with_sidebar(sidebar: AnyElement, document: AnyElement, window: &Window, cx: &mut App) -> AnyElement {
    let width = ReaderSettings::preferences(cx).toc_width.min(f32::from(window.viewport_size().width));
    if sidebar_overlays_document(window) {
        return div().relative().size_full().min_h_0().min_w_0().overflow_hidden().child(document).child(div().absolute().left_0().top_0().bottom_0().w(px(width)).child(sidebar)).into_any_element();
    }
    ui_components::reader_split(sidebar, document, width)
        .on_resize(|width, _, cx| {
            set_sidebar_width(width, cx);
        })
        .into_any_element()
}

/// Persists a new sidebar width, ignoring drags too small to matter.
pub(crate) fn set_sidebar_width(width: f32, cx: &mut App) -> bool {
    if !is_meaningful_resize(ReaderSettings::preferences(cx).toc_width, width) {
        return false;
    }
    ReaderSettings::update(cx, |preferences| preferences.toc_width = width)
}

/// Whether a divider drag moved far enough to be worth persisting.
fn is_meaningful_resize(current: f32, next: f32) -> bool {
    (current - next).abs() >= RESIZE_EPSILON
}

/// The width the document must give up to a visible sidebar, used when sizing
/// overlays against the remaining space.
pub(crate) fn sidebar_width(visible: bool, window: &Window, cx: &App) -> f32 {
    // Compact readers and Kobo float the sidebar above the document rather
    // than reducing the text viewport to an unusable sliver.
    if !visible || sidebar_overlays_document(window) {
        return 0.0;
    }
    ReaderSettings::preferences(cx).toc_width
}

fn sidebar_overlays_document(window: &Window) -> bool {
    cfg!(feature = "kobo") || WindowWidthClass::for_window(window).is_compact()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_pixel_divider_jitter_is_not_persisted() {
        assert!(!is_meaningful_resize(300.0, 300.0));
        assert!(!is_meaningful_resize(300.0, 300.4));
        assert!(!is_meaningful_resize(300.0, 299.6));
    }

    #[test]
    fn a_real_drag_is_persisted_in_either_direction() {
        assert!(is_meaningful_resize(300.0, 301.0));
        assert!(is_meaningful_resize(300.0, 299.0));
        assert!(is_meaningful_resize(300.0, 420.0));
    }
}

//! Window chrome shared by the readers.
//!
//! What is shared here is composition, not appearance: how a sidebar is placed
//! beside a document and how dragging its divider is persisted. The panels
//! themselves stay with each reader, because an EPUB annotation and a PDF
//! annotation genuinely offer different actions.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, IntoElement, Window, div, px};
use ui_components::WindowWidthClass;

use crate::invalidation::Component;
use crate::settings::ReaderSettings;

/// Smallest divider drag worth persisting, in pixels. Resize events arrive on
/// every mouse move; sub-pixel jitter must not write to disk.
const RESIZE_EPSILON: f32 = 1.0;

/// An interaction emitted by the shared reader-surface overlay.
#[derive(Clone, Copy)]
pub(crate) enum SurfaceAction {
    PreviousPage,
    NextPage,
    ToggleChrome,
    ShowChrome,
}

/// Owns toolbar visibility and applies its window/invalidation side effects.
pub(crate) struct ChromeState {
    hidden: bool,
    system_bars_visible: std::cell::Cell<Option<bool>>,
    component: Component,
}

impl ChromeState {
    pub(crate) fn new(component: Component) -> Self {
        Self { hidden: false, system_bars_visible: std::cell::Cell::new(None), component }
    }

    pub(crate) fn is_visible(&self) -> bool {
        !self.hidden
    }

    /// Includes initial display and search keeping chrome visible after paging.
    pub(crate) fn sync_system_bars(&self, visible: bool, cx: &App) {
        if self.system_bars_visible.replace(Some(visible)) != Some(visible) {
            cx.set_system_bars_visible(visible);
        }
    }

    pub(crate) fn hide<C: 'static>(&mut self, cx: &mut Context<C>) {
        self.set_hidden(true, cx);
    }

    pub(crate) fn show<C: 'static>(&mut self, cx: &mut Context<C>) {
        self.set_hidden(false, cx);
    }

    pub(crate) fn toggle<C: 'static>(&mut self, cx: &mut Context<C>) {
        self.set_hidden(!self.hidden, cx);
    }

    fn set_hidden<C: 'static>(&mut self, hidden: bool, cx: &mut Context<C>) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        crate::invalidation::notify(cx, self.component, if hidden { "chrome_hidden" } else { "chrome_shown" });
    }
}

/// Builds the platform-specific interaction layer over a reading surface.
///
/// Kobo uses left/right page zones and a central toolbar toggle. Other touch
/// devices use the center to reveal hidden chrome, while pointer-driven
/// readers reveal it from the top edge. Navigation itself remains owned by the
/// format-specific reader through `on_action`.
pub(crate) fn surface_interactions<C: 'static>(
    reader_id: &'static str, chrome_visible: bool, sidebar_visible: bool, toolbar_height: f32, touch_input: bool, cx: &mut Context<C>, on_action: impl Fn(&mut C, SurfaceAction, &mut Window, &mut Context<C>) + Clone + 'static,
) -> Vec<AnyElement> {
    if cfg!(feature = "kobo") {
        if sidebar_visible {
            return Vec::new();
        }
        let top = px(if chrome_visible { toolbar_height } else { 0.0 });
        let previous = on_action.clone();
        let next = on_action.clone();
        let toggle = on_action;
        return vec![
            div()
                .id(format!("kobo-{reader_id}-previous-page-tap-zone"))
                .absolute()
                .left_0()
                .top(top)
                .bottom_0()
                .w(gpui::relative(0.25))
                .on_click(cx.listener(move |reader, _, window, cx| previous(reader, SurfaceAction::PreviousPage, window, cx)))
                .into_any_element(),
            div()
                .id(format!("kobo-{reader_id}-next-page-tap-zone"))
                .absolute()
                .right_0()
                .top(top)
                .bottom_0()
                .w(gpui::relative(0.25))
                .on_click(cx.listener(move |reader, _, window, cx| next(reader, SurfaceAction::NextPage, window, cx)))
                .into_any_element(),
            div()
                .id(format!("kobo-{reader_id}-toggle-toolbar-tap-zone"))
                .absolute()
                .left(gpui::relative(0.25))
                .right(gpui::relative(0.25))
                .top(top)
                .bottom_0()
                .on_click(cx.listener(move |reader, _, window, cx| toggle(reader, SurfaceAction::ToggleChrome, window, cx)))
                .into_any_element(),
        ];
    }
    if chrome_visible {
        return Vec::new();
    }
    if touch_input {
        return vec![
            div()
                .id(format!("touch-{reader_id}-show-toolbar-tap-zone"))
                .absolute()
                .left(gpui::relative(0.25))
                .right(gpui::relative(0.25))
                .top_0()
                .bottom_0()
                .on_click(cx.listener(move |reader, _, window, cx| on_action(reader, SurfaceAction::ShowChrome, window, cx)))
                .into_any_element(),
        ];
    }
    let reveal_on_move = on_action.clone();
    vec![
        ui_components::reader_chrome_reveal()
            .on_mouse_move(cx.listener(move |reader, _, window, cx| reveal_on_move(reader, SurfaceAction::ShowChrome, window, cx)))
            .on_click(cx.listener(move |reader, _, window, cx| on_action(reader, SurfaceAction::ShowChrome, window, cx)))
            .into_any_element(),
    ]
}

/// Places `sidebar` beside `document`, with a draggable divider whose width is
/// remembered across windows and sessions.
///
/// On Kobo and on a compact window the divider is dropped: the screen is too
/// narrow to show both, so the sidebar overlays the document instead of
/// splitting it.
///
/// That overlay takes the whole content area rather than a column of
/// `toc_width`. A partial overlay left a strip of text showing beside it that
/// could not be read or paged, and the remembered width means nothing on a
/// screen it was never dragged on. Contents is a place the reader goes to and
/// comes back from, so it gets the screen while it is open — the toolbar stays
/// above it, which is what closes it again.
pub(crate) fn with_sidebar(sidebar: AnyElement, document: AnyElement, chrome_visible: bool, search_visible: bool, window: &Window, cx: &mut App) -> AnyElement {
    if sidebar_overlays_document(window) {
        let top = if ui_components::uses_mobile_navigation(window) && chrome_visible {
            window.insets().safe_area.top + px(ui_components::READER_TOOLBAR_HEIGHT + if search_visible { super::search::bar_height(window) } else { 0.0 })
        } else {
            px(0.)
        };
        return div().relative().size_full().min_h_0().min_w_0().overflow_hidden().child(document).child(div().absolute().left_0().right_0().top(top).bottom_0().child(sidebar)).into_any_element();
    }
    let width = ReaderSettings::preferences(cx).toc_width.min(f32::from(window.viewport_size().width));
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

//! Window chrome shared by the readers.
//!
//! What is shared here is composition, not appearance: how a sidebar is placed
//! beside a document and how dragging its divider is persisted. The panels
//! themselves stay with each reader, because an EPUB annotation and a PDF
//! annotation genuinely offer different actions.

use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, IntoElement, Window, div, px};
use std::cell::Cell;

use super::layout::{clamp_reader_sidebar_width, reader_split};
use super::state_panel::{SidebarTab, adjacent_sidebar_tab};
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
    /// Closing the sidebar can leave the pointer over the newly-mounted left
    /// reveal strip. Ignore that one synthetic/replayed move; otherwise the
    /// close click immediately opens the sidebar again.
    suppress_next_pointer_reveal: bool,
    system_bars_visible: Cell<Option<bool>>,
    component: Component,
}

impl ChromeState {
    pub(crate) fn from_preferences(component: Component, cx: &App) -> Self {
        Self { hidden: ReaderSettings::preferences(cx).chrome_hidden, suppress_next_pointer_reveal: false, system_bars_visible: Cell::new(None), component }
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
        self.suppress_next_pointer_reveal = true;
        self.set_hidden(true, cx);
    }

    pub(crate) fn show<C: 'static>(&mut self, cx: &mut Context<C>) {
        self.set_hidden(false, cx);
    }

    /// Moving through the book uncovers the text on phones, where the bars
    /// sit over the page.
    pub(crate) fn hide_for_motion<C: 'static>(&mut self, window: &Window, cx: &mut Context<C>) {
        if ui_components::uses_mobile_reader_controls(window) {
            self.hide(cx);
        }
    }

    /// The tab a sidebar-cycling key selects: the neighbour while the panel is
    /// showing, otherwise the current tab so the first press only reveals it.
    pub(crate) fn cycled_sidebar_tab(&self, sidebar_visible: bool, active: SidebarTab, contents_available: bool, direction: i32, window: &Window) -> SidebarTab {
        let visible = self.is_visible() && (!ui_components::uses_mobile_reader_controls(window) || sidebar_visible);
        if visible { adjacent_sidebar_tab(active, contents_available, direction) } else { active }
    }

    /// Reveals from the desktop left-edge hover strip. A close action replaces
    /// the sidebar with that strip under the stationary pointer, so GPUI can
    /// replay the just-finished pointer movement to it. That replay is not a
    /// new hover intent.
    pub(crate) fn show_from_pointer<C: 'static>(&mut self, cx: &mut Context<C>) -> bool {
        if self.suppress_next_pointer_reveal {
            self.suppress_next_pointer_reveal = false;
            return false;
        }
        let was_hidden = self.hidden;
        self.show(cx);
        was_hidden
    }

    pub(crate) fn show_from_touch<C: 'static>(&mut self, cx: &mut Context<C>) -> bool {
        let was_hidden = self.hidden;
        self.show(cx);
        was_hidden
    }

    pub(crate) fn toggle<C: 'static>(&mut self, cx: &mut Context<C>) {
        self.set_hidden(!self.hidden, cx);
    }

    fn set_hidden<C: 'static>(&mut self, hidden: bool, cx: &mut Context<C>) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        ReaderSettings::update(cx, |preferences| preferences.chrome_hidden = hidden);
        crate::invalidation::notify(cx, self.component, if hidden { "chrome_hidden" } else { "chrome_shown" });
    }
}

/// Builds the platform-specific interaction layer over a reading surface.
///
/// Touch readers handle unclaimed document taps in their document container, so
/// links and images receive them first. Pointer-driven readers reveal the
/// sidebar from the left edge. The legacy Kobo zones only apply without touch
/// controls; they must not cover the mobile bars or consume document taps.
pub(crate) fn surface_interactions<C: 'static>(
    reader_id: &'static str, chrome_visible: bool, sidebar_visible: bool, touch_input: bool, cx: &mut Context<C>, on_action: impl Fn(&mut C, SurfaceAction, &mut Window, &mut Context<C>) + Clone + 'static,
) -> Vec<AnyElement> {
    if touch_input {
        return Vec::new();
    }
    if cfg!(feature = "kobo") {
        if sidebar_visible {
            return Vec::new();
        }
        // No toolbar row sits above these zones any more — the sidebar
        // overlays the whole surface when it is shown, which the check above
        // already accounts for.
        let top = px(0.0);
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
    // The sidebar is the reader's only chrome now and sits on the left, so a
    // pointer-driven reader reaches for the left edge to bring it back rather
    // than the top edge a toolbar used to occupy.
    let reveal_on_move = on_action.clone();
    vec![ui_components::reader_chrome_reveal_left().on_mouse_move(cx.listener(move |reader, _, window, cx| reveal_on_move(reader, SurfaceAction::ShowChrome, window, cx))).into_any_element()]
}

/// A single unclaimed document tap toggles mobile controls. Links, images,
/// selections, and gestures consume their own input before this check runs.
pub(crate) fn is_page_tap(event: &ClickEvent) -> bool {
    matches!(event, ClickEvent::Mouse(_) | ClickEvent::Touch(_)) && event.standard_click() && event.click_count() == 1
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
/// comes back from, so it gets the screen while it is open.
///
/// The sidebar carries its own header, so there is no toolbar row above the
/// overlay to leave room for. On a mobile-navigation window the bottom bar
/// (`shell::page`'s mobile branch) stays visible beneath the sheet, so the
/// sheet leaves room below it. A `partial` sheet — Settings on a phone — stops
/// part way up, so the page shows above it while its look is being changed.
///
/// On desktop the remembered width is the panel's; the rail beside it is added
/// on top, so dragging the divider never squeezes the rail.
pub(crate) fn with_sidebar(sidebar: AnyElement, document: AnyElement, partial: bool, window: &Window, cx: &mut App) -> AnyElement {
    if sidebar_overlays_document(window) {
        return div().relative().size_full().min_h_0().min_w_0().overflow_hidden().child(document).child(sidebar_sheet(sidebar, partial, window)).into_any_element();
    }
    let width = ReaderSettings::preferences(cx).toc_width.min(f32::from(window.viewport_size().width));
    reader_split(sidebar, document, width, rail_width(window)).into_any_element()
}

/// The sheet overlays the whole reader page, including space reserved for a
/// hidden dock. Keeping it outside the document column also preserves the
/// document's height while a sheet is opened and closed.
pub(crate) fn sidebar_sheet(sidebar: AnyElement, partial: bool, window: &Window) -> AnyElement {
    let mobile = ui_components::uses_mobile_reader_controls(window);
    let safe_area = window.insets().safe_area;
    let top = match (mobile, partial) {
        (true, true) => safe_area.top + window.viewport_size().height * PARTIAL_SHEET_GAP,
        (true, false) => safe_area.top,
        (false, _) => px(0.0),
    };
    let bottom = if mobile { safe_area.bottom + gpui::rems(ui_components::BOTTOM_NAV_HEIGHT_REM).to_pixels(window.rem_size()) } else { px(0.0) };
    div().absolute().left_0().right_0().top(top).bottom(bottom).child(sidebar).into_any_element()
}

/// How much of a phone screen a partial sheet leaves showing above it.
const PARTIAL_SHEET_GAP: f32 = 0.35;

/// The rail is part of the desktop sidebar; phones have bars instead.
fn rail_width(window: &Window) -> f32 {
    if ui_components::uses_mobile_reader_controls(window) { 0.0 } else { ui_components::READER_RAIL_WIDTH }
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
    clamp_reader_sidebar_width(ReaderSettings::preferences(cx).toc_width.min(f32::from(window.viewport_size().width))) + rail_width(window)
}

pub(crate) fn sidebar_overlays_document(window: &Window) -> bool {
    cfg!(feature = "kobo") || ui_components::uses_mobile_reader_controls(window)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::TestAppContext;

    use crate::settings::ReaderPreferences;

    use super::*;

    #[gpui::test]
    fn touch_controls_leave_document_taps_and_bars_uncovered(cx: &mut TestAppContext) {
        cx.new(|cx| {
            for visible in [false, true] {
                let overlays = surface_interactions("test-reader", visible, false, true, cx, |_: &mut (), _, _, _| {});
                assert!(overlays.is_empty(), "touch readers must receive taps in their document and controls");
            }
        });
    }

    #[gpui::test]
    fn reader_reopens_with_saved_chrome_visibility(cx: &mut TestAppContext) {
        let saved = Rc::new(RefCell::new(ReaderPreferences::default()));
        let save_target = saved.clone();
        cx.update(|cx| {
            cx.set_global(ReaderSettings { preferences: saved.borrow().clone(), save: Rc::new(move |preferences, _, _| *save_target.borrow_mut() = preferences) });
        });
        let chrome = cx.new(|cx| ChromeState::from_preferences(Component::ReaderShell, cx));
        chrome.update(cx, |chrome, cx| chrome.hide(cx));
        assert!(saved.borrow().chrome_hidden);
        let reopened = cx.new(|cx| ChromeState::from_preferences(Component::PdfShell, cx));
        reopened.update(cx, |chrome, cx| {
            assert!(!chrome.is_visible());
            assert!(chrome.show_from_pointer(cx));
        });
        assert!(!saved.borrow().chrome_hidden);
        cx.update(|cx| assert!(ChromeState::from_preferences(Component::ReaderShell, cx).is_visible()));
    }
}

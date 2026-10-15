//! Reader sidebar layout and divider dragging.

use gpui::prelude::*;
use gpui::{AnyElement, App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, RenderOnce, Style, Window, div, px};
use gpui_component::ElementExt;
use ui_components::browser_theme;

use super::chrome::set_sidebar_width;

const READER_SIDEBAR_MIN_WIDTH: f32 = 260.0;
const READER_SIDEBAR_MAX_WIDTH: f32 = 520.0;

pub(super) fn clamp_reader_sidebar_width(width: f32) -> f32 {
    width.clamp(READER_SIDEBAR_MIN_WIDTH, READER_SIDEBAR_MAX_WIDTH)
}

struct ReaderSplitState {
    width: f32,
    /// The rail's width, which sits left of the resizable panel.
    rail: f32,
    bounds: Bounds<Pixels>,
    resizing: bool,
}

impl ReaderSplitState {
    fn new(width: f32) -> Self {
        Self { width: clamp_reader_sidebar_width(width), rail: 0.0, bounds: Bounds::default(), resizing: false }
    }
}

/// A reader-specific two-pane layout whose sidebar has an absolute pixel width.
///
/// Unlike a proportional resizable panel group, window resizing never changes
/// the sidebar width. Only dragging the divider updates it.
#[derive(IntoElement)]
struct ReaderSplit {
    sidebar: AnyElement,
    document: AnyElement,
    /// The panel's width; the rail is added on top of it.
    width: f32,
    rail: f32,
}

impl RenderOnce for ReaderSplit {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = browser_theme(cx);
        let requested_width = clamp_reader_sidebar_width(self.width);
        let state = window.use_keyed_state("reader-layout", cx, |_, _| ReaderSplitState::new(requested_width));
        let rail = self.rail;
        state.update(cx, |state, _| {
            state.rail = rail;
            if !state.resizing && (state.width - requested_width).abs() >= 0.5 {
                state.width = requested_width;
            }
        });
        let width = state.read(cx).width + rail;

        let start_resize = state.clone();
        let track_bounds = state.clone();
        div()
            .id("reader-layout")
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .overflow_hidden()
            .child(div().h_full().w(px(width)).min_w(px(width)).max_w(px(width)).flex_none().min_h_0().overflow_hidden().child(self.sidebar))
            .child(div().h_full().min_h_0().min_w_0().flex_1().overflow_hidden().child(self.document))
            // The divider is the sidebar's only outer boundary. Keep it a
            // single `theme.rule` line, matching the sidebar header, tabs,
            // and controls; the former sidebar border plus this handle made
            // a visibly heavier two-tone seam.
            .child(div().id("reader-resize-handle").absolute().top_0().left(px(width - 4.0)).h_full().w(px(9.0)).cursor_col_resize().flex().justify_center().child(div().h_full().w(px(1.0)).bg(theme.rule)).on_mouse_down(
                MouseButton::Left,
                move |_, _, cx| {
                    cx.stop_propagation();
                    start_resize.update(cx, |state, cx| {
                        state.resizing = true;
                        cx.notify();
                    });
                },
            ))
            .on_prepaint(move |bounds, _, cx| {
                track_bounds.update(cx, |state, _| state.bounds = bounds);
            })
            .child(ReaderSplitEventSink { state })
    }
}

struct ReaderSplitEventSink {
    state: Entity<ReaderSplitState>,
}

impl IntoElement for ReaderSplitEventSink {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ReaderSplitEventSink {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Window, _: &mut App) -> Self::PrepaintState {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Self::PrepaintState, window: &mut Window, _cx: &mut App) {
        let move_state = self.state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if !phase.bubble() || !move_state.read(cx).resizing {
                return;
            }
            let bounds = move_state.read(cx).bounds;
            let rail = move_state.read(cx).rail;
            let width = clamp_reader_sidebar_width(f32::from(event.position.x - bounds.left()) - rail);
            move_state.update(cx, |state, cx| {
                if (state.width - width).abs() >= 0.5 {
                    state.width = width;
                    cx.notify();
                }
            });
        });

        let end_state = self.state.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if !phase.bubble() || !end_state.read(cx).resizing {
                return;
            }
            let width = end_state.read(cx).width;
            end_state.update(cx, |state, cx| {
                state.resizing = false;
                cx.notify();
            });
            set_sidebar_width(width, cx);
        });
    }
}

pub(super) fn reader_split(sidebar: impl IntoElement, document: impl IntoElement, sidebar_width: f32, rail: f32) -> impl IntoElement {
    ReaderSplit { sidebar: sidebar.into_any_element(), document: document.into_any_element(), width: sidebar_width, rail }
}

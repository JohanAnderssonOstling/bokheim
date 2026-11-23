use std::rc::Rc;

use gpui::{Anchor, AnyElement, App, Bounds, FocusHandle, Global, IntoElement, MouseButton, Pixels, RenderOnce, StyleRefinement, WeakEntity, Window, anchored, deferred, div, point, prelude::*, px};
use gpui_component::{ElementExt, popover::Popover};

use crate::Button;

#[derive(Clone, Copy, Default)]
struct PanelState {
    bounds: Bounds<Pixels>,
    open: bool,
}

#[derive(Default)]
struct Panels(Vec<WeakEntity<PanelState>>);
impl Global for Panels {}

pub fn has_open_panel(_: &Window, cx: &App) -> bool {
    cx.try_global::<Panels>().is_some_and(|panels| panels.0.iter().filter_map(WeakEntity::upgrade).any(|panel| panel.read(cx).open))
}

type OpenHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// Kobo device panels use the shared popover's explicit-position mode.
#[derive(IntoElement)]
pub struct DevicePopover {
    id: &'static str,
    popover: Popover,
    anchor: Anchor,
    trigger: Option<Button>,
    open: Option<bool>,
    on_open_change: Option<OpenHandler>,
    children: Vec<AnyElement>,
}

impl DevicePopover {
    pub fn new(id: &'static str) -> Self {
        Self { id, popover: Popover::new(id), anchor: Anchor::TopRight, trigger: None, open: None, on_open_change: None, children: Vec::new() }
    }

    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn trigger(mut self, trigger: Button) -> Self {
        self.trigger = Some(trigger);
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self.popover = self.popover.open(open);
        self
    }

    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.popover = self.popover.track_focus(focus);
        self
    }

    pub fn on_open_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for DevicePopover {
    fn style(&mut self) -> &mut StyleRefinement {
        self.popover.style()
    }
}

impl ParentElement for DevicePopover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for DevicePopover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(format!("device-panel-{}", self.id), cx, |_, _| PanelState::default());
        if cx.try_global::<Panels>().is_none() {
            cx.set_global(Panels::default());
        }
        let panels = &mut cx.global_mut::<Panels>().0;
        panels.retain(|panel| panel.upgrade().is_some());
        let weak = state.downgrade();
        if !panels.contains(&weak) {
            panels.push(weak);
        }
        if let Some(open) = self.open {
            state.update(cx, |state, _| state.open = open);
        }
        let PanelState { bounds, open } = *state.read(cx);
        let viewport = window.input_viewport();
        let width = px(super::PANEL_WIDTH + 2.0).min((viewport.size.width - px(22.0)).max(px(1.0)));
        let left = if self.anchor == Anchor::TopLeft { bounds.left() } else { bounds.right() - width };
        let left = left.max(viewport.left() + px(8.0)).min((viewport.right() - width - px(8.0)).max(viewport.left()));
        let top = bounds.bottom().max(viewport.top()).min((viewport.bottom() - px(10.0)).max(viewport.top()));
        let available = (viewport.bottom() - top - px(10.0)).max(px(1.0));
        let parent = window.current_view();
        let mut root = div();
        if open {
            root = root
                .child(deferred(anchored().position(point(viewport.left(), top)).child(div().w(viewport.size.width).h(viewport.bottom() - top).occlude().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))).with_priority(1));
        }
        let Some(trigger) = self.trigger else {
            return root;
        };
        root.child(
            self.popover
                .anchor(Anchor::TopLeft)
                .position(point(left, top))
                .p_0()
                .trigger(trigger.on_prepaint({
                    let state = state.clone();
                    move |bounds, _, cx| {
                        state.update(cx, |state, cx| {
                            if state.bounds != bounds {
                                state.bounds = bounds;
                                if state.open {
                                    cx.notify();
                                }
                            }
                        })
                    }
                }))
                .on_open_change(move |open, window, cx| {
                    state.update(cx, |state, _| state.open = *open);
                    if let Some(handler) = &self.on_open_change {
                        handler(open, window, cx);
                    }
                    cx.notify(parent);
                })
                .child(div().id("kobo-panel-scroll").max_h(available).overflow_y_scroll().children(self.children)),
        )
    }
}

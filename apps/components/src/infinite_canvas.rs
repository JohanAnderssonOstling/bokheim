//! A pannable, zoomable surface.
//!
//! Children are laid out in world coordinates and moved together by one element
//! transform, so gpui paints, rasterizes and hit-tests them where they appear.
//! Children handle their own pointer input; a press anywhere pans once it moves
//! past a tap's slop, and a child's click checks [`InfiniteCanvas::panned`] so
//! that a pan ending over it does not also open it.

use gpui::{AnyElement, App, AvailableSpace, Bounds, ElementTransform, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, RenderOnce, ScrollDelta, Styled, Window, canvas, div, point, px, size};

use crate::browser_theme;

/// Movement, in screen pixels, after which a press becomes a pan instead of a tap.
const TAP_SLOP: f32 = 6.0;
/// Zoom change per mouse-wheel line.
const ZOOM_PER_LINE: f32 = 1.12;
/// Zoom exponent per screen pixel of a smooth scroll gesture.
const ZOOM_PER_PIXEL: f32 = 0.0015;

/// Where the world sits on screen: a world point `p` is drawn at `pan + p * zoom`,
/// in pixels relative to the canvas origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasCamera {
    pub pan: Point<Pixels>,
    pub zoom: f32,
}

impl CanvasCamera {
    /// Canvas-relative screen position of a world point.
    pub fn to_screen(&self, world: Point<Pixels>) -> Point<Pixels> {
        self.pan + point(world.x * self.zoom, world.y * self.zoom)
    }

    /// World point under a canvas-relative screen position.
    pub fn to_world(&self, screen: Point<Pixels>) -> Point<Pixels> {
        point((screen.x - self.pan.x) / self.zoom, (screen.y - self.pan.y) / self.zoom)
    }

    /// The same camera at a new zoom, keeping the world point under `anchor` in place.
    fn zoomed_about(self, anchor: Point<Pixels>, zoom: f32) -> Self {
        let world = self.to_world(anchor);
        Self { pan: anchor - point(world.x * zoom, world.y * zoom), zoom }
    }

    fn element_transform(&self) -> ElementTransform {
        ElementTransform::default().translate(self.pan).scale(size(self.zoom, self.zoom)).origin(point(0.0, 0.0))
    }
}

impl Default for CanvasCamera {
    fn default() -> Self {
        Self { pan: point(px(0.0), px(0.0)), zoom: 1.0 }
    }
}

/// A press in progress, in window coordinates.
#[derive(Clone, Copy)]
struct Press {
    start: Point<Pixels>,
    camera: CanvasCamera,
    panning: bool,
}

/// State behind an [`infinite_canvas`]: the camera, its zoom limits and the
/// gesture in progress. The page owns it and sets the camera to frame content.
pub struct InfiniteCanvas {
    pub camera: CanvasCamera,
    min_zoom: f32,
    max_zoom: f32,
    /// Where the canvas was laid out in the window last frame; `None` before its first layout.
    pub bounds: Option<Bounds<Pixels>>,
    press: Option<Press>,
    /// Whether the last press, now released, panned.
    last_panned: bool,
    touch_pan_origin: Option<Point<Pixels>>,
}

impl InfiniteCanvas {
    pub fn new(min_zoom: f32, max_zoom: f32) -> Self {
        Self { camera: CanvasCamera::default(), min_zoom, max_zoom, bounds: None, press: None, last_panned: false, touch_pan_origin: None }
    }

    /// Whether the press in progress, or the one just released, panned rather
    /// than tapped. A child's click asks this before acting.
    pub fn panned(&self) -> bool {
        self.press.map_or(self.last_panned, |press| press.panning)
    }

    fn local(&self, window_position: Point<Pixels>) -> Point<Pixels> {
        self.bounds.map_or(window_position, |bounds| window_position - bounds.origin)
    }

    fn zoom_at(&mut self, window_position: Point<Pixels>, zoom: f32) {
        let zoom = zoom.clamp(self.min_zoom, self.max_zoom);
        self.camera = self.camera.zoomed_about(self.local(window_position), zoom);
    }

    /// Follows a press. Returns whether the camera moved and should repaint now;
    /// on e-ink the pan is applied only when the press ends.
    fn drag_to(&mut self, position: Point<Pixels>) -> bool {
        let Some(press) = self.press.as_mut() else { return false };
        let delta = position - press.start;
        if !press.panning && f32::from(delta.x).hypot(f32::from(delta.y)) < TAP_SLOP {
            return false;
        }
        press.panning = true;
        if cfg!(feature = "kobo") {
            return false;
        }
        self.camera = CanvasCamera { pan: press.camera.pan + delta, ..press.camera };
        true
    }

    /// Ends a press, applying a pan that e-ink held back.
    fn release(&mut self, position: Point<Pixels>) {
        let Some(press) = self.press.take() else { return };
        self.last_panned = press.panning;
        if press.panning {
            self.camera = CanvasCamera { pan: press.camera.pan + (position - press.start), ..press.camera };
        }
    }
}

/// A pannable, zoomable view of `state`'s world. Add children positioned in world
/// coordinates.
pub fn infinite_canvas(state: &Entity<InfiniteCanvas>) -> InfiniteCanvasView {
    InfiniteCanvasView { state: state.clone(), children: Vec::new(), visible_content: None }
}

#[derive(IntoElement)]
pub struct InfiniteCanvasView {
    state: Entity<InfiniteCanvas>,
    children: Vec<AnyElement>,
    visible_content: Option<Box<dyn FnOnce(Bounds<f32>, CanvasCamera) -> AnyElement>>,
}

impl InfiniteCanvasView {
    /// Build world content after layout, using this frame's visible world bounds.
    /// Offscreen elements can be omitted before they are constructed or measured.
    pub fn visible_content(mut self, build: impl FnOnce(Bounds<f32>, CanvasCamera) -> AnyElement + 'static) -> Self {
        self.visible_content = Some(Box::new(build));
        self
    }
}

impl ParentElement for InfiniteCanvasView {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for InfiniteCanvasView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = browser_theme(cx);
        let state = self.state;
        let camera = state.read(cx).camera;
        let visible_content = self.visible_content.map(|build| {
            canvas(
                move |bounds, window, cx| {
                    let origin = camera.to_world(point(px(0.0), px(0.0)));
                    let viewport = Bounds::new(point(f32::from(origin.x), f32::from(origin.y)), size(f32::from(bounds.size.width) / camera.zoom, f32::from(bounds.size.height) / camera.zoom));
                    let mut content = div().relative().size_full().transform(camera.element_transform()).child(build(viewport, camera)).into_any_element();
                    content.prepaint_as_root(bounds.origin, bounds.size.map(AvailableSpace::Definite), window, cx);
                    content
                },
                |_, mut content, window, cx| content.paint(window, cx),
            )
            .absolute()
            .size_full()
        });
        div()
            .id("infinite-canvas")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.page_bg)
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                move |event, _, cx| {
                    state.update(cx, |canvas, _| {
                        canvas.press = Some(Press { start: event.position, camera: canvas.camera, panning: false });
                        canvas.last_panned = false;
                    });
                }
            })
            .on_mouse_move({
                let state = state.clone();
                move |event, _, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        state.update(cx, |canvas, cx| {
                            if canvas.drag_to(event.position) {
                                cx.notify();
                            }
                        });
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let state = state.clone();
                move |event, _, cx| {
                    state.update(cx, |canvas, cx| {
                        canvas.release(event.position);
                        cx.notify();
                    });
                }
            })
            .on_mouse_up_out(MouseButton::Left, {
                let state = state.clone();
                move |event, _, cx| {
                    state.update(cx, |canvas, cx| {
                        canvas.release(event.position);
                        cx.notify();
                    });
                }
            })
            // Wheel and trackpad scrolling both zoom around the pointer.
            .on_scroll_wheel({
                let state = state.clone();
                move |event, window, cx| {
                    state.update(cx, |canvas, cx| {
                        if window.is_page_scroll_swipe() {
                            if event.touch_phase == gpui::TouchPhase::Started {
                                canvas.touch_pan_origin = Some(canvas.camera.pan);
                            }
                            if let ScrollDelta::Pixels(delta) = event.delta {
                                canvas.camera.pan += delta;
                            }
                            cx.notify();
                            return;
                        }
                        if matches!(event.delta, ScrollDelta::Pages(_)) {
                            if let Some(origin) = canvas.touch_pan_origin.take() {
                                if event.touch_phase == gpui::TouchPhase::Cancelled {
                                    canvas.camera.pan = origin;
                                }
                                cx.notify();
                                return;
                            }
                        }
                        match event.delta {
                            ScrollDelta::Lines(lines) => {
                                let zoom = canvas.camera.zoom * ZOOM_PER_LINE.powf(lines.y);
                                canvas.zoom_at(event.position, zoom);
                            }
                            ScrollDelta::Pages(delta) => {
                                if let Some(bounds) = canvas.bounds {
                                    canvas.camera.pan += point(bounds.size.width * delta.x, bounds.size.height * delta.y);
                                }
                            }
                            ScrollDelta::Pixels(delta) => {
                                if cfg!(target_os = "android") {
                                    // Android sends a one-finger drag as pixel scrolling.
                                    canvas.camera.pan += delta;
                                } else {
                                    let zoom = canvas.camera.zoom * (f32::from(delta.y) * ZOOM_PER_PIXEL).exp();
                                    canvas.zoom_at(event.position, zoom);
                                }
                            }
                        }
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            })
            .on_pinch({
                let state = state.clone();
                move |event, _, cx| {
                    state.update(cx, |canvas, cx| {
                        let zoom = canvas.camera.zoom * (1.0 + event.delta);
                        canvas.zoom_at(event.position, zoom);
                        cx.notify();
                    });
                }
            })
            .child(
                canvas(
                    {
                        let state = state.clone();
                        move |bounds, _, cx| state.update(cx, |canvas, _| canvas.bounds = Some(bounds))
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().absolute().top_0().left_0().size_full().transform(camera.element_transform()).children(self.children))
            .children(visible_content)
    }
}

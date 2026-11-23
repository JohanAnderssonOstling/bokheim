use gpui::{AccessibleAction, App, Bounds, Context, Entity, FocusHandle, IntoElement, MouseButton, MouseUpEvent, Pixels, RenderOnce, Role, Window, div, prelude::*, px, relative};
use gpui_component::{
    ElementExt,
    global_state::GlobalState,
    slider::{SliderEvent, SliderState},
};

struct Gesture {
    bounds: Bounds<Pixels>,
    pressed: bool,
    focus: FocusHandle,
}

/// A light slider that changes its value and appearance only on release.
#[derive(IntoElement)]
pub struct ReleaseSlider {
    slider: Entity<SliderState>,
    disabled: bool,
}

impl ReleaseSlider {
    pub fn new(slider: &Entity<SliderState>, disabled: bool) -> Self {
        Self { slider: slider.clone(), disabled }
    }
}

fn commit(slider: &Entity<SliderState>, value: f32, window: &mut Window, cx: &mut App) {
    slider.update(cx, |slider, cx| {
        let value = ((value / slider.step_value()).round() * slider.step_value()).clamp(slider.min_value(), slider.max_value());
        if slider.value().start() == value {
            return;
        }
        slider.set_value(value, window, cx);
        cx.emit(SliderEvent::Change(slider.value()));
        cx.emit(SliderEvent::Release(slider.value()));
    });
}

fn release(gesture: &mut Gesture, event: &MouseUpEvent, slider: &Entity<SliderState>, window: &mut Window, cx: &mut Context<Gesture>) {
    if !gesture.pressed {
        return;
    }
    gesture.pressed = false;
    let width = f32::from(gesture.bounds.size.width);
    if width <= 0.0 {
        return;
    }
    let fraction = (f32::from(event.position.x - gesture.bounds.left()) / width).clamp(0.0, 1.0);
    let state = slider.read(cx);
    let value = state.min_value() + fraction * (state.max_value() - state.min_value());
    commit(slider, value, window, cx);
    cx.stop_propagation();
}

impl RenderOnce for ReleaseSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let gesture = window.use_keyed_state(("kobo-light-slider", self.slider.entity_id()), cx, |_, cx| Gesture { bounds: Bounds::default(), pressed: false, focus: cx.focus_handle().tab_index(0).tab_stop(true) });
        let state = self.slider.read(cx);
        let value = state.value().start();
        let minimum = state.min_value();
        let maximum = state.max_value();
        let fraction = if maximum > minimum { (value - minimum) / (maximum - minimum) } else { 0.0 };
        let theme = crate::browser_theme(cx);
        let disabled = self.disabled;
        let surface = div()
            .id(("kobo-light-track", self.slider.entity_id()))
            .relative()
            .w_full()
            .h(px(32.0))
            .role(Role::Slider)
            .aria_numeric_value(f64::from(value))
            .aria_min_numeric_value(f64::from(minimum))
            .aria_max_numeric_value(f64::from(maximum))
            .on_prepaint({
                let gesture = gesture.clone();
                move |bounds, _, cx| gesture.update(cx, |gesture, _| gesture.bounds = bounds)
            })
            .child(div().absolute().left_0().right_0().top(px(15.0)).h(px(2.0)).bg(theme.text_muted))
            .child(div().absolute().left(relative(fraction)).ml(px(-8.0)).top(px(8.0)).size(px(16.0)).border_1().border_color(theme.text).bg(theme.page_bg))
            .when(disabled, |surface| surface.opacity(0.4));
        surface.when(!disabled, |surface| {
            surface
                .track_focus(&gesture.read(cx).focus)
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(&gesture, |gesture, _, window, cx| {
                        gesture.pressed = true;
                        gesture.focus.focus(window, cx);
                        window.prevent_default();
                        GlobalState::suppress_text_selection(cx);
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_up(MouseButton::Left, {
                    let slider = self.slider.clone();
                    window.listener_for(&gesture, move |gesture, event, window, cx| release(gesture, event, &slider, window, cx))
                })
                .on_mouse_up_out(MouseButton::Left, {
                    let slider = self.slider.clone();
                    window.listener_for(&gesture, move |gesture, event, window, cx| release(gesture, event, &slider, window, cx))
                })
                .on_key_down({
                    let slider = self.slider.clone();
                    move |event, window, cx| {
                        let state = slider.read(cx);
                        let value = match event.keystroke.key.as_str() {
                            "left" | "down" => state.value().start() - state.step_value(),
                            "right" | "up" => state.value().start() + state.step_value(),
                            "home" => state.min_value(),
                            "end" => state.max_value(),
                            _ => return,
                        };
                        commit(&slider, value, window, cx);
                        cx.stop_propagation();
                    }
                })
                .on_a11y_action(AccessibleAction::Increment, {
                    let slider = self.slider.clone();
                    move |_, window, cx| {
                        let state = slider.read(cx);
                        commit(&slider, state.value().start() + state.step_value(), window, cx);
                    }
                })
                .on_a11y_action(AccessibleAction::Decrement, {
                    let slider = self.slider.clone();
                    move |_, window, cx| {
                        let state = slider.read(cx);
                        commit(&slider, state.value().start() - state.step_value(), window, cx);
                    }
                })
        })
    }
}

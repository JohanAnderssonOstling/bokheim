use crate::style::palette;
use floem::event::EventListener;
use floem::prelude::{container, create_rw_signal, h_stack, label, v_stack, Decorators, RwSignal, Scroll, ScrollExt, SignalGet, SignalUpdate, Stack, View, ViewTuple};
use floem::unit::PxPctAuto;
use floem::views::stack_from_iter;
use floem::IntoView;

pub fn create_button_group<VT: ViewTuple + 'static>(children: VT) -> Stack {
    h_stack(children)
}

pub fn create_top_row(content: Stack) -> Stack {
    h_stack((content,))
}

pub fn create_list_header<VT: ViewTuple + 'static>(children: VT) -> Stack {
    h_stack(children)
}

pub fn create_list_bodyr<V>(iterator: impl IntoIterator<Item = V>) -> Scroll
where
    V: IntoView + 'static,
{
    stack_from_iter(iterator).scroll()
}

pub fn create_list<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children)
}

pub fn create_centered_stack<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children)
}

pub fn create_full_stack<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children)
}

pub fn create_resizable_hsplit(
    left: impl View + 'static,
    right: impl View + 'static,
    left_width: RwSignal<f64>,
    min_left: f64,
    max_left: f64,
) -> impl View {
    let dragging = create_rw_signal(false);
    let last_x = create_rw_signal(0.0);
    let has_last = create_rw_signal(false);

    let handle = container(label(|| ""))
        .style(move |s| {
            let theme = palette();
            let base = s.width(8).height_full().cursor(floem::style::CursorStyle::ColResize).background(theme.split_handle_bg);
            if dragging.get() { base.background(theme.split_handle_active) } else { base.hover(|s| s.background(theme.split_handle_hover)) }
        })
        .on_event_stop(EventListener::PointerDown, {
            let dragging = dragging;
            move |event| {
                if event.point().is_some() {
                    dragging.set(true);
                    has_last.set(false);
                }
            }
        })
        .on_event_stop(EventListener::PointerUp, {
            let dragging = dragging;
            move |_| {
                dragging.set(false);
                has_last.set(false);
            }
        });

    h_stack((
        container(left).style(move |s| s.flex_basis(PxPctAuto::Px(left_width.get())).height_full().flex_grow(0.0).flex_shrink(0.0).min_height(0).min_width(0)),
        handle,
        container(right).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0)),
    ))
    .style(|s| s.width_full().height_full().flex_row().min_height(0).min_width(0))
    .on_event_stop(EventListener::PointerMove, {
        let dragging = dragging;
        let last_x = last_x;
        move |event| {
            if !dragging.get() {
                return;
            }
            if let Some(point) = event.point() {
                if !has_last.get() {
                    last_x.set(point.x);
                    has_last.set(true);
                    return;
                }
                let delta = point.x - last_x.get();
                let next = (left_width.get_untracked() + delta).clamp(min_left, max_left);
                left_width.set(next);
                last_x.set(point.x);
            }
        }
    })
    .on_event_stop(EventListener::PointerUp, {
        let dragging = dragging;
        move |_| {
            dragging.set(false);
            has_last.set(false);
        }
    })
    .on_event_stop(EventListener::FocusLost, {
        let dragging = dragging;
        move |_| {
            dragging.set(false);
            has_last.set(false);
        }
    })
    .on_event_stop(EventListener::PointerLeave, {
        let dragging = dragging;
        move |_| {
            dragging.set(false);
            has_last.set(false);
        }
    })
}

pub fn create_resizable_right_split(
    left: impl View + 'static,
    right: impl View + 'static,
    right_width: RwSignal<f64>,
    min_right: f64,
    max_right: f64,
) -> impl View {
    let dragging = create_rw_signal(false);
    let last_x = create_rw_signal(0.0);
    let has_last = create_rw_signal(false);

    let handle = container(label(|| ""))
        .style(move |s| {
            let theme = palette();
            let base = s.width(8).height_full().cursor(floem::style::CursorStyle::ColResize).background(theme.split_handle_bg);
            if dragging.get() { base.background(theme.split_handle_active) } else { base.hover(|s| s.background(theme.split_handle_hover)) }
        })
        .on_event_stop(EventListener::PointerDown, {
            let dragging = dragging;
            move |event| {
                if event.point().is_some() {
                    dragging.set(true);
                    has_last.set(false);
                }
            }
        })
        .on_event_stop(EventListener::PointerUp, {
            let dragging = dragging;
            move |_| {
                dragging.set(false);
                has_last.set(false);
            }
        });

    h_stack((
        container(left).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0)),
        handle,
        container(right).style(move |s| s.flex_basis(PxPctAuto::Px(right_width.get())).height_full().flex_grow(0.0).flex_shrink(0.0).min_height(0).min_width(0)),
    ))
    .style(|s| s.width_full().height_full().flex_row().min_height(0).min_width(0))
    .on_event_stop(EventListener::PointerMove, {
        let dragging = dragging;
        let last_x = last_x;
        move |event| {
            if !dragging.get() {
                return;
            }
            if let Some(point) = event.point() {
                if !has_last.get() {
                    last_x.set(point.x);
                    has_last.set(true);
                    return;
                }
                let delta = last_x.get() - point.x;
                let next = (right_width.get_untracked() + delta).clamp(min_right, max_right);
                right_width.set(next);
                last_x.set(point.x);
            }
        }
    })
    .on_event_stop(EventListener::PointerUp, {
        let dragging = dragging;
        move |_| {
            dragging.set(false);
            has_last.set(false);
        }
    })
    .on_event_stop(EventListener::FocusLost, {
        let dragging = dragging;
        move |_| {
            dragging.set(false);
            has_last.set(false);
        }
    })
    .on_event_stop(EventListener::PointerLeave, {
        let dragging = dragging;
        move |_| {
            dragging.set(false);
            has_last.set(false);
        }
    })
}

pub fn create_resizable_three_pane(
    left: impl View + 'static,
    middle: impl View + 'static,
    right: impl View + 'static,
    left_width: RwSignal<f64>,
    right_width: RwSignal<f64>,
    min_left: f64,
    max_left: f64,
    min_right: f64,
    max_right: f64,
) -> impl View {
    let center_with_right = create_resizable_right_split(middle, right, right_width, min_right, max_right);
    create_resizable_hsplit(left, center_with_right, left_width, min_left, max_left)
}

use crate::style::palette;
use floem::event::EventListener;
use floem::prelude::{container, create_rw_signal, h_stack, label, Decorators, RwSignal, SignalGet, SignalUpdate, View};
use floem::style::Style;

/// Which side of a resizable split keeps a fixed (user-draggable) width;
/// the other side flexes to fill the remaining space.
#[derive(Clone, Copy, PartialEq)]
pub enum FixedSide {
    Left,
    Right,
}

fn fixed_panel_style(s: Style, width: f64) -> Style {
    s.width(width).height_full().flex_grow(0.0).flex_shrink(0.0).min_height(0).min_width(0)
}

fn flex_panel_style(s: Style) -> Style {
    s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0)
}

pub fn resizable_split(fixed_side: FixedSide, left: impl View + 'static, right: impl View + 'static, fixed_width: RwSignal<f64>, min_width: f64, max_width: f64) -> impl View {
    let dragging = create_rw_signal(false);
    let last_x = create_rw_signal(0.0);
    let has_last = create_rw_signal(false);

    let reset = move || {
        dragging.set(false);
        has_last.set(false);
    };

    let handle = container(label(|| ""))
        .style(move |s| {
            let theme = palette();
            let base = s.width(8).height_full().cursor(floem::style::CursorStyle::ColResize).background(theme.split_handle_bg);
            if dragging.get() {
                base.background(theme.split_handle_active)
            } else {
                base.hover(|s| s.background(theme.split_handle_hover))
            }
        })
        .on_event_stop(EventListener::PointerDown, move |event| {
            if event.point().is_some() {
                dragging.set(true);
                has_last.set(false);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |_| reset());

    let left_panel = container(left).style(move |s| match fixed_side {
        FixedSide::Left => fixed_panel_style(s, fixed_width.get()),
        FixedSide::Right => flex_panel_style(s),
    });
    let right_panel = container(right).style(move |s| match fixed_side {
        FixedSide::Right => fixed_panel_style(s, fixed_width.get()),
        FixedSide::Left => flex_panel_style(s),
    });

    h_stack((left_panel, handle, right_panel))
        .style(|s| s.width_full().height_full().flex_row().min_height(0).min_width(0))
        .on_event_stop(EventListener::PointerMove, move |event| {
            if !dragging.get() {
                return;
            }
            if let Some(point) = event.point() {
                if !has_last.get() {
                    last_x.set(point.x);
                    has_last.set(true);
                    return;
                }
                let delta = match fixed_side {
                    FixedSide::Left => point.x - last_x.get(),
                    FixedSide::Right => last_x.get() - point.x,
                };
                let next = (fixed_width.get_untracked() + delta).clamp(min_width, max_width);
                fixed_width.set(next);
                last_x.set(point.x);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |_| reset())
        .on_event_stop(EventListener::FocusLost, move |_| reset())
        .on_event_stop(EventListener::PointerLeave, move |_| reset())
}

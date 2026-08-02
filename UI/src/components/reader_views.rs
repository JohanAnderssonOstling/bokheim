use crate::components::square_button;
use crate::components::text::{subtle_label, ui_label};
use crate::components::tokens::TOKENS;
use crate::components::toolbar_surface;
use crate::style::palette;
use floem::event::{EventListener, EventPropagation};
use floem::prelude::{button, container, h_stack, label, text_input, v_stack, Button, Decorators, RwSignal, SignalGet, SignalUpdate, View, ViewTuple};
use floem::style::{AlignItems, JustifyContent};
use floem::views::{ScrollExt, TextInput};

/// The search text field; turns red while the query has no matches.
/// Key handling and focus behavior are wired by the reader page.
pub fn reader_search_input(search_query_buf: RwSignal<String>, match_info: RwSignal<(usize, usize)>) -> TextInput {
    text_input(search_query_buf).style(move |s| {
        let (_, total) = match_info.get();
        let no_matches = !search_query_buf.get().is_empty() && total == 0;
        let base = s.width(240).padding_horiz(6).padding_vert(4).border(1).border_radius(4);
        if no_matches {
            base.background(palette().danger_bg)
        } else {
            base
        }
    })
}

pub fn reader_search_count(match_info: RwSignal<(usize, usize)>) -> impl View {
    label(move || {
        let (cur, total) = match_info.get();
        if total == 0 {
            String::new()
        } else {
            format!("{}/{}", cur, total)
        }
    })
    .style(|s| s.min_width(40))
}

pub fn reader_search_bar<VT: ViewTuple + 'static>(children: VT) -> impl View {
    h_stack(children).style(|s| {
        let theme = palette();
        s.padding(8).gap(TOKENS.spacing.sm).border_top(1).border_color(theme.border_color).align_items(AlignItems::Center).background(theme.surface_bg)
    })
}

pub fn reader_body(renderer: impl View + 'static, search_bar: impl View + 'static, search_visible: impl Fn() -> bool + 'static) -> impl View {
    v_stack((renderer.style(|s| s.flex_grow(1.0).width_full().min_width(0).min_height(0)), search_bar.style(move |s| if search_visible() { s } else { s.hide() }))).style(|s| s.flex_grow(1.0).width_full().min_width(0).height_full())
}

pub fn reader_title_label(title: impl Fn() -> String + 'static) -> impl View {
    label(title).style(|s| s.min_width(0).flex_grow(1.0).text_ellipsis())
}

pub fn reader_top_bar_left<VT: ViewTuple + 'static>(children: VT) -> impl View {
    h_stack(children).style(|s| s.align_items(AlignItems::Center).gap(TOKENS.spacing.sm).min_width(0).flex_grow(1.0))
}

pub fn reader_top_bar(left: impl View + 'static, right: impl View + 'static) -> impl View {
    toolbar_surface((left, right)).style(|s| s.min_width(0).height(44).justify_content(JustifyContent::SpaceBetween))
}

pub fn reader_line_numbers_label() -> impl View {
    label(|| "Line numbers").style(|s| s.min_width(120))
}

/// On/Off caption button reflecting the line-numbers setting; the reader page
/// wires the click handler.
pub fn reader_line_numbers_toggle(settings_line_numbers: RwSignal<bool>) -> Button {
    button(label(move || if settings_line_numbers.get() { "On".to_string() } else { "Off".to_string() })).style(|s| s.padding_horiz(10).height(28).border(1).border_radius(4))
}

pub fn reader_line_numbers_row<VT: ViewTuple + 'static>(children: VT) -> impl View {
    h_stack(children).style(|s| s.align_items(AlignItems::Center).gap(8.0))
}

pub fn reader_keyboard_shortcuts_title() -> impl View {
    ui_label(|| "Keyboard shortcuts".to_string()).style(|s| s.margin_top(6).font_size(TOKENS.typography.text_sm))
}

pub fn reader_shortcuts_stack<VT: ViewTuple + 'static>(children: VT) -> impl View {
    v_stack(children).style(|s| s.gap(4.0))
}

pub fn reader_settings_dropdown<VT: ViewTuple + 'static>(children: VT, open: impl Fn() -> bool + 'static) -> impl View {
    v_stack(children).style(move |s| {
        let theme = palette();
        if open() {
            s.padding(12).gap(10.0).border_bottom(1).border_color(theme.border_color).background(theme.surface_bg)
        } else {
            s.hide()
        }
    })
}

pub fn reader_toc_panel(content: impl View + 'static) -> impl View {
    container(content).style(|s| s.width_full().min_width(0)).scroll().style(|s| s.width_full().height_full().border_right(1).flex_shrink(0.0))
}

pub fn reader_split_content(left_toc_panel: impl View + 'static, body: impl View + 'static, left_sidebar_width: RwSignal<f64>, toc_open: RwSignal<bool>) -> impl View {
    let dragging = floem::reactive::create_rw_signal(false);
    let last_x = floem::reactive::create_rw_signal(0.0);
    let has_last = floem::reactive::create_rw_signal(false);

    let reset = move || {
        dragging.set(false);
        has_last.set(false);
    };

    let left_panel = container(left_toc_panel).style(move |s| if toc_open.get() { s.width(left_sidebar_width.get()).height_full().flex_grow(0.0).flex_shrink(0.0).min_height(0).min_width(0) } else { s.hide() });
    let handle = container(label(|| ""))
        .style(move |s| {
            if !toc_open.get() {
                return s.hide();
            }
            let theme = palette();
            let base = s.width(8).height_full().cursor(floem::style::CursorStyle::ColResize).background(theme.split_handle_bg);
            if dragging.get() {
                base.background(theme.split_handle_active)
            } else {
                base.hover(|s| s.background(theme.split_handle_hover))
            }
        })
        .on_event_stop(EventListener::PointerDown, move |event| {
            if toc_open.get() && event.point().is_some() {
                dragging.set(true);
                has_last.set(false);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |_| reset());
    let body = container(body).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0));

    h_stack((left_panel, handle, body))
        .style(|s| s.flex_grow(1.0).width_full().min_width(0).height_full().flex_row().min_height(0))
        .on_event_stop(EventListener::PointerMove, move |event| {
            if !toc_open.get() || !dragging.get() {
                return;
            }
            if let Some(point) = event.point() {
                if !has_last.get() {
                    last_x.set(point.x);
                    has_last.set(true);
                    return;
                }
                let delta = point.x - last_x.get();
                let next = (left_sidebar_width.get_untracked() + delta).clamp(200.0, 560.0);
                left_sidebar_width.set(next);
                last_x.set(point.x);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |_| reset())
        .on_event_stop(EventListener::FocusLost, move |_| reset())
        .on_event_stop(EventListener::PointerLeave, move |_| reset())
}

pub fn reader_root<VT: ViewTuple + 'static>(children: VT) -> impl View {
    v_stack(children).style(|s| s.width_full().height_full().flex_grow(1.0).align_items(AlignItems::Stretch))
}

/// History back/forward button that renders disabled while `can_nav` is false;
/// the reader page wires the click handler.
pub fn history_nav_button(glyph: &'static str, can_nav: RwSignal<bool>) -> Button {
    square_button(glyph).style(move |s| {
        let theme = palette();
        if can_nav.get() {
            s
        } else {
            s.color(theme.disabled_text).border_color(theme.disabled_border).background(theme.hover_bg)
        }
    })
}

pub fn shortcut_label(text: &'static str) -> impl View {
    subtle_label(move || text.to_string()).style(|s| s.font_size(TOKENS.typography.text_xs))
}

pub fn stepper_row(title: &'static str, value_label: impl Fn() -> String + 'static, on_decrement: impl Fn() + 'static, on_increment: impl Fn() + 'static) -> impl View {
    h_stack((
        label(move || title).style(|s| s.min_width(120)),
        square_button("-")
            .on_click(move |_| {
                on_decrement();
                EventPropagation::Stop
            })
            .style(|s| s.width(28).height(28)),
        label(value_label).style(|s| s.min_width(72).justify_center()),
        square_button("+")
            .on_click(move |_| {
                on_increment();
                EventPropagation::Stop
            })
            .style(|s| s.width(28).height(28)),
    ))
    .style(|s| s.align_items(AlignItems::Center).gap(8.0))
}

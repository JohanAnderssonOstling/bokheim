use crate::style::BORDER_COLOR;
use floem::peniko::Color;
use floem::prelude::{h_stack, v_stack, Decorators, RwSignal, Scroll, ScrollExt, Stack, ViewTuple};
use floem::style::{AlignItems, JustifyContent};
use floem::views::stack_from_iter;
use floem::IntoView;

pub fn create_button_group<VT: ViewTuple + 'static>(children: VT) -> Stack {
    h_stack(children).style(|s| s.gap(12).align_items(Some(AlignItems::Center)))
}

pub fn create_top_row(content: Stack) -> Stack {
    h_stack((content,)).style(|s| s.height(44).padding_left(16).padding_right(16).border_bottom(1).border_color(BORDER_COLOR).justify_content(Some(JustifyContent::SpaceBetween)).align_items(Some(AlignItems::Center)))
}

pub fn create_list_header<VT: ViewTuple + 'static>(children: VT) -> Stack {
    h_stack(children).style(|s| s.height(44).padding_left(16).padding_right(16).border_bottom(1).border_color(BORDER_COLOR).justify_content(Some(JustifyContent::SpaceBetween)))
}

pub fn create_list_bodyr<V>(iterator: impl IntoIterator<Item = V>) -> Scroll
where
    V: IntoView + 'static,
{
    stack_from_iter(iterator).style(|s| s.flex_col().gap(8).width_full()).scroll().style(|s| s.flex_grow(1.0).width_full())
}

pub fn create_list<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children).style(|s| {
        s.size_pct(50.0, 50.0)
            .min_width(320)
            .min_height(240)
            .padding(24)
            .gap(16)
            .border(1)
            .border_color(BORDER_COLOR)
            .border_radius(16)
            .background(Color::WHITE)
            .box_shadow_color(Color::rgba8(0, 0, 0, 150))
            .box_shadow_blur(28)
            .box_shadow_h_offset(0)
            .box_shadow_v_offset(16)
    })
}

pub fn create_centered_stack<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children).style(|s| s.size_full().items_center().justify_center())
}

pub fn create_full_stack<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children).style(|s| s.width_full().height_full().flex_grow(1.0))
}

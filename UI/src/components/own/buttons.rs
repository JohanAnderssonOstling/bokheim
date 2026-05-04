use crate::style::{ACTIVE_HIGHLIGHT, HOVER_BG, HOVER_COLOR};
use floem::peniko::Color;
use floem::prelude::{button, label, v_stack, Button, Decorators};
use floem::style::{AlignContent, CursorStyle};
use floem::views::svg;

pub fn create_button(text: String) -> Button {
    button(label(move || text.clone()))
}

pub fn create_list_item_button(text: String) -> Button {
    create_button(text).style(|s| s.flex_grow(1.0).padding(8).border_radius(6).justify_start().cursor(CursorStyle::Pointer).hover(|s| s.background(HOVER_BG)))
}

pub fn create_square_button(text: String) -> Button {
    button(label(move || text.clone())).style(move |s| s.hover(|s| s.background(HOVER_COLOR)).text_ellipsis().font_size(20).width(36).height(36).border_radius(8).justify_center().align_content(AlignContent::Center))
}

pub fn create_danger_button(text: &'static str) -> Button {
    button(label(move || text)).style(|s| s.padding_horiz(8).padding_vert(6).border_radius(6).border(1).border_color(Color::rgb8(220, 38, 38)).color(Color::rgb8(220, 38, 38)).hover(|s| s.background(Color::rgb8(254, 226, 226))))
}

pub fn create_tab_button(text: &'static str, is_active: impl Fn() -> bool + 'static) -> Button {
    button(label(move || text)).style(move |s| {
        let s = s.padding_horiz(16).padding_vert(10).cursor(CursorStyle::Pointer);
        if is_active() {
            s.background(ACTIVE_HIGHLIGHT).border_bottom(2).border_color(Color::rgb8(99, 102, 241))
        } else {
            s.hover(|s| s.background(HOVER_BG))
        }
    })
}

pub fn create_sidebar_nav_button(text: &'static str, icon_svg: &'static str, is_active: impl Fn() -> bool + 'static) -> Button {
    button(v_stack((svg(icon_svg.to_string()).style(|s| s.size(22, 22)), label(move || text).style(|s| s.font_size(12))))).style(move |s| {
        let s = s.width(92).height(84).border(1).cursor(CursorStyle::Pointer).flex_col().justify_center().items_center().gap(8);
        if is_active() {
            s.background(ACTIVE_HIGHLIGHT).border_color(Color::rgb8(99, 102, 241))
        } else {
            s.background(Color::WHITE).border_color(Color::rgb8(229, 231, 235)).hover(|s| s.background(HOVER_BG))
        }
    })
}

pub fn add_library_button() -> Button {
    button(label(|| "Add library")).style(|s| s.padding_left(10).padding_right(10).padding_top(6).padding_bottom(6).border_radius(6).border(1).background(Color::WHITE).cursor(CursorStyle::Pointer).hover(|s| s.background(HOVER_BG)))
}

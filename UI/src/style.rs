use floem::event::EventPropagation;
use floem::peniko::Color;
use floem::prelude::{button, h_stack, Button, Decorators, RwSignal, SignalGet, SignalUpdate, Stack, ViewTuple};
use floem::style::{AlignContent, AlignItems, CursorStyle, JustifyContent, StyleValue};
use floem::text::Weight;
use floem::views::{label, Img, Label};

pub const HOVER_COLOR: Color = Color { r: 240, g: 240, b: 240, a: 255 };
const TEXT_COLOR: Color = Color { r: 43, g: 43, b: 43, a: 255 };

pub const BOX_SHADOW: Color = Color { r: 0, g: 0, b: 0, a: 150 };
pub const BORDER_COLOR: Color = Color::rgb8(229, 231, 235);
pub const HOVER_BG: Color = Color::rgb8(243, 244, 246);
pub const PANEL_BG: Color = Color::rgb8(249, 250, 251);
pub const ACTIVE_HIGHLIGHT: Color = Color::rgb8(240, 244, 255);
pub const DISABLED_TEXT: Color = Color::rgb8(156, 163, 175);
pub const DISABLED_BORDER: Color = Color::rgb8(209, 213, 219);
pub const SUBTLE_TEXT: Color = Color::rgb8(75, 85, 99);

pub fn create_label(text: String, font_size: i32) -> Label {
    label(move || text.clone()).style(move |s| s.hover(|s| s.background(HOVER_COLOR)).text_ellipsis().font_size(font_size).font_family("Liberation Serif".to_string()).font_weight(Weight::LIGHT).color(TEXT_COLOR))
}

pub fn label_style(label: Label, font_size: i32) -> Label {
    label.style(move |s| s.hover(|s| s.background(HOVER_COLOR)).text_ellipsis().font_size(font_size))
}

pub fn create_book_title_label(title: String) -> Label {
    label(move || title.clone()).style(|s| s.width(200).height(40).font_size(16).text_ellipsis().margin_top(10))
}

pub fn style_book_cover(img: Img) -> Img {
    img.style(move |s| s.width(200).height(320).border_radius(6).border_color(Color::BLACK).box_shadow_blur(8).box_shadow_color(BOX_SHADOW).box_shadow_spread(0).box_shadow_h_offset(6).box_shadow_v_offset(12).cursor(CursorStyle::Pointer))
}

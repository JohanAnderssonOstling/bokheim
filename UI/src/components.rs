use floem::peniko::Color;
use floem::prelude::{button, h_stack, Button, Decorators, Stack};
use floem::style::{AlignContent, AlignItems, CursorStyle, JustifyContent, StyleValue};
use floem::text::Weight;
use floem::views::{label, Img, Label};

const BACKGROUND_COLOR: Color = Color::WHITE;
const HOVER_COLOR: Color = Color {
    r: 240,
    g: 240,
    b: 240,
    a: 255,
};
const Text_COLOR: Color = Color {
    r: 43,
    g: 43,
    b: 43,
    a: 255,
};

pub const BOX_SHADOW: Color = Color {r: 0, g: 0, b: 0, a: 150};

pub fn create_label(text: String, font_size: i32) -> Label {
    label(move || text.clone()).style(move |s| {
        s.hover(|s| s.background(HOVER_COLOR))
            .text_ellipsis()
            .font_size(font_size)
            .font_family("Liberation Serif".to_string())
            .font_weight(Weight::LIGHT)
            .color(Text_COLOR)
    })
}

pub fn label_style(label: Label, font_size: i32) -> Label {
    label.style(move |s| {
        s.hover(|s| s.background(HOVER_COLOR))
            .text_ellipsis()
            .font_size(font_size)
    })
}

pub fn create_square_button(text: String) -> Button {
    button(label(move || text.clone())).style(move |s| {
        s.hover(|s| s.background(HOVER_COLOR))
            .text_ellipsis()
            .font_size(20)
            .width(36)
            .height(36)
            .border_radius(8)
            .justify_center()
            .align_content(AlignContent::Center)
    })
}

pub fn create_book_title_label(title: String) -> Label {
    label(move || title.clone()).style(|s| {
        s.width(200)
            .height(40)
            .font_size(16)
            .text_ellipsis()
            .margin_top(10)
    })
}

pub fn create_book_cover_style() {

}

pub fn add_library_button() -> Button {
    button(label(|| "Add library"))
        .style(|s| {
            s.padding_left(10)
                .padding_right(10)
                .padding_top(6)
                .padding_bottom(6)
                .border_radius(6)
                .border(1)
                .background(Color::WHITE)
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.background(Color::rgb8(243, 244, 246)))
        })
}

pub fn create_top_row(content: Stack) -> Stack {
    h_stack((content,)).style(|s| {
        s.height(44)
            .padding_left(16)
            .padding_right(16)
            .border_bottom(1)
            .border_color(Color::rgb8(229, 231, 235))
            .justify_content(Some(JustifyContent::SpaceBetween))
            .align_items(Some(AlignItems::Center))
    })
}

pub fn style_book_cover(img: Img) -> Img {
    img.style(move |s| {
        s.width(200)
            .height(320)
            .border_radius(6)
            .border_color(Color::BLACK)
            .box_shadow_blur(8)
            .box_shadow_color(BOX_SHADOW)
            .box_shadow_spread(0)
            .box_shadow_h_offset(6)
            .box_shadow_v_offset(12)
            .cursor(CursorStyle::Pointer)

    })
}

//pub fn create_book_cover
use crate::style::{BORDER_COLOR, PANEL_BG};
use floem::prelude::{label, Decorators, Label};

pub fn create_header_label(text: String) -> Label {
    label(move || text.clone()).style(|s| s.font_size(20))
}

pub fn create_label_background(text: String) -> Label {
    create_label(text, 20).style(|s| s.padding(16).border_radius(8).background(PANEL_BG).border(1).border_color(BORDER_COLOR))
}

pub fn create_padded_label(text: String) -> Label {
    label(move || text.clone()).style(|s| s.padding(24).width_full())
}

pub fn create_label(text: String, font_size: i32) -> Label {
    label(move || text.clone()).style(move |s| s.font_size(font_size))
}

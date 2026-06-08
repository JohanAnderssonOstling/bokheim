use crate::style::palette;
use floem::prelude::{label, Decorators, Label};

pub fn create_header_label(text: String) -> Label {
    label(move || text.clone()).style(|s| {
        let theme = palette();
        s.font_size(20).font_family(theme.title_font_family.to_string()).color(theme.ink_text)
    })
}

pub fn create_label_background(text: String) -> Label {
    create_label(text, 20).style(|s| {
        let theme = palette();
        s.padding(16).border_radius(8).background(theme.surface_bg).border(1).border_color(theme.border_color)
    })
}

pub fn create_padded_label(text: String) -> Label {
    label(move || text.clone()).style(|s| {
        let theme = palette();
        s.padding(24).width_full().font_family(theme.ui_font_family.to_string()).color(theme.ink_text)
    })
}

pub fn create_label(text: String, font_size: i32) -> Label {
    label(move || text.clone()).style(move |s| {
        let theme = palette();
        s.font_size(font_size).font_family(theme.ui_font_family.to_string()).color(theme.ink_text)
    })
}

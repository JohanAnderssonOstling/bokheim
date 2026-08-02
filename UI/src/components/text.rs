use crate::components::tokens::TOKENS;
use crate::style::{heading_font_family, interface_font_family, palette};
use floem::prelude::{label, Decorators, Label};

// Base labels. All text components derive from one of these three instead of
// restating font family / color / size.

/// Base for body text: UI font family, ink color, body size.
pub fn ui_label(text: impl Fn() -> String + 'static) -> Label {
    label(text).style(|s| {
        let theme = palette();
        s.font_family(interface_font_family()).color(theme.ink_text).font_size(TOKENS.typography.text_md)
    })
}

/// Base for headings: title font family, ink color, title size.
pub fn title_label(text: impl Fn() -> String + 'static) -> Label {
    label(text).style(|s| {
        let theme = palette();
        s.font_family(heading_font_family()).color(theme.ink_text).font_size(TOKENS.typography.title_md)
    })
}

/// Body text in the subdued color.
pub fn subtle_label(text: impl Fn() -> String + 'static) -> Label {
    ui_label(text).style(|s| s.color(palette().subtle_text))
}

// Specializations.

pub fn body_text(text: impl Into<String>) -> Label {
    let text = text.into();
    ui_label(move || text.clone())
}

pub fn padded_text(text: impl Into<String>) -> Label {
    body_text(text).style(|s| s.padding(TOKENS.spacing.xl).width_full())
}

pub fn label_card(text: impl Into<String>) -> Label {
    body_text(text).style(|s| {
        let theme = palette();
        s.font_size(TOKENS.typography.title_md).padding(TOKENS.spacing.lg).border_radius(TOKENS.radius.md).background(theme.surface_bg).border(1).border_color(theme.border_color)
    })
}

pub fn modal_title(text: &'static str) -> Label {
    title_label(move || text.to_string()).style(|s| s.padding_bottom(TOKENS.spacing.sm))
}

pub fn switcher_title(text: &'static str) -> Label {
    title_label(move || text.to_string()).style(|s| s.font_size(22).color(palette().subtle_text))
}

pub fn danger_text(text: impl Into<String>) -> Label {
    let text = text.into();
    ui_label(move || text.clone()).style(|s| s.color(palette().danger_text).padding_top(TOKENS.spacing.xs))
}

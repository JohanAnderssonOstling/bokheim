use crate::components::tokens::TOKENS;
use crate::style::{interface_font_family, palette};
use floem::prelude::{button, label, Button, Decorators};
use floem::style::{AlignContent, CursorStyle};

/// Base for all buttons: caption font + control chrome. Variants derive from
/// this (directly or via `secondary_button`) instead of restating styling.
/// The caption deliberately sets no text color so the button-level `color`
/// (ink, danger, disabled, ...) cascades into it.
fn base_button(text: impl Fn() -> String + 'static) -> Button {
    button(label(text).style(|s| s.font_family(interface_font_family()).font_size(TOKENS.typography.text_md))).style(|s| {
        let theme = palette();
        s.height(TOKENS.size.control_md)
            .padding_horiz(TOKENS.spacing.md)
            .padding_vert(TOKENS.spacing.sm)
            .border(1)
            .border_radius(TOKENS.radius.md)
            .cursor(CursorStyle::Pointer)
            .justify_center()
            .items_center()
            .background(theme.white_surface)
            .border_color(theme.border_color)
            .color(theme.ink_text)
            .hover(|s| s.background(theme.hover_bg))
    })
}

pub fn secondary_button(text: impl Into<String>) -> Button {
    let text = text.into();
    base_button(move || text.clone())
}

/// A secondary button whose caption is recomputed reactively.
pub fn dynamic_button(text: impl Fn() -> String + 'static) -> Button {
    base_button(text)
}

pub fn compact_button(text: impl Into<String>) -> Button {
    secondary_button(text).style(|s| s.height(TOKENS.size.control_sm))
}

pub fn dialog_tab_button(text: impl Into<String>) -> Button {
    secondary_button(text).style(|s| s.height(30))
}

pub fn list_item_button(text: impl Into<String>) -> Button {
    secondary_button(text).style(|s| s.flex_grow(1.0).padding(TOKENS.spacing.sm).border_radius(TOKENS.radius.sm).justify_start())
}

pub fn square_button(text: impl Into<String>) -> Button {
    secondary_button(text).style(|s| s.width(TOKENS.size.control_md).height(TOKENS.size.control_md).padding(0).text_ellipsis().font_size(TOKENS.typography.title_md).justify_center().align_content(AlignContent::Center))
}

pub fn danger_button(text: impl Into<String>) -> Button {
    let text = text.into();
    base_button(move || text.clone()).style(|s| {
        let theme = palette();
        s.border_color(theme.danger_text).color(theme.danger_text).hover(|s| s.background(theme.danger_bg))
    })
}

/// Fixed-width "Delete" button used in switcher rows so rows align.
pub fn switcher_delete_button() -> Button {
    danger_button("Delete").style(|s| s.width(88).flex_shrink(0.0))
}

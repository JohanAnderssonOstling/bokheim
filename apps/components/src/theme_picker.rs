//! Compact palette samples shared by Browse and Reader settings.

use app_preferences::{ApplicationTheme, ResolvedAppearance};
use gpui::prelude::*;
use gpui::{Div, ElementId, px, rgb};
use gpui_component::button::{Button, ButtonVariants};

use crate::theme::palette;

const SWATCH_SIZE: f32 = 48.0;
const MAX_GAP: f32 = 24.0;
pub const THEME_PICKER_WIDTH: f32 = 4.0 * SWATCH_SIZE + 3.0 * MAX_GAP;

pub const THEME_CHOICES: [(ApplicationTheme, ResolvedAppearance); 4] = [
    (ApplicationTheme::Warm, ResolvedAppearance::Light),
    (ApplicationTheme::Cool, ResolvedAppearance::Light),
    (ApplicationTheme::Warm, ResolvedAppearance::Dark),
    (ApplicationTheme::Cool, ResolvedAppearance::Dark),
];

/// Four fixed-size squares. The space between them expands with the panel,
/// stopping at 24 px; the samples themselves never stretch.
pub fn theme_picker_group() -> Div {
    gpui::div().w(px(THEME_PICKER_WIDTH)).max_w_full().flex().flex_row().items_center().justify_between()
}

pub fn theme_picker_swatch(id: impl Into<ElementId>, theme: ApplicationTheme, appearance: ResolvedAppearance, selected: bool) -> Button {
    let colors = palette(theme, appearance);
    let page = rgb(colors.surface_bg);
    let ink = rgb(colors.ink_text);
    let accent = rgb(colors.accent);
    let label = format!("{} {}", theme.label(), if appearance == ResolvedAppearance::Light { "light" } else { "dark" });

    let letters = gpui::div()
        .flex()
        .items_baseline()
        .font_family(app_preferences::INTERFACE_FONT_FAMILY)
        .text_color(ink)
        .child(gpui::div().text_size(px(29.0)).child("A"))
        .child(gpui::div().text_size(px(16.0)).child("a"));

    let sample = gpui::div()
        .relative()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(letters)
        .child(gpui::div().absolute().bottom(px(6.0)).left(px(14.0)).w(px(18.0)).h(px(3.0)).bg(accent))
        .when(selected, |sample| {
            sample.child(gpui::div().absolute().top(px(2.0)).right(px(2.0)).size(px(14.0)).flex().items_center().justify_center().bg(accent).text_color(page).text_size(px(10.0)).child("✓"))
        });

    crate::base_button(id)
        .ghost()
        .size(px(SWATCH_SIZE))
        .flex_none()
        .p_0()
        .rounded(px(0.0))
        .border_1()
        .border_color(if selected { accent } else { rgb(colors.border) })
        .bg(page)
        .tooltip(label)
        .child(sample)
}

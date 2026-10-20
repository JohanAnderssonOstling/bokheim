//! Choice controls shared by application and reader settings.

use gpui::prelude::*;
use gpui::{Div, ElementId, SharedString, div, px, rems};
use gpui_component::Selectable;
use gpui_component::button::{Button, ButtonVariants};

use crate::{BrowserTheme, RADIUS_SM, SETTINGS_CONTROL_GAP, SPACE_SM, TEXT_SM, base_button, settings_control_size_rem};

/// One quiet outline with single-pixel separators between equal-width choices.
/// Callers choose the width to suit their settings surface.
pub fn settings_choice_button_group(theme: BrowserTheme) -> Div {
    div().min_w_0().flex().flex_row().gap(px(SETTINGS_CONTROL_GAP)).overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.rule).bg(theme.rule)
}

/// The same outline as [`settings_choice_button_group`] laid out as a grid,
/// for a list of independent on/off choices too long for one row.
pub fn settings_choice_button_grid(columns: u16, theme: BrowserTheme) -> Div {
    div().w_full().grid().grid_cols(columns).gap(px(SETTINGS_CONTROL_GAP)).overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.rule).bg(theme.rule)
}

/// A selected choice has a tint and an accent underline. Marking it selected
/// leaves its hover slot available, so hovering preserves both marks.
pub fn settings_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    base_button(id)
        .label(label)
        .ghost()
        .selected(selected)
        .flex_1()
        .min_w_0()
        .h(rems(settings_control_size_rem()))
        .px(px(SPACE_SM))
        .rounded(px(0.0))
        .border_0()
        // On Kobo the underline alone marks the choice; nothing is filled.
        .bg(if selected && !cfg!(feature = "kobo") { theme.accent_wash } else { theme.page_bg })
        .text_color(theme.text)
        .text_size(rems(TEXT_SM))
        .when(selected, |button| button.border_b(px(2.0)).border_color(theme.accent).hover(move |style| style.bg(if cfg!(feature = "kobo") { theme.hover } else { theme.accent_wash_hover })))
}

//! Choice controls shared by application and reader settings.

use crate::Button;
use gpui::prelude::*;
use gpui::{Div, ElementId, IntoElement, SharedString, div, px, rems};
use gpui_component::Selectable;

use crate::{BrowserTheme, RADIUS_SM, SETTINGS_CONTROL_GAP, SPACE_SM, TEXT_SM, base_button, settings_control_size_rem};

/// One quiet outline with single-pixel separators between equal-width choices.
/// Callers choose the width to suit their settings surface.
pub fn settings_choice_button_group(theme: BrowserTheme) -> Div {
    settings_control_row().gap(px(SETTINGS_CONTROL_GAP)).overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.rule).bg(theme.rule)
}

/// A single segmented row owns its outer height, including its outline.
/// Tracks share width equally until a segment's contents require more room.
pub fn settings_choice_row(columns: u16, theme: BrowserTheme) -> Div {
    settings_choice_button_group(theme).h(rems(settings_control_size_rem())).grid().grid_cols_min_content(columns)
}

/// Buttons and composite segments share the same selected surface.
pub fn settings_choice_surface<T: Styled>(surface: T, selected: bool, theme: BrowserTheme) -> T {
    if !selected {
        return surface;
    }
    if cfg!(feature = "kobo") { surface.bg(theme.page_bg).text_color(theme.text).border_0().border_b(px(2.0)).border_color(theme.accent) } else { surface.bg(theme.accent).text_color(theme.accent_text) }
}

/// Controls belonging to one setting always share a single row.
pub fn settings_control_row() -> Div {
    div().w_full().min_w_0().flex().flex_row().flex_nowrap()
}

/// The same outline as [`settings_choice_button_group`] laid out as a grid,
/// for a list of independent on/off choices too long for one row.
pub fn settings_choice_button_grid(columns: u16, theme: BrowserTheme) -> Div {
    settings_choice_button_group(theme).grid().grid_cols(columns)
}

/// Chosen values share the heavy accent used by selected buttons.
/// Kobo retains its underline so changing a setting avoids a solid e-ink fill.
pub fn settings_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    settings_choice_button_content(id, div().min_w_0().max_w_full().whitespace_nowrap().overflow_hidden().text_ellipsis().text_align(gpui::TextAlign::Center).child(label.clone()), selected, theme).aria_label(label)
}

/// A segment with custom content, sharing the text choices' geometry and states.
pub fn settings_choice_button_content(id: impl Into<ElementId>, content: impl IntoElement, selected: bool, theme: BrowserTheme) -> Button {
    let button = base_button(id).child(content).ghost().selected(selected).flex_1().min_w_0().h_auto().min_h(rems(settings_control_size_rem())).px(px(SPACE_SM)).rounded(px(0.0)).border_0().bg(theme.page_bg).text_size(rems(TEXT_SM));
    settings_choice_surface(button, selected, theme)
}

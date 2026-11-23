//! One stepper shape for browsing and reading preferences.

use crate::Button;
use gpui::prelude::*;
use gpui::{Div, ElementId, SharedString, div, px, rems};
use gpui_component::Disableable;

use crate::{BrowserTheme, RADIUS_SM, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_SM, outlined_text_button, settings_stepper_size_rem};

/// A borderless nudge button inside the stepper's single outer outline.
pub fn settings_stepper_button(id: impl Into<ElementId>, label: impl Into<SharedString>, disabled: bool) -> Button {
    outlined_text_button(id, label).ghost().disabled(disabled).flex_basis(rems(settings_stepper_size_rem())).h_full().min_h_0().min_w_auto().flex_shrink(1.0).px(px(SPACE_SM)).rounded(px(0.0)).text_size(rems(TEXT_SM))
}

/// Numeric values retain their full text instead of behaving like clipped choice labels.
pub fn settings_stepper_value_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    let label = label.into();
    crate::settings_choice_button_content(id, div().whitespace_nowrap().child(label.clone()), false, theme).aria_label(label).flex_auto().min_w(rems(settings_stepper_size_rem())).flex_shrink(0.0)
}

/// A content-sized stepper with a single outline and square nudge buttons.
pub fn settings_stepper(decrement: Button, value: impl IntoElement, increment: Button, theme: BrowserTheme) -> Div {
    div()
        .flex_none()
        .w_auto()
        .self_start()
        .flex()
        .items_center()
        .h(rems(settings_stepper_size_rem()))
        .px(px(SPACE_XXS))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .bg(theme.page_bg)
        .child(decrement)
        .child(div().flex_auto().min_w_auto().px(px(SPACE_XS)).h_full().flex().items_center().justify_center().whitespace_nowrap().text_align(gpui::TextAlign::Center).text_size(rems(TEXT_SM)).text_color(theme.text_muted).child(value))
        .child(increment)
}

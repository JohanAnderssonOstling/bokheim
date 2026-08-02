use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, SharedString, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::{Scrollable, ScrollableElement};
use gpui_component::{Disableable, IconName};

use crate::BrowserTheme;

pub fn cloud_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().p(px(24.0)).overflow_y_scrollbar()
}

pub fn cloud_page_content() -> Div {
    div().w_full().max_w(px(920.0)).mx_auto().flex().flex_col().gap(px(18.0))
}

pub fn cloud_page_title(title: impl Into<SharedString>, description: impl Into<SharedString>, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(div().text_size(gpui::rems(26.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family(font_family).child(title.into()))
        .child(div().text_size(gpui::rems(13.0 / 16.0)).text_color(theme.text_muted).child(description.into()))
}

pub fn cloud_section(theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(14.0)).p(px(18.0)).rounded(px(14.0)).border_1().border_color(theme.border).bg(theme.card_bg)
}

pub fn cloud_section_header(title: impl Into<SharedString>, detail: impl Into<SharedString>, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(div().text_size(gpui::rems(19.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family(font_family).child(title.into()))
        .child(div().text_size(gpui::rems(12.0 / 16.0)).text_color(theme.text_muted).child(detail.into()))
}

pub fn cloud_form() -> Div {
    div().w_full().flex().flex_col().gap(px(11.0))
}

pub fn cloud_field(label: impl Into<SharedString>, control: impl IntoElement, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(5.0)).child(div().text_size(gpui::rems(12.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(label.into())).child(div().w_full().max_w(px(520.0)).child(control))
}

pub fn cloud_actions() -> Div {
    div().flex().items_center().gap(px(9.0)).flex_wrap()
}

pub fn cloud_primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).label(label).icon(icon).primary().h(px(40.0)).px(px(13.0)).disabled(disabled)
}

pub fn cloud_secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).label(label).icon(icon).secondary().outline().h(px(40.0)).px(px(13.0)).disabled(disabled)
}

pub fn cloud_message(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().text_size(gpui::rems(12.0 / 16.0)).text_color(theme.accent).child(text.into())
}

pub fn cloud_error(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().text_size(gpui::rems(12.0 / 16.0)).text_color(theme.danger).child(text.into())
}

pub fn cloud_list() -> Div {
    div().w_full().flex().flex_col().gap(px(8.0))
}

pub fn cloud_row(theme: BrowserTheme) -> Div {
    div().w_full().min_h(px(58.0)).px(px(12.0)).py(px(9.0)).flex().items_center().gap(px(12.0)).rounded(px(10.0)).border_1().border_color(theme.border).bg(theme.surface_alt)
}

pub fn cloud_row_identity(title: impl Into<SharedString>, detail: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(div().line_clamp(1).text_size(gpui::rems(14.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(div().line_clamp(1).text_size(gpui::rems(11.0 / 16.0)).text_color(theme.text_muted).child(detail.into()))
}

pub fn cloud_badge(text: impl Into<SharedString>, highlighted: bool, theme: BrowserTheme) -> Div {
    div()
        .flex_none()
        .px(px(8.0))
        .py(px(4.0))
        .rounded_full()
        .bg(if highlighted { theme.accent_soft } else { theme.surface })
        .text_color(if highlighted { theme.accent } else { theme.text_muted })
        .text_size(gpui::rems(10.0 / 16.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

pub fn cloud_empty(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().min_h(px(64.0)).flex().items_center().text_size(gpui::rems(13.0 / 16.0)).text_color(theme.text_muted).child(text.into())
}

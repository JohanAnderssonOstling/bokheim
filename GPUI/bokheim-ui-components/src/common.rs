use gpui::prelude::*;
use gpui::{AnyElement, App, Div, ElementId, FontWeight, SharedString, Stateful, div, px};
use gpui_component::IconName;
use gpui_component::Sizable;
use gpui_component::button::{Button, ButtonVariants};

use crate::{ACCENT_SOFT, BORDER, BrowserTheme, NAV_RAIL_WIDTH, PAGE_BG, SURFACE, TEXT, TEXT_MUTED};

pub fn page() -> Div {
    div().size_full().flex().flex_col().bg(gpui::rgb(PAGE_BG)).text_color(gpui::rgb(TEXT)).font_family("IBM Plex Sans")
}

pub fn browser_page(font_family: impl Into<SharedString>, font_size_px: u8, theme: BrowserTheme) -> Div {
    div().size_full().min_h_0().min_w_0().relative().flex().flex_col().overflow_hidden().bg(theme.page_bg).text_color(theme.text).font_family(font_family).text_size(px(font_size_px as f32))
}

pub fn browser_loading_placeholder(theme: BrowserTheme) -> AnyElement {
    div().size_full().min_h_0().flex().items_center().justify_center().child(browser_inline_message("Loading…", theme)).into_any_element()
}

pub fn browser_error_banner(error: Option<SharedString>, retry_label: SharedString, theme: BrowserTheme, body: AnyElement, on_retry: impl Fn(&mut App) + 'static) -> AnyElement {
    let Some(error) = error else {
        return body;
    };
    let retry = crate::topbar_action_button("retry-section", retry_label, IconName::Redo2, theme).on_click(move |_, _, cx| on_retry(cx));
    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(div().w_full().flex().items_center().justify_between().gap(px(10.0)).px(px(10.0)).py(px(7.0)).rounded(px(8.0)).bg(theme.surface_alt).child(browser_inline_message(error, theme)).child(retry))
        .child(div().flex_1().min_h_0().child(body))
        .into_any_element()
}

pub fn toolbar() -> Div {
    div().h(px(52.0)).flex().flex_row().items_center().gap(px(14.0)).px(px(18.0)).bg(gpui::rgb(SURFACE)).border_b_1().border_color(gpui::rgb(BORDER))
}

pub fn sidebar(theme: BrowserTheme) -> Div {
    div().w(px(NAV_RAIL_WIDTH)).h_full().min_h_0().flex_none().flex().flex_col().gap_0().px(px(8.0)).bg(theme.page_bg).border_r_1().border_color(theme.border)
}

pub fn content() -> Div {
    div().flex_1().min_w_0().min_h_0().flex().flex_col().p(px(18.0))
}

pub fn title(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(24.0 / 16.0)).font_weight(FontWeight::BOLD).font_family("Source Serif 4").child(text.into())
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(18.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family("Source Serif 4").child(text.into())
}

pub fn browser_section_title(text: impl Into<SharedString>, font_family: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(18.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family(font_family).child(text.into())
}

pub fn muted(text: impl Into<SharedString>) -> Div {
    div().text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn status(text: impl Into<SharedString>) -> Div {
    div().flex_1().flex().items_center().justify_center().text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn surface() -> Div {
    div().bg(gpui::rgb(SURFACE)).border_1().border_color(gpui::rgb(BORDER)).rounded(px(8.0))
}

pub fn nav_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(NAV_RAIL_WIDTH))
        .h(px(88.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(9.0))
        .text_size(gpui::rems(12.0 / 16.0))
        .border_1()
        .border_color(gpui::rgb(BORDER))
        .cursor_pointer()
        .when(selected, |item| item.bg(gpui::rgb(ACCENT_SOFT)))
        .when(!selected, |item| item.bg(gpui::rgb(SURFACE)).hover(|style| style.bg(gpui::rgb(PAGE_BG))))
        .child(label.into())
}

pub fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).with_size(gpui_component::Size::Small).secondary().outline()
}

pub fn spacer() -> Div {
    div().flex_1().min_h_0()
}

pub fn inline_message(text: impl Into<SharedString>) -> Div {
    div().w_full().px(px(9.0)).py(px(5.0)).text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn browser_inline_message(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().px(px(9.0)).py(px(5.0)).text_color(theme.text_muted).child(text.into())
}

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, Div, FontWeight, SharedString, Window};
use gpui_component::alert::Alert;
use gpui_component::IconName;
use gpui_component::Sizable;
use gpui_component::Size;
use gpui_component::Theme;

use crate::{BrowserTheme, Scrollable, ScrollableElement, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, TEXT_LG, TEXT_SM, TEXT_XS, TITLE_MD};

/// The window-sized application root shared by browser views.
pub fn app_page(font_family: impl Into<SharedString>, font_size_px: u8, theme: BrowserTheme) -> Div {
    div().size_full().min_h_0().min_w_0().relative().flex().flex_col().overflow_hidden().bg(theme.page_bg).text_color(theme.text).font_family(font_family).text_size(px(font_size_px as f32))
}

/// A reader-window root using the application-wide interface typography.
pub fn reader_app_page(window: &mut Window, cx: &App, theme: BrowserTheme) -> Div {
    let application_theme = Theme::global(cx);
    let font_size = application_theme.font_size;
    let font_family = application_theme.font_family.clone();
    window.set_rem_size(font_size);
    div().size_full().min_h_0().min_w_0().relative().flex().flex_col().overflow_hidden().bg(theme.page_bg).text_color(theme.text).font_family(font_family).text_size(font_size)
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
        .gap(px(SPACE_SM))
        .child(div().w_full().flex().items_center().justify_between().gap(px(SPACE_SM)).child(Alert::error("browser-error", error).with_size(Size::Small)).child(retry))
        .child(div().flex_1().min_h_0().child(body))
        .into_any_element()
}

pub fn sidebar(width: f32, theme: BrowserTheme) -> Scrollable<Div> {
    let mut separator = theme.border;
    separator.a *= 0.55;
    div().w(px(width)).h_full().min_h_0().flex_none().flex().flex_col().gap_0().border_r_1().border_color(separator).bg(theme.page_bg).overflow_y_scroll_hidden()
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(TEXT_LG / 16.0)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn scrollable_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().p(px(SPACE_LG)).overflow_y_scrollbar()
}

pub fn centered_page_content() -> Div {
    div().w_full().max_w(px(920.0)).mx_auto().flex().flex_col().gap(px(SPACE_LG))
}

pub fn section_header(title: impl Into<SharedString>, detail: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let detail = detail.into();
    let mut header = div().flex().flex_col().gap(px(SPACE_XXS)).child(div().text_size(gpui::rems(TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()));
    if !detail.is_empty() {
        header = header.child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(detail));
    }
    header
}

pub fn action_row() -> Div {
    div().flex().items_center().gap(px(SPACE_SM)).flex_wrap()
}

pub fn item_list() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM))
}

pub fn item_row(theme: BrowserTheme) -> Div {
    div().w_full().min_h(px(58.0)).px(px(SPACE_XS)).py(px(SPACE_SM)).flex().items_center().gap(px(SPACE_MD)).border_b_1().border_color(theme.border)
}

pub fn item_identity(title: impl Into<SharedString>, detail: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let detail = detail.into();
    let mut identity = div().flex_1().min_w_0().flex().flex_col().gap(px(SPACE_XXS)).child(div().line_clamp(1).text_size(gpui::rems(TEXT_SM / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()));
    if !detail.is_empty() {
        identity = identity.child(div().line_clamp(1).text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(detail));
    }
    identity
}

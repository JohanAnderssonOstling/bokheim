use gpui::prelude::*;
use gpui::{Div, FontWeight, SharedString, div, px};

use crate::{BrowserTheme, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, Scrollable, ScrollableElement, TEXT_SM, TEXT_XS, TITLE_MD};

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

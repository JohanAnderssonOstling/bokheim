use gpui::prelude::*;
use gpui::{AnyElement, App, Div, ElementId, FontWeight, SharedString, Window, div, px};
use gpui_component::Disableable;
use gpui_component::IconName;
use gpui_component::Sizable;
use gpui_component::Size;
use gpui_component::Theme;
use gpui_component::alert::Alert;
use gpui_component::button::{Button, ButtonVariants};

use crate::{BrowserTheme, SPACE_MD, SPACE_SM, Scrollable, ScrollableElement, TEXT_LG, TOPBAR_ACTION_HEIGHT};

pub fn app_page(font_family: impl Into<SharedString>, font_size_px: u8, theme: BrowserTheme) -> Div {
    div().size_full().min_h_0().min_w_0().relative().flex().flex_col().overflow_hidden().bg(theme.page_bg).text_color(theme.text).font_family(font_family).text_size(px(font_size_px as f32))
}

/// A reader-window root using the application-wide interface typography.
///
/// Reader content owns its typography separately. This root only establishes
/// the font family and rem basis used by reader chrome, panels, and popovers.
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

/// Titles are the interface font at a larger size and heavier weight.
///
/// This used to name "Source Serif 4", which is not among the fonts `add_fonts`
/// installs, so it resolved to whatever fontconfig happened to pick. The
/// editorial serif/sans pairing it implied was attempted in two places and
/// achieved in none; the rest of the app already set titles in the body face.
pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(TEXT_LG / 16.0)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub(crate) fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).with_size(gpui_component::Size::Small).secondary().outline()
}

pub fn primary_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).label(label).icon(icon).primary().h(px(TOPBAR_ACTION_HEIGHT)).px(px(SPACE_MD)).disabled(disabled)
}

pub fn secondary_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).label(label).icon(icon).secondary().outline().h(px(TOPBAR_ACTION_HEIGHT)).px(px(SPACE_MD)).disabled(disabled)
}

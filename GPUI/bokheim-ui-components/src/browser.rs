use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, Pixels, SharedString, Stateful, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::radio::Radio;
use gpui_component::{Disableable, Icon, IconName, Selectable, Sizable};

use crate::{BrowserTheme, CONTENT_PADDING, NAV_RAIL_WIDTH, Scrollable, ScrollableElement, TOPBAR_ACTION_GAP, TOPBAR_ACTION_HEIGHT, TOPBAR_ACTION_HORIZONTAL_PADDING, TOPBAR_HEIGHT, content};

pub fn nav_rail_item(id: impl Into<ElementId>, icon: impl Into<Icon>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .selected(selected)
        .w(px(NAV_RAIL_WIDTH - 16.0))
        .h(px(64.0))
        .p_0()
        .rounded(px(10.0))
        .text_color(if selected { theme.accent } else { theme.text_muted })
        .when(selected, |button| button.bg(theme.accent).text_color(theme.surface_alt).border_1().border_color(theme.accent))
        .child(div().size_full().flex().flex_col().items_center().justify_center().gap(px(6.0)).child(Icon::new(icon).large()).child(div().text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child(label)))
}

pub fn home_navigation_icon() -> Icon {
    Icon::empty().path("icons/bokheim-home.svg")
}

pub fn browser_search_box() -> Div {
    div().w(px(480.0)).max_w_full().flex_none()
}

pub fn browser_body() -> Div {
    div().flex_1().min_h_0().flex().flex_row()
}

pub fn library_section_content() -> Div {
    div().flex_1().min_w_0().min_h_0().flex().flex_col()
}

pub fn library_switcher_group() -> Div {
    div().relative().w_full().h(px(72.0)).flex_none().flex().items_center().gap(px(9.0))
}

pub fn browser_topbar(theme: BrowserTheme) -> Div {
    div().h(px(TOPBAR_HEIGHT)).flex_none().flex().flex_row().items_center().gap(px(12.0)).px(px(CONTENT_PADDING)).bg(theme.surface).border_b_1().border_color(theme.border)
}

pub fn browser_topbar_left() -> Div {
    div().flex_1().min_w_0().flex().items_center().justify_start()
}

pub fn browser_topbar_center() -> Div {
    div().flex_1().min_w_0().flex().items_center().justify_center()
}

pub fn browser_topbar_right() -> Div {
    div().flex_none().min_w_0().flex().items_center().justify_end().gap(px(TOPBAR_ACTION_GAP))
}

pub fn browser_search_page() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(18.0)).p(px(20.0))
}

pub fn browser_actionable_empty(title: impl Into<SharedString>, description: impl Into<SharedString>, actions: impl IntoElement, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_h(px(240.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(9.0))
        .text_center()
        .child(div().text_size(gpui::rems(19.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family(font_family).child(title.into()))
        .child(div().text_size(gpui::rems(13.0 / 16.0)).text_color(theme.text_muted).child(description.into()))
        .child(div().mt(px(5.0)).child(actions))
}

pub fn library_switcher_trigger(label: impl Into<SharedString>) -> Button {
    let label = label.into();
    Button::new("library-switcher-trigger")
        .ghost()
        .w(px(NAV_RAIL_WIDTH - 16.0))
        .h(px(64.0))
        .px_0()
        .overflow_hidden()
        .text_size(gpui::rems(10.0 / 16.0))
        .font_weight(FontWeight::BOLD)
        .tooltip(label.clone())
        .child(div().w(px(NAV_RAIL_WIDTH - 16.0)).min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

pub fn library_switcher_overlay() -> Div {
    div().absolute().inset_0().size_full()
}

pub fn library_switcher_scrim(theme: BrowserTheme) -> Stateful<Div> {
    div().id("library-switcher-scrim").absolute().inset_0().size_full().bg(theme.overlay_scrim).cursor_default()
}

pub fn library_switcher_dialog(theme: BrowserTheme) -> Scrollable<Div> {
    div().w(px(304.0)).max_h(px(520.0)).occlude().overflow_y_scrollbar().p(px(12.0)).rounded(px(12.0)).border_1().border_color(theme.border).bg(theme.card_bg).shadow(vec![gpui_component::box_shadow(
        px(0.0),
        px(14.0),
        px(34.0),
        px(0.0),
        theme.shadow,
    )])
}

pub fn library_switcher_panel() -> Div {
    div().w(px(280.0)).flex().flex_col().gap(px(4.0))
}

pub fn library_switcher_title(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().px(px(10.0)).pb(px(6.0)).text_size(gpui::rems(13.0 / 16.0)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).child(text.into())
}

pub fn library_switcher_item(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .selected(selected)
        .when(selected, |button| button.icon(IconName::Check))
        .w_full()
        .h(px(40.0))
        .px(px(10.0))
        .overflow_hidden()
        .text_size(gpui::rems(14.0 / 16.0))
        .tooltip(label.clone())
        .child(div().w(px(220.0)).min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

pub fn library_switcher_actions_label(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().mt(px(6.0)).pt(px(10.0)).px(px(10.0)).border_t_1().border_color(theme.border).text_size(gpui::rems(11.0 / 16.0)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).child(text.into())
}

pub fn library_switcher_action_item(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    Button::new(id)
        .label(label)
        .icon(icon)
        .secondary()
        .outline()
        .justify_start()
        .w_full()
        .h(px(40.0))
        .px(px(10.0))
        .rounded(px(9.0))
        .border_color(theme.border)
        .text_color(theme.accent)
        .text_size(gpui::rems(13.0 / 16.0))
        .font_weight(FontWeight::BOLD)
}

pub fn topbar_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    Button::new(id)
        .label(label)
        .icon(icon)
        .secondary()
        .outline()
        .h(px(TOPBAR_ACTION_HEIGHT))
        .px(px(TOPBAR_ACTION_HORIZONTAL_PADDING))
        .rounded(px(10.0))
        .border_color(theme.border)
        .text_color(theme.accent)
        .text_size(gpui::rems(13.0 / 16.0))
        .font_weight(FontWeight::BOLD)
}

pub fn browser_settings_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().flex().flex_col().items_center().p(px(18.0)).overflow_y_scrollbar()
}

pub fn browser_settings_content() -> Div {
    div().w_full().max_w(px(580.0)).flex().flex_col().gap(px(16.0))
}

pub fn browser_settings_title(text: impl Into<SharedString>, font_family: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(26.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family(font_family).child(text.into())
}

pub fn browser_settings_group(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.card_bg)
        .overflow_hidden()
        .child(div().px(px(16.0)).py(px(12.0)).text_size(gpui::rems(13.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into()))
}

pub fn browser_settings_row(title: impl Into<SharedString>, description: impl Into<SharedString>, control: impl IntoElement, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_h(px(76.0))
        .px(px(16.0))
        .py(px(12.0))
        .flex()
        .items_center()
        .gap(px(20.0))
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(div().text_size(gpui::rems(15.0 / 16.0)).font_weight(FontWeight::MEDIUM).child(title.into()))
                .child(div().text_size(gpui::rems(12.0 / 16.0)).text_color(theme.text_muted).child(description.into())),
        )
        .child(control)
}

pub fn browser_settings_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    Button::new(id).label(label).dropdown_caret(true).secondary().outline().min_w(px(190.0)).h(px(40.0)).px(px(12.0)).rounded(px(9.0)).border_color(theme.border)
}

pub fn browser_settings_card(theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(18.0)).p(px(18.0)).rounded(px(16.0)).border_1().border_color(theme.border).bg(theme.card_bg).shadow(vec![gpui_component::box_shadow(px(0.0), px(14.0), px(34.0), px(0.0), theme.shadow)])
}

pub fn browser_settings_overlay() -> Div {
    div().absolute().inset_0().size_full().flex().items_center().justify_center().p(px(22.0))
}

pub fn browser_settings_scrim(theme: BrowserTheme) -> Stateful<Div> {
    div().id("browser-settings-scrim").absolute().inset_0().size_full().bg(theme.overlay_scrim).cursor_default()
}

pub fn browser_settings_dialog(max_height: Pixels) -> Scrollable<Div> {
    div().relative().w_full().max_w(px(580.0)).max_h(max_height).min_h_0().occlude().overflow_y_scrollbar()
}

pub fn browser_settings_header_row() -> Div {
    div().w_full().flex().items_start().justify_between().gap(px(12.0))
}

pub fn browser_settings_close_button(theme: BrowserTheme) -> Button {
    Button::new("close-browser-settings").icon(IconName::Close).ghost().size(px(34.0)).rounded(px(9.0)).text_color(theme.text_muted).tooltip("Close settings")
}

pub fn browser_settings_header(eyebrow: impl Into<SharedString>, title: impl Into<SharedString>, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(div().text_size(gpui::rems(10.0 / 16.0)).font_weight(FontWeight::BOLD).text_color(theme.accent).child(eyebrow.into()))
        .child(div().text_size(gpui::rems(22.0 / 16.0)).font_weight(FontWeight::MEDIUM).font_family(font_family).child(title.into()))
}

pub fn browser_settings_sections() -> Div {
    div().w_full().flex().flex_col().gap(px(16.0))
}

pub fn browser_settings_fieldset(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(9.0)).child(div().text_size(gpui::rems(12.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into()))
}

pub fn browser_settings_font_choices(theme: BrowserTheme) -> Div {
    div().flex().flex_row().gap(px(1.0)).overflow_hidden().rounded(px(9.0)).border_1().border_color(theme.border).bg(theme.border)
}

pub fn browser_settings_font_row(title: impl Into<SharedString>, choices: impl IntoElement, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_row().items_center().justify_between().gap(px(16.0)).child(div().text_size(gpui::rems(12.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into())).child(choices)
}

pub fn browser_settings_font_button(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    Button::new(id)
        .label(label)
        .selected(checked)
        .ghost()
        .min_w(px(116.0))
        .h(px(36.0))
        .px(px(10.0))
        .rounded(px(0.0))
        .border_0()
        .bg(if checked { theme.accent } else { theme.surface_alt })
        .text_color(if checked { theme.card_bg } else { theme.text })
        .font_family(font_family)
        .text_size(gpui::rems(12.0 / 16.0))
        .font_weight(FontWeight::SEMIBOLD)
}

pub fn browser_settings_font_size_button(id: impl Into<ElementId>, label: impl Into<SharedString>, disabled: bool, theme: BrowserTheme) -> Button {
    Button::new(id).label(label).disabled(disabled).ghost().w(px(36.0)).h(px(36.0)).rounded(px(0.0)).border_0().bg(theme.surface_alt).text_color(theme.text).text_size(gpui::rems(16.0 / 16.0)).font_weight(FontWeight::SEMIBOLD)
}

pub fn browser_settings_font_size_stepper(decrease: impl IntoElement, value: impl Into<SharedString>, increase: impl IntoElement, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .overflow_hidden()
        .rounded(px(9.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.border)
        .child(decrease)
        .child(div().w(px(58.0)).h(px(36.0)).flex().items_center().justify_center().bg(theme.surface_alt).text_color(theme.text).text_size(gpui::rems(12.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child(value.into()))
        .child(increase)
}

pub fn browser_settings_choice_card(id: impl Into<ElementId>, title: impl Into<SharedString>, description: impl Into<SharedString>, checked: bool, title_font: impl Into<SharedString>, theme: BrowserTheme) -> Radio {
    Radio::new(id)
        .checked(checked)
        .w_full()
        .min_h(px(64.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(11.0))
        .border_1()
        .border_color(if checked { theme.accent } else { theme.border })
        .bg(if checked { theme.accent_soft } else { theme.surface_alt })
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(div().text_size(gpui::rems(13.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).font_family(title_font).child(title.into()))
                .child(div().text_size(gpui::rems(10.0 / 16.0)).text_color(theme.text_muted).child(description.into())),
        )
}

pub fn browser_content() -> Div {
    content().gap(px(14.0))
}

pub fn browser_section_tabs() -> Div {
    div().flex().flex_row().gap_2().items_center()
}

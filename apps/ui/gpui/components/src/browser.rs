use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, SharedString, Stateful, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::radio::Radio;
use gpui_component::{Disableable, Icon, IconName, Selectable, Sizable};

use crate::{
    BrowserTheme, CONTENT_PADDING, RADIUS_MD, RADIUS_SM, SETTINGS_CHOICE_SEGMENT_WIDTH, SETTINGS_CONTROL_GAP, SETTINGS_CONTROL_HEIGHT, SETTINGS_STEPPER_VALUE_WIDTH, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, Scrollable,
    ScrollableElement, TEXT_MD, TEXT_SM, TEXT_XS, TITLE_MD, TOPBAR_ACTION_GAP, TOPBAR_ACTION_HEIGHT, TOPBAR_ACTION_HORIZONTAL_PADDING, TOPBAR_HEIGHT,
};

pub fn nav_rail_item(id: impl Into<ElementId>, icon: impl Into<Icon>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .w_full()
        .min_h(gpui::rems(crate::NAV_RAIL_ITEM_MIN_HEIGHT_REM))
        .py(px(SPACE_SM))
        .p_0()
        .rounded(px(RADIUS_MD))
        .text_color(if selected { theme.accent } else { theme.text })
        .when(selected, |button| button.bg(theme.accent_soft).text_color(theme.accent))
        .tooltip(label.clone())
        .child(
            div()
                .size_full()
                .min_w_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(crate::NAV_RAIL_ICON_LABEL_GAP))
                .child(Icon::new(icon).size(gpui::rems(crate::NAV_RAIL_ICON_SIZE / 16.0)))
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_center().text_size(gpui::rems(crate::NAV_RAIL_LABEL_SIZE / 16.0)).font_weight(crate::NAV_RAIL_LABEL_WEIGHT).child(label)),
        )
}

/// Separates the library-scoped destinations from the app-scoped ones. Inset
/// from both rail edges so it reads as a grouping rule rather than a panel
/// border.
pub fn nav_rail_divider(theme: BrowserTheme) -> Div {
    let _ = theme;
    div().w_full().h(px(SPACE_SM)).flex_none()
}

pub fn home_navigation_icon() -> Icon {
    navigation_icon(app_contract::NavigationIcon::House)
}

/// The GPUI rendering of the cross-frontend navigation icon contract.
pub fn navigation_icon(icon: app_contract::NavigationIcon) -> Icon {
    let path = match icon {
        app_contract::NavigationIcon::House => "icons/navigation/house.svg",
        app_contract::NavigationIcon::BookOpen => "icons/navigation/book-open.svg",
        app_contract::NavigationIcon::Folder => "icons/navigation/folder.svg",
        app_contract::NavigationIcon::Asterisk => "icons/navigation/asterisk.svg",
        app_contract::NavigationIcon::Tags => "icons/navigation/tags.svg",
        app_contract::NavigationIcon::User => "icons/navigation/user.svg",
        app_contract::NavigationIcon::Trash2 => "icons/navigation/trash-2.svg",
        app_contract::NavigationIcon::Building2 => "icons/navigation/building-2.svg",
        app_contract::NavigationIcon::CircleUser => "icons/navigation/circle-user.svg",
        app_contract::NavigationIcon::Settings => "icons/navigation/settings.svg",
    };
    Icon::empty().path(path)
}

pub fn browser_search_box() -> Div {
    div().w_full().min_w_0().h(px(TOPBAR_ACTION_HEIGHT)).flex_1()
}

/// A text input that searches or filters.
///
/// The leading icon carries the general meaning; individual pages may add a
/// contextual placeholder through their `InputState`. `cleanable` gives a
/// filter a way out that does not involve selecting and deleting its text.
pub fn browser_search_input(state: &gpui::Entity<gpui_component::input::InputState>) -> gpui_component::input::Input {
    gpui::Styled::h(gpui_component::input::Input::new(state).prefix(Icon::new(IconName::Search)).cleanable(true), px(TOPBAR_ACTION_HEIGHT))
}

pub fn browser_body() -> Div {
    div().flex_1().min_h_0().flex().flex_row()
}

pub fn library_section_content() -> Div {
    div().flex_1().min_w_0().min_h_0().flex().flex_col()
}

pub fn library_switcher_group(theme: BrowserTheme) -> Div {
    let _ = theme;
    div().relative().w_full().min_h(gpui::rems(crate::NAV_RAIL_SWITCHER_HEIGHT_REM)).flex_none().flex().items_center().gap(px(SPACE_SM))
}

pub fn browser_topbar(theme: BrowserTheme) -> Div {
    div().h(gpui::rems(TOPBAR_HEIGHT / 16.0)).flex_none().flex().flex_row().items_center().gap(px(SPACE_MD)).px(px(CONTENT_PADDING)).bg(theme.surface)
}

/// Two-line hierarchy header used when controls belong beneath the path.
pub fn browser_stacked_topbar(theme: BrowserTheme) -> Div {
    div().w_full().flex_none().flex().flex_col().gap(px(SPACE_XS)).px(px(CONTENT_PADDING)).py(px(SPACE_XS)).bg(theme.surface)
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
    crate::full_size_column().gap(px(SPACE_LG)).p(px(SPACE_LG))
}

/// Centred, quiet status text for a section that is still loading.
///
/// Deliberately plainer than [`browser_actionable_empty`]: a page that is
/// waiting has nothing for the reader to act on, and an empty-state heading
/// would assert something not yet known to be true.
pub fn browser_loading_message(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().min_h(px(240.0)).flex().items_center().justify_center().text_center().text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(text.into())
}

/// A one-line muted note standing in for content: "no matching titles",
/// "searching catalogues…". Quieter than [`browser_actionable_empty`], which
/// asserts a heading and offers an action.
pub fn browser_inline_note(text: impl Into<SharedString>, theme: BrowserTheme) -> gpui_component::label::Label {
    gpui_component::label::Label::new(text.into()).w_full().px(px(SPACE_SM)).py(px(SPACE_XS)).text_color(theme.text_muted)
}

pub fn browser_actionable_empty(title: impl Into<SharedString>, description: impl Into<SharedString>, actions: impl IntoElement, theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_h(px(240.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(SPACE_SM))
        .text_center()
        .child(div().text_size(gpui::rems(TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(div().text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(description.into()))
        .child(div().mt(px(SPACE_XS)).child(actions))
}

pub fn library_switcher_trigger(label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new("library-switcher-trigger")
        .ghost()
        .w_full()
        .h(gpui::rems(crate::NAV_RAIL_SWITCHER_HEIGHT_REM))
        .min_h(gpui::rems(crate::NAV_RAIL_SWITCHER_HEIGHT_REM))
        .p_0()
        .overflow_hidden()
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .font_weight(FontWeight::BOLD)
        .text_color(theme.accent)
        .tooltip(label.clone())
        .child(
            div()
                .size_full()
                .min_w_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(SPACE_XXS))
                .child(navigation_icon(app_contract::NavigationIcon::BookOpen).large())
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_center().child(label)),
        )
}

pub fn library_switcher_panel() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_XS))
}

pub fn library_switcher_overlay() -> Div {
    crate::modal_overlay().occlude()
}

pub fn library_switcher_scrim(theme: BrowserTheme) -> Stateful<Div> {
    crate::modal_scrim("library-switcher-scrim", theme)
}

pub fn library_switcher_dialog(theme: BrowserTheme) -> Scrollable<Div> {
    crate::modal_surface(theme).w(px(328.0)).max_w_full().h_auto().max_h_full().overflow_y_scrollbar().pt(px(SPACE_LG)).pr(px(SPACE_LG)).pb(px(SPACE_LG)).pl(px(SPACE_LG - SPACE_SM))
}

pub fn library_switcher_header(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().h(px(40.0)).w_full().flex().items_center().justify_between().pl(px(SPACE_SM)).pb(px(SPACE_XS)).text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::BOLD).text_color(theme.text).child(title.into())
}

pub fn library_switcher_add_button(theme: BrowserTheme) -> Button {
    crate::outlined_icon_button("library-switcher-add", "Add library", IconName::Plus, px(40.0), RADIUS_MD, theme).small().bg(theme.surface)
}

pub fn library_switcher_item(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .selected(selected)
        .flex_1()
        .min_w_0()
        .h(px(40.0))
        .px(px(SPACE_SM))
        .justify_start()
        .overflow_hidden()
        .text_size(gpui::rems(TEXT_SM / 16.0))
        .tooltip(label.clone())
        .child(div().w(px(220.0)).min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

pub fn library_switcher_row(library: impl IntoElement, delete: impl IntoElement) -> Div {
    div().w_full().flex().items_center().gap(px(SPACE_XS)).child(library).child(delete)
}

pub fn library_switcher_delete_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    let label = label.into();
    crate::outlined_icon_button(id, format!("Delete {label}"), navigation_icon(app_contract::NavigationIcon::Trash2), px(40.0), RADIUS_MD, theme).small().flex_none().bg(theme.surface)
}

pub fn topbar_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    crate::outlined_action_button(id, label, icon, theme).h(px(TOPBAR_ACTION_HEIGHT)).px(px(TOPBAR_ACTION_HORIZONTAL_PADDING)).rounded(px(RADIUS_MD))
}

pub fn topbar_icon_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    crate::outlined_icon_button(id, label, icon, px(TOPBAR_ACTION_HEIGHT), RADIUS_MD, theme).text_color(theme.accent)
}

pub fn browser_settings_page() -> Scrollable<Div> {
    crate::full_size_column().items_center().p(px(SPACE_LG)).overflow_y_scrollbar()
}

pub fn browser_settings_content() -> Div {
    div().w_full().max_w(px(580.0)).flex().flex_col().gap(px(SPACE_LG))
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
                .gap(px(SPACE_XS))
                .child(div().text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::MEDIUM).child(title.into()))
                .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(description.into())),
        )
        .child(control)
}

pub fn browser_settings_sections() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_LG))
}

pub fn browser_settings_fieldset(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM)).child(div().text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into()))
}

pub fn browser_settings_font_choices(theme: BrowserTheme) -> Div {
    browser_settings_button_group(theme)
}

pub fn browser_settings_button_group(theme: BrowserTheme) -> Div {
    div().flex().flex_row().gap(px(SETTINGS_CONTROL_GAP)).overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.border).bg(theme.border)
}

/// A wrapping group for open-ended settings such as catalogue languages.
/// Unlike the compact segmented control, this remains readable when it has
/// more choices than fit on one line.
pub fn browser_settings_chip_group() -> Div {
    div().w_full().flex().flex_row().flex_wrap().gap(px(SPACE_SM))
}

pub fn browser_settings_chip_button(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool, theme: BrowserTheme) -> Button {
    Button::new(id)
        .label(label)
        .selected(checked)
        .ghost()
        .h(px(32.0))
        .px(px(SPACE_SM))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if checked { theme.accent } else { theme.border })
        .bg(if checked { theme.accent_soft } else { theme.surface })
        .text_color(if checked { theme.accent } else { theme.text_muted })
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .font_weight(FontWeight::SEMIBOLD)
}

pub fn browser_settings_font_row(title: impl Into<SharedString>, choices: impl IntoElement, theme: BrowserTheme) -> Div {
    browser_settings_compact_row(title, choices, theme)
}

pub fn browser_settings_compact_row(title: impl Into<SharedString>, choices: impl IntoElement, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_row().items_center().justify_between().gap(px(SPACE_LG)).child(div().text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into())).child(choices)
}

pub fn browser_settings_font_button(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    browser_settings_choice_button(id, label, checked, theme).font_family(font_family)
}

pub fn browser_settings_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool, theme: BrowserTheme) -> Button {
    Button::new(id)
        .label(label)
        .selected(checked)
        .ghost()
        .w(px(SETTINGS_CHOICE_SEGMENT_WIDTH))
        .flex_none()
        .h(px(SETTINGS_CONTROL_HEIGHT))
        .px(px(SPACE_SM))
        .rounded(px(0.0))
        .border_0()
        .bg(if checked { theme.accent } else { theme.surface })
        .text_color(if checked { theme.raised_bg } else { theme.text })
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .font_weight(FontWeight::SEMIBOLD)
}

pub fn browser_settings_font_size_button(id: impl Into<ElementId>, label: impl Into<SharedString>, disabled: bool, theme: BrowserTheme) -> Button {
    Button::new(id)
        .label(label)
        .disabled(disabled)
        .ghost()
        .w(px(SETTINGS_CONTROL_HEIGHT))
        .h(px(SETTINGS_CONTROL_HEIGHT))
        .rounded(px(0.0))
        .border_0()
        .bg(theme.surface)
        .text_color(theme.text)
        .text_size(gpui::rems(16.0 / 16.0))
        .font_weight(FontWeight::SEMIBOLD)
}

pub fn browser_settings_font_size_stepper(decrease: impl IntoElement, value: impl Into<SharedString>, increase: impl IntoElement, theme: BrowserTheme) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(SETTINGS_CONTROL_GAP))
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .bg(theme.border)
        .child(decrease)
        .child(
            div()
                .w(px(SETTINGS_STEPPER_VALUE_WIDTH))
                .h(px(SETTINGS_CONTROL_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.surface)
                .text_color(theme.text)
                .text_size(gpui::rems(TEXT_XS / 16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value.into()),
        )
        .child(increase)
}

pub fn browser_settings_choice_card(id: impl Into<ElementId>, title: impl Into<SharedString>, description: impl Into<SharedString>, checked: bool, theme: BrowserTheme) -> Radio {
    Radio::new(id)
        .checked(checked)
        .w_full()
        .min_h(px(64.0))
        .px(px(SPACE_MD))
        .py(px(SPACE_SM))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(if checked { theme.accent } else { theme.border })
        .bg(if checked { theme.accent_soft } else { theme.surface_alt })
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(SPACE_XXS))
                .child(div().text_size(gpui::rems(TEXT_SM / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(description.into())),
        )
}

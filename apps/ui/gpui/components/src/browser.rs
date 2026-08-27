use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, SharedString, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Icon, IconName, Selectable, Sizable};

use crate::{
    BrowserTheme, RADIUS_MD, RADIUS_SM, SETTINGS_CHOICE_SEGMENT_WIDTH, SETTINGS_CONTROL_GAP, SETTINGS_CONTROL_HEIGHT, SETTINGS_STEPPER_VALUE_WIDTH, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, Scrollable, ScrollableElement, TEXT_MD, TEXT_SM,
    TEXT_XS, TITLE_MD, TOPBAR_ACTION_GAP, TOPBAR_ACTION_HEIGHT, TOPBAR_ACTION_HORIZONTAL_PADDING,
};

pub fn nav_rail_item(id: impl Into<ElementId>, icon: impl Into<Icon>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .w_full()
        .h(gpui::rems(crate::NAV_RAIL_ITEM_HEIGHT_REM))
        .flex_none()
        .py(px(SPACE_SM))
        .p_0()
        .rounded(px(RADIUS_MD))
        .text_color(if selected { theme.accent } else { theme.text })
        .when(selected, |button| button.bg(theme.accent_soft).text_color(theme.accent))
        .tooltip(label.clone())
        .child(div().size_full().min_w_0().flex().items_center().justify_center().child(Icon::new(icon).size(gpui::rems(crate::NAV_RAIL_ICON_SIZE_REM))))
}

/// Separates the library-scoped destinations from the app-scoped ones. Inset
/// from both rail edges so it reads as a grouping rule rather than a panel
/// border.
pub fn nav_rail_divider(theme: BrowserTheme) -> Div {
    let _ = theme;
    div().w_full().h(px(SPACE_SM)).flex_none()
}

pub fn bottom_navigation(theme: BrowserTheme) -> Div {
    div().w_full().h(gpui::rems(crate::BOTTOM_NAV_HEIGHT_REM)).flex_none().flex().flex_row().items_stretch().border_t_1().border_color(theme.border).bg(theme.page_bg)
}

pub fn bottom_navigation_item(id: impl Into<ElementId>, icon: impl Into<Icon>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .h_full()
        .flex_1()
        .min_w_0()
        .p_0()
        .rounded(px(0.0))
        .text_color(if selected { theme.accent } else { theme.text_muted })
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
                .gap(px(SPACE_XXS))
                .child(Icon::new(icon).size(gpui::rems(crate::NAV_RAIL_ICON_SIZE_REM)))
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_center().text_size(gpui::rems(TEXT_XS / 16.0)).child(label)),
        )
}

pub fn more_navigation_overlay() -> Div {
    crate::absolute_full_size().flex().items_end().justify_center()
}

/// A modal list sized from its fixed-height header and rows.
pub fn navigation_modal_list(item_count: usize, max_height: f32, theme: BrowserTheme) -> Scrollable<Div> {
    let height = (72.0 + item_count as f32 * 44.0).min(max_height);
    crate::modal_surface(theme).h(px(height)).max_h_full().overflow_y_scrollbar()
}

pub fn more_navigation_header(theme: BrowserTheme) -> Div {
    div().w_full().h(px(40.0)).flex_none().flex().items_center().px(px(SPACE_SM)).text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text).child("More")
}

pub fn more_navigation_item(id: impl Into<ElementId>, icon: impl Into<Icon>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .w_full()
        .h(px(40.0))
        .flex_none()
        .px(px(SPACE_SM))
        .justify_start()
        .rounded(px(RADIUS_MD))
        .text_color(if selected { theme.accent } else { theme.text })
        .when(selected, |button| button.bg(theme.accent_soft).text_color(theme.accent))
        .tooltip(label.clone())
        .child(div().size_full().min_w_0().flex().items_center().gap(px(SPACE_SM)).child(Icon::new(icon)).child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationIcon {
    House,
    BookOpen,
    Folder,
    Tags,
    User,
    Trash2,
    Building2,
    Settings,
}

pub fn navigation_icon(icon: NavigationIcon) -> Icon {
    let path = match icon {
        NavigationIcon::House => "icons/navigation/house.svg",
        NavigationIcon::BookOpen => "icons/navigation/book-open.svg",
        NavigationIcon::Folder => "icons/navigation/folder.svg",
        NavigationIcon::Tags => "icons/navigation/tags.svg",
        NavigationIcon::User => "icons/navigation/user.svg",
        NavigationIcon::Trash2 => "icons/navigation/trash-2.svg",
        NavigationIcon::Building2 => "icons/navigation/building-2.svg",
        NavigationIcon::Settings => "icons/navigation/settings.svg",
    };
    Icon::empty().path(path)
}

/// A centered topbar input that searches or filters.
///
/// Individual pages may add a contextual placeholder through their
/// `InputState`. `cleanable` gives a filter a way out that does not involve
/// selecting and deleting its text.
pub fn browser_search_control(state: &gpui::Entity<gpui_component::input::InputState>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .child(div().w_full().min_w_0().h(px(TOPBAR_ACTION_HEIGHT)).flex_1().child(gpui::Styled::h(gpui_component::input::Input::new(state).cleanable(true), px(TOPBAR_ACTION_HEIGHT))))
}

pub fn browser_body() -> Div {
    div().flex_1().min_h_0().flex().flex_row()
}

pub fn library_section_content() -> Div {
    div().flex_1().min_w_0().min_h_0().flex().flex_col()
}

pub fn library_switcher_group(theme: BrowserTheme) -> Div {
    let _ = theme;
    div().relative().w_full().h(gpui::rems(crate::NAV_RAIL_SIZE_REM)).flex_none().flex().items_center().gap(px(SPACE_SM))
}

pub fn browser_topbar(theme: BrowserTheme) -> Div {
    div().w_full().h(gpui::rems(crate::TOPBAR_HEIGHT_REM)).flex_none().flex().flex_row().items_center().gap(px(SPACE_MD)).bg(theme.surface)
}

pub fn browser_topbar_left() -> Div {
    div().flex_1().min_w_0().flex().items_center().justify_start()
}

pub fn browser_topbar_right() -> Div {
    div().flex_none().min_w_0().flex().items_center().justify_end().gap(px(TOPBAR_ACTION_GAP))
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
        .h(gpui::rems(crate::NAV_RAIL_SIZE_REM))
        .flex_none()
        .p_0()
        .overflow_hidden()
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .font_weight(FontWeight::NORMAL)
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
                .child(navigation_icon(NavigationIcon::BookOpen).large())
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_center().child(label)),
        )
}

pub fn library_switcher_panel() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_XS))
}

pub fn library_switcher_header(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().h(px(40.0)).w_full().flex().items_center().justify_between().pl(px(SPACE_SM)).pb(px(SPACE_XS)).text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text).child(title.into())
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
    crate::outlined_icon_button(id, format!("Delete {label}"), navigation_icon(NavigationIcon::Trash2), px(40.0), RADIUS_MD, theme).small().flex_none().bg(theme.surface)
}

pub fn topbar_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    crate::outlined_button(id, theme)
        .label(label)
        .icon(icon)
        .text_color(theme.accent)
        .text_size(gpui::rems(TEXT_SM / 16.0))
        .font_weight(crate::TOPBAR_ACTION_LABEL_WEIGHT)
        .h(px(TOPBAR_ACTION_HEIGHT))
        .px(px(TOPBAR_ACTION_HORIZONTAL_PADDING))
        .rounded(px(RADIUS_MD))
}

pub fn topbar_icon_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    crate::outlined_icon_button(id, label, icon, px(TOPBAR_ACTION_HEIGHT), RADIUS_MD, theme).text_color(theme.accent)
}

pub fn browser_settings_page() -> Scrollable<Div> {
    crate::full_size_column().items_center().overflow_y_scrollbar()
}

pub fn browser_settings_content() -> Div {
    div().w_full().max_w(px(580.0)).flex().flex_col().gap(px(SPACE_MD))
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
                .child(div().text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(description.into())),
        )
        .child(control)
}

pub fn browser_settings_sections() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_MD))
}

pub fn browser_settings_fieldset(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_SM)).child(div().text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into()))
}

pub fn browser_settings_button_group(theme: BrowserTheme) -> Div {
    div().flex().flex_row().gap(px(SETTINGS_CONTROL_GAP)).overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.border).bg(theme.border)
}

pub fn browser_settings_compact_row(title: impl Into<SharedString>, choices: impl IntoElement, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_row().items_center().justify_between().gap(px(SPACE_MD)).child(div().text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(title.into())).child(choices)
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

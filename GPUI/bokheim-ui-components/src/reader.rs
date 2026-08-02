use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, MouseButton, ScrollHandle, SharedString, Stateful, div, px};
use gpui_component::{Disableable, Selectable};
use gpui_component::button::Button;
use gpui_component::list::ListItem;
use gpui_component::popover::Popover;
use gpui_component::resizable::{ResizablePanelGroup, h_resizable, resizable_panel};
use gpui_component::scroll::Scrollable;
use gpui_component::scroll::ScrollableElement;
use gpui_component::switch::Switch;
use gpui_component::{Icon, IconName};

use crate::{ACCENT_SOFT, BORDER, CARD_BG, PAGE_BG, SURFACE, TEXT_MUTED, action_button};

pub fn reader_toolbar() -> Div {
    div().h(px(58.0)).flex_none().w_full().min_w_0().px(px(14.0)).flex().items_center().gap(px(10.0)).bg(gpui::rgb(SURFACE)).border_b_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_toolbar_group() -> Div {
    div().min_w_0().flex().items_center().gap(px(7.0))
}

pub fn reader_control_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    action_button(id, label).h(px(34.0)).rounded(px(8.0)).border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

pub fn reader_control_button_state(id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool) -> Button {
    reader_control_button(id, label).disabled(!enabled)
}

pub fn reader_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Button {
    reader_control_button(id, label)
        .selected(selected)
        .px(px(10.0))
}

pub fn reader_square_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    reader_control_button(id, label).w(px(34.0)).px_0().text_size(gpui::rems(18.0 / 16.0))
}

pub fn reader_match_count(text: impl Into<SharedString>) -> Div {
    div().min_w(px(42.0)).text_size(gpui::rems(13.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn reader_settings_trigger() -> gpui_component::button::Button {
    action_button("reader-settings-trigger", "Settings").h(px(34.0)).rounded(px(8.0)).border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

pub fn reader_title(text: impl Into<SharedString>) -> Div {
    div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().px(px(5.0)).text_size(gpui::rems(16.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn reader_document_surface() -> Div {
    div().size_full().min_h_0().min_w_0().bg(gpui::rgb(PAGE_BG)).overflow_hidden()
}

pub fn reader_document_message(text: impl Into<SharedString>) -> Div {
    reader_document_surface().flex().items_center().justify_center().text_color(gpui::rgb(TEXT_MUTED)).text_size(gpui::rems(14.0 / 16.0)).child(text.into())
}

pub fn reader_pdf_scroll_surface(scroll_handle: &ScrollHandle) -> Stateful<Div> {
    reader_document_surface()
        .id("pdf-reader-scroll")
        .overflow_x_scroll()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .flex()
        .items_start()
        .justify_center()
        .gap(px(16.0))
        .p(px(24.0))
}

pub fn reader_toc() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(2.0)).p(px(8.0)).bg(gpui::rgb(SURFACE)).border_r_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_toc_row(id: impl Into<ElementId>, depth: usize, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(38.0))
        .min_w_0()
        .flex()
        .flex_none()
        .items_center()
        .pl(px((10.0 + depth as f32 * 12.0).min(70.0)))
        .pr(px(10.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .rounded(px(7.0))
        .text_size(gpui::rems(15.0 / 16.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(selected, |row| row.bg(gpui::rgb(ACCENT_SOFT)))
        .hover(|style| style.bg(gpui::rgb(ACCENT_SOFT)))
}

pub fn reader_toc_disclosure(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div().id(id).w(px(22.0)).h_full().flex_none().flex().items_center().justify_center().text_size(gpui::rems(11.0 / 16.0)).cursor_pointer().child(label.into())
}

pub fn reader_toc_title(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis()
}

pub fn reader_settings_panel() -> Div {
    div().w(px(240.0)).flex().flex_col().gap_2()
}

pub fn reader_settings_dropdown() -> Scrollable<Div> {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .w(px(440.0))
        .max_h(px(620.0))
        .overflow_y_scrollbar()
        .px(px(14.0))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(9.0))
        .bg(gpui::rgb(SURFACE))
}

pub fn reader_setting_row(label: impl Into<SharedString>) -> Div {
    div().flex().items_center().gap(px(8.0)).child(div().w(px(124.0)).text_size(gpui::rems(14.0 / 16.0)).child(label.into()))
}

pub fn reader_setting_value(value: impl Into<SharedString>) -> Div {
    div().w(px(72.0)).text_center().text_size(gpui::rems(13.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(value.into())
}

pub fn reader_line_numbers_switch(checked: bool) -> Switch {
    Switch::new("reader-line-numbers").checked(checked).tooltip("Toggle line numbers (L)")
}

pub fn reader_toc_tree_item(id: impl Into<ElementId>, depth: usize, has_children: bool, expanded: bool, title: impl Into<SharedString>) -> ListItem {
    let disclosure = div().w(px(18.0)).flex_none().children(has_children.then(|| Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight })));
    ListItem::new(id)
        .w_full()
        .h(px(38.0))
        .pl(px((8.0 + depth as f32 * 14.0).min(76.0)))
        .pr(px(8.0))
        .rounded(px(7.0))
        .child(disclosure)
        .child(div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().child(title.into()))
}

pub fn reader_shortcuts() -> Div {
    div().mt(px(4.0)).flex().flex_col().gap(px(3.0))
}

pub fn reader_shortcut(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(12.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn reader_settings_actions() -> Div {
    div().flex().gap_2()
}

pub fn reader_search_box() -> Div {
    div().w(px(260.0)).h(px(36.0)).overflow_hidden().rounded(px(8.0)).border_1().border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

pub fn reader_search_actions() -> Div {
    div().flex().items_center().gap_1()
}

pub fn reader_search_bar() -> Div {
    div().h(px(52.0)).flex_none().w_full().px(px(12.0)).flex().items_center().gap(px(8.0)).bg(gpui::rgb(SURFACE)).border_t_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_document_body() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col()
}

pub fn empty_document() -> Div {
    div().size_full()
}

pub fn reader_content() -> Div {
    div().flex_1().min_h_0()
}

pub fn reader_settings_popover() -> Popover {
    Popover::new("reader-settings").anchor(gpui::Anchor::BottomRight).p_0().trigger(reader_settings_trigger().tooltip("Reader settings"))
}

pub fn reader_split(toc: impl IntoElement, document: impl IntoElement, toc_width: f32) -> ResizablePanelGroup {
    h_resizable("reader-layout").child(resizable_panel().size(px(toc_width)).size_range(px(260.0)..px(520.0)).child(toc)).child(resizable_panel().child(document))
}

use gpui::prelude::*;
use gpui::{Div, ElementId, FontWeight, ImageSource, Img, ObjectFit, SharedString, Stateful, StyledImage, UniformList, div, img, px};
use gpui_component::box_shadow;
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::button::Button;
use gpui_component::resizable::{ResizablePanelGroup, h_resizable, resizable_panel};
use gpui_component::scroll::{Scrollable, ScrollableElement};
use gpui_component::{Icon, IconName, Sizable};

use crate::{ACCENT_SOFT, AUTHOR_LIST_PANEL_WIDTH, BOOK_COVER_HEIGHT, BOOK_COVER_WIDTH, BOOK_GRID_GAP, BOOK_GRID_ROW_HEIGHT, BORDER, BrowserTheme, CARD_BG, PAGE_BG, SHADOW, SURFACE, SURFACE_ALT, TEXT_MUTED};

pub fn book_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w(px(BOOK_COVER_WIDTH)).flex().flex_col().gap(px(5.0)).cursor_pointer()
}

pub fn book_cover(theme: BrowserTheme) -> Div {
    div().w(px(BOOK_COVER_WIDTH)).h(px(BOOK_COVER_HEIGHT)).overflow_hidden().rounded(px(8.0)).border_1().border_color(theme.border).shadow(vec![box_shadow(px(2.0), px(8.0), px(14.0), px(0.0), theme.shadow)])
}

pub fn book_cover_image(source: impl Into<ImageSource>) -> Img {
    img(source).size_full().object_fit(ObjectFit::Cover)
}

pub fn missing_book_cover(theme: BrowserTheme) -> Div {
    book_cover(theme).flex().items_center().justify_center().bg(theme.surface_alt)
}

pub fn book_card_title() -> Div {
    div().w(px(BOOK_COVER_WIDTH)).h(px(24.0)).mt(px(9.0)).text_size(gpui::rems(17.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).line_clamp(1)
}

pub fn book_card_author(theme: BrowserTheme) -> Div {
    div().w(px(BOOK_COVER_WIDTH)).h(px(22.0)).text_size(gpui::rems(15.0 / 16.0)).text_color(theme.text_muted).line_clamp(1)
}

pub fn book_grid_row() -> Div {
    div().h(px(BOOK_GRID_ROW_HEIGHT)).flex().flex_row().gap(px(BOOK_GRID_GAP))
}

pub fn virtual_list(list: UniformList) -> UniformList {
    list.flex_1()
}

pub fn embedded_virtual_list(list: UniformList, height: f32) -> UniformList {
    list.h(px(height)).flex_none()
}

pub fn home_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(16.0)).p(px(16.0)).overflow_y_scrollbar()
}

pub fn carousel_section() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(16.0))
}

pub fn carousel_title(text: impl Into<SharedString>, font_family: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(20.0 / 16.0)).font_family(font_family).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn carousel_row(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w_full().h(px(390.0)).min_w_0().flex().flex_row().gap(px(16.0)).pb(px(6.0)).overflow_x_scroll()
}

pub fn book_progress(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w(px(200.0)).pl(px(4.0)).text_size(gpui::rems(12.0 / 16.0)).text_color(theme.text_muted).child(text.into())
}

pub fn author_detail() -> Div {
    div().size_full().flex().flex_col().gap_3()
}

pub fn authors_split_page() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_row()
}

pub fn author_list_panel(theme: BrowserTheme) -> Div {
    div().w(px(AUTHOR_LIST_PANEL_WIDTH)).h_full().min_h_0().flex_none().flex().flex_col().gap(px(12.0)).p(px(16.0)).border_r_1().border_color(theme.border)
}

pub fn author_filter_box() -> Div {
    div().w_full().flex_none()
}

pub fn author_list_row(id: impl Into<ElementId>, selected: bool, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(64.0))
        .px(px(9.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(if selected { theme.border } else { gpui::Hsla::transparent_black() })
        .when(selected, |row| row.bg(theme.accent_soft))
        .when(!selected, |row| row.hover(move |style| style.bg(theme.surface_alt)))
        .cursor_pointer()
}

pub fn author_avatar(initial: impl Into<SharedString>, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().size(px(42.0)).flex_none().flex().items_center().justify_center().rounded_full().bg(theme.accent).text_color(theme.surface_alt).text_size(gpui::rems(16.0 / 16.0)).font_family(font_family).child(initial.into())
}

pub fn author_list_identity() -> Div {
    div().flex_1().min_w_0().flex().flex_col().gap(px(2.0))
}

pub fn author_list_name(name: impl Into<SharedString>, font_family: impl Into<SharedString>) -> Div {
    div().w_full().line_clamp(1).text_size(gpui::rems(15.0 / 16.0)).font_family(font_family).font_weight(FontWeight::MEDIUM).child(name.into())
}

pub fn author_book_count(count: usize, theme: BrowserTheme) -> Div {
    let noun = if count == 1 { "book" } else { "books" };
    div().text_size(gpui::rems(11.0 / 16.0)).text_color(theme.text_muted).child(format!("{count} {noun}"))
}

pub fn author_list_chevron(theme: BrowserTheme) -> Icon {
    Icon::new(IconName::ChevronRight).small().text_color(theme.text_muted)
}

pub fn author_books_panel() -> Div {
    div().flex_1().min_h_0().min_w_0().flex().flex_col().gap(px(16.0)).p(px(18.0))
}

pub fn author_books_heading(author: impl Into<SharedString>, book_count: usize, font_family: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let noun = if book_count == 1 { "book" } else { "books" };
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(div().text_size(gpui::rems(21.0 / 16.0)).font_family(font_family).font_weight(FontWeight::SEMIBOLD).child(author.into()))
        .child(div().text_size(gpui::rems(12.0 / 16.0)).text_color(theme.text_muted).child(format!("{book_count} {noun}")))
}

pub fn author_selection_hint(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().size_full().flex().items_center().justify_center().text_color(theme.text_muted).child(text.into())
}

pub fn author_page() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().p(px(18.0)).gap(px(14.0))
}

pub fn author_detail_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().flex().flex_col().p(px(18.0)).gap(px(14.0)).overflow_y_scrollbar()
}

pub fn author_grid_row() -> Div {
    div().h(px(88.0)).flex().flex_row().gap(px(14.0))
}

pub fn author_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(280.0))
        .h(px(74.0))
        .px(px(9.0))
        .py(px(9.0))
        .flex()
        .flex_col()
        .items_start()
        .gap(px(5.0))
        .border_1()
        .border_color(gpui::rgb(BORDER))
        .rounded(px(16.0))
        .bg(gpui::rgb(SURFACE_ALT))
        .cursor_pointer()
        .hover(|style| style.bg(gpui::rgb(PAGE_BG)))
}

pub fn author_card_title(text: impl Into<SharedString>, font_family: impl Into<SharedString>) -> Div {
    div().w(px(260.0)).text_size(gpui::rems(16.0 / 16.0)).font_family(font_family).font_weight(FontWeight::BOLD).line_clamp(1).child(text.into())
}

pub fn author_group() -> Div {
    div().w_full().flex().flex_col().gap(px(14.0)).p(px(18.0)).border_1().border_color(gpui::rgb(BORDER)).rounded(px(16.0)).bg(gpui::rgb(CARD_BG)).shadow(vec![box_shadow(
        px(0.0),
        px(7.0),
        px(14.0),
        px(0.0),
        gpui::Hsla::from(gpui::rgba(SHADOW)),
    )])
}

pub fn author_group_title(text: impl Into<SharedString>, font_family: impl Into<SharedString>) -> Div {
    div().pb(px(8.0)).text_size(gpui::rems(16.0 / 16.0)).font_family(font_family).font_weight(FontWeight::BOLD).child(text.into())
}

pub fn author_row(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).h(px(42.0)).px_3().flex().items_center().justify_between().border_b_1().border_color(gpui::rgb(BORDER)).cursor_pointer().hover(|style| style.bg(gpui::rgb(ACCENT_SOFT)))
}

pub fn folder_content() -> Div {
    div().size_full().flex().flex_col().gap_3()
}

pub fn folder_dashboard() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(20.0)).p(px(20.0))
}

pub fn folder_workspace() -> Div {
    div().size_full().min_h_0().min_w_0().p(px(18.0))
}

pub fn folder_tree_panel() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().p(px(9.0)).bg(gpui::rgb(SURFACE)).border_r_1().border_color(gpui::rgb(BORDER))
}

pub fn folder_tree_row(id: impl Into<ElementId>, depth: usize, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(40.0))
        .pl(px((depth as f32 * 20.0).min(140.0) + 10.0))
        .pr(px(10.0))
        .flex()
        .items_center()
        .text_size(gpui::rems(17.0 / 16.0))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .when(selected, |row| row.bg(gpui::rgb(ACCENT_SOFT)))
        .when(!selected, |row| row.hover(|style| style.bg(gpui::rgb(PAGE_BG))))
}

pub fn folder_books_panel() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().p(px(18.0)).gap(px(14.0))
}

pub fn breadcrumb_bar() -> Div {
    div().w_full().flex().items_center().py(px(5.0)).gap(px(2.0))
}

pub fn folder_breadcrumb() -> Breadcrumb {
    Breadcrumb::new().text_size(gpui::rems(14.0 / 16.0)).py(px(4.0))
}

pub fn folder_breadcrumb_item(label: impl Into<SharedString>, current: bool) -> BreadcrumbItem {
    BreadcrumbItem::new(label).px(px(5.0)).py(px(4.0)).when(current, |item| item.font_weight(FontWeight::BOLD))
}

pub fn breadcrumb_root(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).px(px(9.0)).py(px(5.0)).text_size(gpui::rems(20.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).cursor_pointer().hover(|style| style.bg(gpui::rgb(ACCENT_SOFT)))
}

pub fn breadcrumb_separator() -> Div {
    div().px(px(5.0)).text_size(gpui::rems(20.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child("/")
}

pub fn breadcrumb_part(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).px(px(9.0)).py(px(5.0)).text_size(gpui::rems(20.0 / 16.0)).cursor_pointer().hover(|style| style.bg(gpui::rgb(ACCENT_SOFT)))
}

pub fn breadcrumb_current(id: impl Into<ElementId>) -> Stateful<Div> {
    breadcrumb_part(id).font_weight(FontWeight::BOLD)
}

pub fn folder_split(tree: impl IntoElement, books: impl IntoElement, tree_width: f32) -> ResizablePanelGroup {
    h_resizable("library-folder-layout").child(resizable_panel().size(px(tree_width)).size_range(px(120.0)..px(560.0)).child(tree)).child(resizable_panel().child(books))
}

pub fn folder_buttons() -> Div {
    div().flex().flex_row().flex_wrap().gap(px(7.0))
}

pub fn folder_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    let label = label.into();
    crate::action_button(id, label.clone())
        .icon(IconName::Folder)
        .tooltip(format!("Open {label}"))
        .h(px(41.0))
        .px(px(11.0))
        .rounded(px(10.0))
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .text_size(gpui::rems(13.0 / 16.0))
        .font_weight(FontWeight::BOLD)
}

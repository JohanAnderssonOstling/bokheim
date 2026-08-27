use gpui::prelude::*;
use gpui::{AnyElement, Div, ElementId, FontWeight, ImageSource, ObjectFit, SharedString, Stateful, StyledImage, div, img, px};
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Icon, IconName, Sizable};

use crate::{
    BOOK_CARD_AUTHOR_HEIGHT_REM, BOOK_CARD_AUTHOR_LINE_HEIGHT_EM, BOOK_CARD_AUTHOR_SIZE, BOOK_CARD_COVER_CONTENT_GAP, BOOK_CARD_TITLE_LINE_HEIGHT_EM, BOOK_CARD_TITLE_LINES, BOOK_CARD_TITLE_SIZE, BOOK_CARD_TITLE_WEIGHT,
    BOOK_COVER_ASPECT_RATIO, BrowserTheme, CAROUSEL_CONTENT_GAP, CAROUSEL_CONTROL_GAP, CAROUSEL_SECTION_GAP, CAROUSEL_TITLE_SIZE, CAROUSEL_TITLE_WEIGHT, RADIUS_SM, SPACE_XS, Scrollable, ScrollableElement, TEXT_SM, TEXT_XS,
};

/// Cover, then the text block. The two levels each have one gap: `SPACE_MD`
/// separates the art from the words, `SPACE_XS` separates the words from each
/// other. Previously the wider first gap was a margin on the title fighting the
/// column's own gap, which meant two spacing mechanisms in one component.
pub fn book_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w_full().min_w_0().flex_none().flex().flex_col().gap(px(BOOK_CARD_COVER_CONTENT_GAP))
}

pub fn book_card_text() -> Div {
    div().w_full().min_w_0().flex_none().flex().flex_col().gap(px(crate::BOOK_CARD_TEXT_GAP))
}

pub fn book_cover(_theme: BrowserTheme) -> Div {
    div().w_full().min_w_0().aspect_ratio(BOOK_COVER_ASPECT_RATIO).flex().flex_col().items_center().justify_end()
}

pub fn book_cover_frame() -> Div {
    div().relative().w_full().min_w_0().aspect_ratio(BOOK_COVER_ASPECT_RATIO).cursor_pointer()
}

/// Cover art cropped to the standard 1:1.5 cover frame.
pub fn book_cover_image(source: impl Into<ImageSource>, theme: BrowserTheme) -> Div {
    div()
        .size_full()
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .shadow(vec![gpui_component::box_shadow(px(0.0), px(4.0), px(12.0), px(0.0), theme.shadow)])
        .child(img(source).size_full().object_fit(ObjectFit::Cover))
}

/// Stand-in for a book with no cover art.
///
/// The label was previously an unstyled raw string at inherited body size,
/// which read as loud as a real title. Muted and small: it is an absence, not
/// a thing to look at.
pub fn missing_book_cover(label: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    book_cover(theme)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .gap(px(SPACE_XS))
        .child(Icon::new(IconName::BookOpen).large().text_color(theme.text_muted))
        .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(label.into()))
}

pub fn book_card_title() -> Div {
    div().w_full().min_w_0().text_size(gpui::rems(BOOK_CARD_TITLE_SIZE / 16.0)).line_height(gpui::relative(BOOK_CARD_TITLE_LINE_HEIGHT_EM)).font_weight(BOOK_CARD_TITLE_WEIGHT).text_ellipsis().line_clamp(BOOK_CARD_TITLE_LINES)
}

pub fn book_card_author(theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_w_0()
        .h(gpui::rems(BOOK_CARD_AUTHOR_HEIGHT_REM))
        .flex_none()
        .text_size(gpui::rems(BOOK_CARD_AUTHOR_SIZE / 16.0))
        .line_height(gpui::relative(BOOK_CARD_AUTHOR_LINE_HEIGHT_EM))
        .text_color(theme.text_muted)
        .text_ellipsis()
        .line_clamp(1)
}

pub fn home_page() -> Scrollable<Div> {
    crate::full_size_column().gap(px(CAROUSEL_SECTION_GAP)).p(px(crate::SPACE_MD)).overflow_y_scrollbar()
}

/// A heading and the row it names. The gap inside a section is smaller than
/// the one between sections, so a title belongs to the carousel under it
/// rather than floating equidistant between two rows.
pub fn carousel_section() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(CAROUSEL_CONTENT_GAP))
}

/// The title line, with room for scroll controls at its far end.
pub fn carousel_heading(title: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(CAROUSEL_CONTENT_GAP))
        .child(div().flex_1().min_w_0().text_size(gpui::rems(CAROUSEL_TITLE_SIZE / 16.0)).font_weight(CAROUSEL_TITLE_WEIGHT).child(title.into()))
}

pub fn paginator_controls(previous: Option<AnyElement>, next: Option<AnyElement>) -> Div {
    div().flex_none().flex().flex_row().items_center().gap(px(CAROUSEL_CONTROL_GAP)).children(previous).children(next)
}

/// One step of a carousel. Square, quiet, and consistent across GPUI targets;
/// a horizontal row of cards is otherwise reachable only by a pointer that can
/// scroll sideways.
pub fn carousel_arrow_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    crate::outlined_icon_button(id, label, icon, gpui::rems(crate::CAROUSEL_ARROW_SIZE / 16.0), RADIUS_SM, theme).text_color(theme.accent)
}

pub fn folder_breadcrumb() -> Breadcrumb {
    Breadcrumb::new().text_size(gpui::rems(TEXT_SM / 16.0)).py(px(SPACE_XS))
}

pub fn folder_breadcrumb_item(label: impl Into<SharedString>, current: bool) -> BreadcrumbItem {
    let label = label.into();
    let max_characters = 24;
    let mut characters = label.chars();
    let visible = characters.by_ref().take(max_characters).collect::<String>();
    let visible = if characters.next().is_some() { format!("{}…", visible.chars().take(max_characters.saturating_sub(1)).collect::<String>()) } else { visible };
    BreadcrumbItem::new(visible).px(px(SPACE_XS)).py(px(SPACE_XS)).when(current, |item| item.font_weight(FontWeight::SEMIBOLD))
}

/// A compact hierarchy item for mixed paginator pages.
///
/// This function owns presentation only. Feature code remains responsible for
/// attaching click handlers and navigation behavior.
pub fn compact_hierarchy_item(id: impl Into<ElementId>, label: impl Into<SharedString>, path: Option<SharedString>, count: usize, theme: BrowserTheme) -> Button {
    let label = label.into();
    let text = div()
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(div().size_full().min_w_0().flex().items_center().child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(label)))
        .children(path.map(|path| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(gpui::relative(0.5))
                .min_w_0()
                .flex()
                .items_center()
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis_start().text_size(gpui::rems(0.5)).text_color(theme.text_muted).child(path))
        }));
    Button::new(id).ghost().w_full().max_w_full().h_full().p_0().px(px(crate::COMPACT_HIERARCHY_ITEM_HORIZONTAL_PADDING)).rounded(px(0.0)).border_0().bg(theme.raised_bg).child(
        div()
            .size_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(crate::COMPACT_HIERARCHY_ITEM_CONTENT_GAP))
            .child(Icon::new(IconName::Folder).text_color(theme.accent))
            .child(text)
            .child(div().flex_none().text_color(theme.text_muted).child(count.to_string())),
    )
}

use gpui::prelude::*;
use gpui::{AnyElement, Div, ElementId, FontWeight, ImageSource, Img, ObjectFit, PathBuilder, SharedString, Stateful, StyledImage, canvas, div, img, point, px};
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Icon, IconName, Selectable, Sizable};

use crate::{
    AUTHOR_PAGE_MAX_WIDTH, AUTHOR_ROW_COVER_COUNT, AUTHOR_ROW_COVER_HEIGHT, AUTHOR_ROW_COVER_WIDTH, AUTHOR_ROW_HEIGHT, BOOK_CARD_AUTHOR_HEIGHT_REM, BOOK_CARD_TITLE_HEIGHT_REM, BOOK_COVER_HEIGHT,
    BOOK_COVER_WIDTH, BrowserTheme, DETAILED_ROW_DESCRIPTION_LINES, DETAILED_ROW_HEIGHT, DETAILED_ROW_TEXT_MEASURE, DETAILED_ROW_THUMBNAIL_HEIGHT, DETAILED_ROW_THUMBNAIL_WIDTH, DETAILED_ROW_VERTICAL_PADDING, RADIUS_MD, RADIUS_SM, SPACE_LG,
    SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, SPACE_XXS, Scrollable, ScrollableElement, TEXT_LG, TEXT_MD, TEXT_SM, TEXT_XS, TITLE_MD,
};

/// Cover, then the text block. The two levels each have one gap: `SPACE_MD`
/// separates the art from the words, `SPACE_XS` separates the words from each
/// other. Previously the wider first gap was a margin on the title fighting the
/// column's own gap, which meant two spacing mechanisms in one component.
pub fn book_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w(px(BOOK_COVER_WIDTH)).flex_none().flex().flex_col().gap(px(SPACE_MD))
}

pub fn book_card_text() -> Div {
    div().w(px(BOOK_COVER_WIDTH)).flex_none().flex().flex_col().gap(px(SPACE_XS))
}

pub fn book_cover(_theme: BrowserTheme) -> Div {
    div().w(px(BOOK_COVER_WIDTH)).h(px(BOOK_COVER_HEIGHT)).flex().flex_col().items_center().justify_end()
}

pub fn book_cover_frame() -> Div {
    div().relative().w(px(BOOK_COVER_WIDTH)).h(px(BOOK_COVER_HEIGHT)).cursor_pointer()
}

/// Cover art, whole.
///
/// `Contain` rather than `Cover`: real covers run roughly 1:1.3 to 1:1.7
/// against a 1:1.6 frame, and cropping to fill takes the difference off the top
/// and bottom — which is where the title usually is. Off-ratio covers letterbox
/// against the mat instead, which reads like a shelf rather than a damaged
/// image.
pub fn book_cover_image(source: impl Into<ImageSource>, theme: BrowserTheme) -> Img {
    // Give the image element its intrinsic aspect-ratio height. If the element
    // itself fills the whole cover frame, `ObjectFit::Contain` centers a short
    // image inside that frame before the parent's bottom alignment can act.
    img(source)
        .w(px(BOOK_COVER_WIDTH))
        .h_auto()
        .max_h_full()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .shadow(vec![gpui_component::box_shadow(px(0.0), px(4.0), px(12.0), px(0.0), theme.shadow)])
        .object_fit(ObjectFit::Contain)
}

pub fn book_row_cover_image(source: impl Into<ImageSource>) -> Img {
    img(source).w(px(DETAILED_ROW_THUMBNAIL_WIDTH)).h_auto().max_h_full().object_fit(ObjectFit::Contain)
}

/// The icon-and-label pair a missing cover shows, without a frame of its own,
/// for callers such as the detailed row that supply their own smaller frame.
pub fn missing_cover_mark(label: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(SPACE_XS))
        .child(Icon::new(IconName::BookOpen).text_color(theme.text_muted))
        .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(label.into()))
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
        .bg(theme.surface_alt)
        .gap(px(SPACE_XS))
        .child(Icon::new(IconName::BookOpen).large().text_color(theme.text_muted))
        .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(label.into()))
}

pub fn book_card_title() -> Div {
    div().w(px(BOOK_COVER_WIDTH)).h(gpui::rems(BOOK_CARD_TITLE_HEIGHT_REM)).flex_none().text_size(gpui::rems(TEXT_LG / 16.0)).line_height(gpui::relative(21.0 / TEXT_LG)).font_weight(FontWeight::SEMIBOLD).line_clamp(2)
}

pub fn book_card_author(theme: BrowserTheme) -> Div {
    div().w(px(BOOK_COVER_WIDTH)).h(gpui::rems(BOOK_CARD_AUTHOR_HEIGHT_REM)).flex_none().text_size(gpui::rems(TEXT_MD / 16.0)).line_height(gpui::relative(22.0 / TEXT_MD)).text_color(theme.text_muted).line_clamp(1)
}

pub fn home_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(SPACE_XL)).p(px(crate::CONTENT_PADDING)).overflow_y_scrollbar()
}

/// A heading and the row it names. The gap inside a section is smaller than
/// the one between sections, so a title belongs to the carousel under it
/// rather than floating equidistant between two rows.
pub fn carousel_section() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(SPACE_MD))
}

/// The title line: the section's name, and its scroll controls pushed to the
/// far end of the same row.
pub fn carousel_heading() -> Div {
    div().w_full().min_w_0().flex().flex_row().items_center().justify_between().gap(px(SPACE_MD))
}

pub fn carousel_title(text: impl Into<SharedString>) -> Div {
    div().flex_1().min_w_0().text_size(gpui::rems(TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn carousel_controls() -> Div {
    div().flex_none().flex().flex_row().gap(px(SPACE_XS))
}

/// One step of a carousel. Square, quiet, and the same size on both frontends;
/// a horizontal row of cards is otherwise reachable only by a pointer that can
/// scroll sideways.
pub fn carousel_arrow_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    Button::new(id).icon(icon).secondary().outline().border_color(theme.border).text_color(theme.accent).w(px(crate::CAROUSEL_ARROW_SIZE)).h(px(crate::CAROUSEL_ARROW_SIZE)).rounded(px(RADIUS_SM)).tooltip(label.into())
}

pub fn authors_split_page() -> Div {
    div().w_full().max_w(px(AUTHOR_PAGE_MAX_WIDTH)).h_full().min_h_0().min_w_0().mx_auto().flex().flex_row()
}

pub fn author_list_panel(width: f32, theme: BrowserTheme) -> Div {
    div().w(px(width)).h_full().min_h_0().flex_none().flex().flex_col().gap(px(SPACE_MD)).p(px(SPACE_LG)).border_r_1().border_color(theme.border)
}

pub fn author_filter_box() -> Div {
    div().w_full().flex_none()
}


/// The index's two orders, side by side. A pair rather than a menu: there are
/// two of them, and which one is active has to be visible without opening
/// anything.
pub fn author_sort_control() -> Div {
    div().w_full().flex_none().flex().flex_row().gap(px(SPACE_XS))
}

pub fn author_sort_option(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(SPACE_SM))
        .py(px(SPACE_XXS))
        .flex_none()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if selected { theme.accent } else { theme.border })
        .when(selected, |option| option.bg(theme.accent).text_color(theme.page_bg).font_weight(FontWeight::SEMIBOLD))
        .when(!selected, |option| option.text_color(theme.text_muted).hover(move |style| style.bg(theme.hover)))
        .text_size(gpui::rems(TEXT_SM / 16.0))
        .cursor_pointer()
        .child(label.into())
}

/// One author: the covers of their books, then their filing name. The row is
/// as tall as a pair of covers, which is what replaced the initial-in-a-disc
/// and the book count that stood beside it.
pub fn author_list_row(id: impl Into<ElementId>, selected: bool, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(AUTHOR_ROW_HEIGHT))
        .px(px(SPACE_SM))
        .flex()
        .items_center()
        .gap(px(SPACE_MD))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if selected { theme.border } else { gpui::Hsla::transparent_black() })
        .when(selected, |row| row.bg(theme.accent_soft))
        .when(!selected, |row| row.hover(move |style| style.bg(theme.surface_alt)))
        .cursor_pointer()
}

/// The covers, side by side. Fixed width whatever it holds, so the names below
/// each other start at the same place even where an author has one book.
pub fn author_row_covers() -> Div {
    let width = AUTHOR_ROW_COVER_WIDTH * AUTHOR_ROW_COVER_COUNT as f32 + SPACE_XS * (AUTHOR_ROW_COVER_COUNT.saturating_sub(1)) as f32;
    div().w(px(width)).h(px(AUTHOR_ROW_COVER_HEIGHT)).flex_none().flex().flex_row().gap(px(SPACE_XS))
}

pub fn author_row_cover_frame() -> Div {
    div().w(px(AUTHOR_ROW_COVER_WIDTH)).h(px(AUTHOR_ROW_COVER_HEIGHT)).flex_none().flex().flex_col().justify_end().overflow_hidden()
}

pub fn author_row_cover_image(source: impl Into<ImageSource>, theme: BrowserTheme) -> Img {
    img(source).w(px(AUTHOR_ROW_COVER_WIDTH)).h_auto().max_h_full().border_1().border_color(theme.border).object_fit(ObjectFit::Contain)
}

/// The slot an author with one book — or one thumbnail — leaves empty. Drawn
/// rather than omitted, so the name keeps its place down the list.
pub fn author_row_missing_cover(theme: BrowserTheme) -> Div {
    author_row_cover_frame().border_1().border_dashed().border_color(theme.border).bg(theme.surface_alt)
}

pub fn author_list_identity() -> Div {
    div().flex_1().min_w_0().flex().flex_col().gap(px(SPACE_XXS))
}

pub fn author_list_name(name: impl Into<SharedString>) -> Div {
    div().w_full().line_clamp(1).text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::MEDIUM).child(name.into())
}


pub fn author_books_panel() -> Div {
    div().flex_1().min_h_0().min_w_0().flex().flex_col().gap(px(SPACE_LG)).p(px(SPACE_LG))
}

/// The selected author, over their books. The name alone: the count that used
/// to sit under it restated what the grid already shows, and disagreed with the
/// index whenever the two queries counted differently.
pub fn author_books_heading(author: impl Into<SharedString>, _theme: BrowserTheme) -> Div {
    div().flex_none().flex().flex_col().child(div().text_size(gpui::rems(TITLE_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(author.into()))
}

/// The catalogue results under an author's books, and the control that asks
/// for them.
pub fn author_discovery_section() -> Div {
    div().flex().flex_col().gap(px(SPACE_MD))
}

/// A closed disclosure is a promise, not a result: the catalogue search behind
/// it only leaves the machine once this is opened.
pub fn author_discovery_disclosure(id: impl Into<ElementId>, title: impl Into<SharedString>, hint: impl Into<SharedString>, expanded: bool, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(SPACE_SM))
        .py(px(SPACE_XS))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .child(Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }).small().text_color(theme.accent))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE_XXS))
                .child(div().text_size(gpui::rems(TEXT_LG / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(hint.into())),
        )
}


pub fn folder_dashboard() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(SPACE_LG)).p(px(crate::CONTENT_PADDING))
}

/// Stable full-page host for both hierarchy variants. Folders may append its
/// picker as an overlay child; Subjects uses the same sizing contract without
/// an overlay.
pub fn hierarchy_page_host(content: impl IntoElement) -> Div {
    div().relative().size_full().min_h_0().min_w_0().flex().flex_col().child(content)
}

pub fn folder_breadcrumb() -> Breadcrumb {
    Breadcrumb::new().text_size(gpui::rems(TEXT_SM / 16.0)).py(px(SPACE_XS))
}

pub fn folder_breadcrumb_item(label: impl Into<SharedString>, current: bool) -> BreadcrumbItem {
    let label = label.into();
    let max_characters = usize::from(app_contract::frontend::BREADCRUMB_ITEM_MAX_CHARACTERS);
    let mut characters = label.chars();
    let visible = characters.by_ref().take(max_characters).collect::<String>();
    let visible = if characters.next().is_some() { format!("{}…", visible.chars().take(max_characters.saturating_sub(1)).collect::<String>()) } else { visible };
    BreadcrumbItem::new(visible).px(px(SPACE_XS)).py(px(SPACE_XS)).when(current, |item| item.font_weight(FontWeight::BOLD))
}

/// A wrapping row of label-width buttons. The hierarchy pages moved off this
/// onto [`hierarchy_grid`]; the store's catalogue links still want it, because
/// a handful of links of wildly different lengths is exactly the case a mosaic
/// suits.
pub fn folder_buttons() -> Div {
    div().flex().flex_row().flex_wrap().gap(px(SPACE_XS))
}

/// Fixed columns rather than a row of label-width buttons.
///
/// Cells sized by their label made a mosaic: the counts landed wherever the
/// name ended, so they could not be read down the page, and a wide cell implied
/// an importance nothing intended. Uniform columns cost roughly twice the rows
/// and buy an aligned column of figures and a lattice the pointer can learn.
pub fn hierarchy_grid(_theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col()
}

/// One explicit hierarchy row. Column placement is decided by the shared
/// hysteresis state, so this row must never independently wrap its children.
pub fn hierarchy_grid_row() -> Div {
    div().w_full().flex_none().flex().flex_row()
}

/// One child of the folder or subject being browsed: name, and the number of
/// books beneath it right-set in its own column.
///
/// The count is a separate element rather than part of the label, so it aligns
/// and cannot be eaten by the name's ellipsis — which is exactly what
/// `format!("{name} ({count})")` used to do.
/// Every cell receives the same responsive width from
/// [`crate::hierarchy_grid_layout`]. Keeping that width fixed here prevents an
/// incomplete final row from stretching wider than the rows above it.
pub fn hierarchy_cell(id: impl Into<ElementId>, label: impl Into<SharedString>, count: usize, end_action: Option<AnyElement>, width: f32, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .ghost()
        .w(px(width))
        .flex_none()
        .h(px(crate::HIERARCHY_CELL_HEIGHT))
        .m(px(crate::HIERARCHY_CELL_MARGIN))
        .px(px(SPACE_SM))
        .py(px(SPACE_XS))
        .rounded(px(0.0))
        .border_0()
        .bg(theme.raised_bg)
        .tooltip(format!("Open {label}"))
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(SPACE_SM))
                .child(gpui_component::Icon::new(IconName::Folder).small().text_color(theme.accent))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(gpui::rems(TEXT_LG / 16.0)).font_weight(FontWeight::MEDIUM).text_color(theme.text).child(label))
                .child(div().flex_none().text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(count.to_string()))
                .children(end_action),
        )
}

/// Compact trailing download status for a folder cell. An empty ring is 0%,
/// its accent arc grows with the downloaded share, and completion collapses
/// to a checkmark. The fixed slot keeps folder counts aligned.
pub fn hierarchy_download_action(id: impl Into<ElementId>, downloaded: usize, total: usize, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    let progress = if total == 0 { 0.0 } else { downloaded.min(total) as f32 / total as f32 };
    let tooltip = if total == 0 { label.into() } else { format!("{downloaded} of {total} downloaded").into() };
    let button = Button::new(id).ghost().compact().w(px(22.0)).h(px(22.0)).p_0().text_color(theme.text_muted).tooltip(tooltip);
    if progress >= 1.0 {
        return button.icon(IconName::Check).text_color(theme.accent);
    }
    if total == 0 {
        return button;
    }
    button.child(
        canvas(
            move |bounds, _, _| {
                let center = point(bounds.origin.x + px(8.0), bounds.origin.y + px(8.0));
                let radius = px(6.5);
                let mut ring = PathBuilder::stroke(px(1.5));
                ring.move_to(point(center.x, center.y - radius));
                ring.arc_to(point(radius, radius), px(0.0), false, true, point(center.x, center.y + radius));
                ring.arc_to(point(radius, radius), px(0.0), false, true, point(center.x, center.y - radius));

                let arc = (progress > 0.0).then(|| {
                    let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * progress;
                    let end = point(center.x + px(6.5 * angle.cos()), center.y + px(6.5 * angle.sin()));
                    let mut arc = PathBuilder::stroke(px(2.0));
                    arc.move_to(point(center.x, center.y - radius));
                    arc.arc_to(point(radius, radius), px(0.0), progress > 0.5, true, end);
                    arc.build().ok()
                });
                (ring.build().ok(), arc.flatten())
            },
            move |_, (ring, arc), window, _| {
                if let Some(ring) = ring {
                    window.paint_path(ring, theme.border);
                }
                if let Some(arc) = arc {
                    window.paint_path(arc, theme.accent);
                }
            },
        )
        .w(px(16.0))
        .h(px(16.0)),
    )
}

pub fn folder_navigation_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    let label = label.into();
    folder_navigation_button_base(id, label.clone(), icon, theme).flex_1().min_w_0().child(div().w(px(190.0)).min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

fn folder_navigation_button_base(id: impl Into<ElementId>, label: SharedString, icon: IconName, theme: BrowserTheme) -> Button {
    Button::new(id)
        .secondary()
        .outline()
        .icon(icon)
        .tooltip(format!("Open {label}"))
        .overflow_hidden()
        .h(px(41.0))
        .px(px(SPACE_SM))
        .rounded(px(RADIUS_MD))
        .border_color(theme.border)
        .bg(theme.surface)
        .text_size(gpui::rems(TEXT_SM / 16.0))
        .font_weight(FontWeight::BOLD)
}

pub fn catalog_facet_button(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool, disabled: bool) -> Button {
    let label = label.into();
    Button::new(id)
        .secondary()
        .outline()
        .selected(active)
        .disabled(active || disabled)
        .h(px(32.0))
        .max_w(px(crate::FOLDER_CARD_WIDTH))
        .px(px(SPACE_SM))
        .overflow_hidden()
        .rounded_full()
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .tooltip(label.clone())
        .child(div().max_w(px(crate::FOLDER_CARD_WIDTH - 24.0)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

pub fn folder_card(open: impl IntoElement, download: Option<impl IntoElement>) -> Div {
    div().w(px(crate::FOLDER_CARD_WIDTH)).h(px(41.0)).flex().items_center().gap(px(SPACE_XXS)).child(open).children(download)
}

pub fn hierarchy_card(open: impl IntoElement) -> Div {
    div().w_auto().max_w(px(crate::HIERARCHY_ITEM_MAX_WIDTH)).h(px(41.0)).flex_none().flex().items_center().child(open)
}

/// One book as a full-width row: cover art beside the text rather than under
/// it, with room for actions on the right.
///
/// The height is fixed at every window width — the description is line-clamped
/// rather than allowed to reflow into more lines — because the list that draws
/// these is virtualized and a uniform row height is what makes that possible.
pub fn book_row(id: impl Into<ElementId>, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(DETAILED_ROW_HEIGHT))
        .flex_none()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(SPACE_MD))
        .px(px(SPACE_MD))
        .py(px(DETAILED_ROW_VERTICAL_PADDING))
        .rounded(px(RADIUS_SM))
        .border_b_1()
        .border_color(theme.border)
        .hover(move |style| style.bg(theme.surface_alt))
}

pub fn book_row_cover(theme: BrowserTheme) -> Div {
    div().w(px(DETAILED_ROW_THUMBNAIL_WIDTH)).h(px(DETAILED_ROW_THUMBNAIL_HEIGHT)).flex_none().flex().flex_col().items_center().justify_end().overflow_hidden().rounded(px(RADIUS_SM)).bg(theme.surface_alt).cursor_pointer()
}

/// The text column, capped to a readable measure. Beyond that the row spends
/// its width on the action cluster instead of on longer lines.
pub fn book_row_text() -> Div {
    div().flex_1().min_w_0().max_w(px(DETAILED_ROW_TEXT_MEASURE)).flex().flex_col().gap(px(SPACE_XS))
}

pub fn book_row_title() -> Div {
    div().w_full().h(px(22.0)).flex_none().text_size(gpui::rems(TEXT_LG / 16.0)).font_weight(FontWeight::SEMIBOLD).line_clamp(1)
}

pub fn book_row_author(theme: BrowserTheme) -> Div {
    div().w_full().h(px(20.0)).flex_none().text_size(gpui::rems(TEXT_MD / 16.0)).text_color(theme.text_muted).line_clamp(1)
}

pub fn book_row_description(theme: BrowserTheme) -> Div {
    div().w_full().h(px(84.0)).flex_none().text_size(gpui::rems(TEXT_SM / 16.0)).line_height(gpui::relative(1.4)).text_color(theme.text_muted).line_clamp(DETAILED_ROW_DESCRIPTION_LINES)
}

/// Actions sit to the right of the text when the row is wide enough to hold
/// them there; below that width the caller omits this entirely.
pub fn book_row_actions() -> Div {
    div().flex_none().flex().flex_col().items_end().gap(px(SPACE_XS))
}

pub fn book_row_list() -> Div {
    div().w_full().flex().flex_col()
}

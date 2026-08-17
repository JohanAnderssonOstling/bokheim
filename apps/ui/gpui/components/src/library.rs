use gpui::prelude::*;
use gpui::{AnyElement, Div, ElementId, FontWeight, ImageSource, Img, ObjectFit, PathBuilder, SharedString, Stateful, StyledImage, canvas, div, img, point, px};
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Icon, IconName, Selectable, Sizable};

use crate::{
    AUTHOR_PAGE_MAX_WIDTH, AUTHOR_ROW_CONTENT_GAP, AUTHOR_ROW_COVER_COUNT, AUTHOR_ROW_COVER_GAP, AUTHOR_ROW_COVER_HEIGHT, AUTHOR_ROW_COVER_WIDTH, AUTHOR_ROW_HEIGHT, AUTHOR_ROW_HORIZONTAL_PADDING, AUTHOR_ROW_LABEL_SIZE,
    BOOK_CARD_AUTHOR_HEIGHT_REM, BOOK_CARD_AUTHOR_LINE_HEIGHT, BOOK_CARD_AUTHOR_SIZE, BOOK_CARD_COVER_CONTENT_GAP, BOOK_CARD_TEXT_GAP, BOOK_CARD_TITLE_HEIGHT_REM, BOOK_CARD_TITLE_LINE_HEIGHT, BOOK_CARD_TITLE_LINES, BOOK_CARD_TITLE_SIZE,
    BOOK_CARD_TITLE_WEIGHT, BOOK_COVER_HEIGHT, BOOK_COVER_WIDTH, BrowserTheme, CAROUSEL_CONTENT_GAP, CAROUSEL_CONTROL_GAP, CAROUSEL_SECTION_GAP, CAROUSEL_TITLE_SIZE, CAROUSEL_TITLE_WEIGHT, CATALOG_CHIP_HEIGHT, CATALOG_CONTROL_HEIGHT,
    CATALOG_FILTER_RAIL_WIDTH, CATALOG_SEARCH_WIDTH, DETAILED_ROW_DESCRIPTION_LINES, DETAILED_ROW_HEIGHT, DETAILED_ROW_TEXT_MEASURE, DETAILED_ROW_THUMBNAIL_HEIGHT, DETAILED_ROW_THUMBNAIL_WIDTH, DETAILED_ROW_VERTICAL_PADDING, RADIUS_MD,
    RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XXS, Scrollable, ScrollableElement, TEXT_LG, TEXT_MD, TEXT_SM, TEXT_XS, TITLE_MD,
};

/// Cover, then the text block. The two levels each have one gap: `SPACE_MD`
/// separates the art from the words, `SPACE_XS` separates the words from each
/// other. Previously the wider first gap was a margin on the title fighting the
/// column's own gap, which meant two spacing mechanisms in one component.
pub fn book_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).w_full().min_w(px(BOOK_COVER_WIDTH)).pb(px(crate::BOOK_GRID_ROW_TRAILING_SPACE)).flex_none().flex().flex_col().gap(px(BOOK_CARD_COVER_CONTENT_GAP))
}

pub fn book_card_text() -> Div {
    div().w_full().min_w(px(BOOK_COVER_WIDTH)).flex_none().flex().flex_col().gap(px(BOOK_CARD_TEXT_GAP))
}

pub fn book_cover(_theme: BrowserTheme) -> Div {
    div().w_full().min_w(px(BOOK_COVER_WIDTH)).aspect_ratio(BOOK_COVER_WIDTH / BOOK_COVER_HEIGHT).flex().flex_col().items_center().justify_end()
}

pub fn book_cover_frame() -> Div {
    div().relative().w_full().min_w(px(BOOK_COVER_WIDTH)).aspect_ratio(BOOK_COVER_WIDTH / BOOK_COVER_HEIGHT).cursor_pointer()
}

/// Cover art, whole.
///
/// The image element uses the JPEG's real ratio inside the standard cover slot.
/// Wide covers fill the slot's width; tall covers fill its height. Consequently
/// the element's border follows the painted cover instead of outlining the card
/// or an invisible letterbox, while the parent bottom-aligns every cover.
pub fn book_cover_image(source: impl Into<ImageSource>, aspect_ratio: f32, theme: BrowserTheme) -> Img {
    let frame_ratio = BOOK_COVER_WIDTH / BOOK_COVER_HEIGHT;
    img(source)
        .flex_none()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .shadow(vec![gpui_component::box_shadow(px(0.0), px(4.0), px(12.0), px(0.0), theme.shadow)])
        .object_fit(ObjectFit::Contain)
        .when(aspect_ratio >= frame_ratio, |image| image.w_full().h(gpui::relative(frame_ratio / aspect_ratio)))
        .when(aspect_ratio < frame_ratio, |image| image.w(gpui::relative(aspect_ratio / frame_ratio)).h_full())
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
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .gap(px(SPACE_XS))
        .child(Icon::new(IconName::BookOpen).large().text_color(theme.text_muted))
        .child(div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(label.into()))
}

pub fn book_card_title() -> Div {
    div()
        .w_full()
        .min_w(px(BOOK_COVER_WIDTH))
        .h(gpui::rems(BOOK_CARD_TITLE_HEIGHT_REM))
        .flex_none()
        .text_size(gpui::rems(BOOK_CARD_TITLE_SIZE / 16.0))
        .line_height(gpui::relative(BOOK_CARD_TITLE_LINE_HEIGHT / BOOK_CARD_TITLE_SIZE))
        .font_weight(BOOK_CARD_TITLE_WEIGHT)
        .line_clamp(BOOK_CARD_TITLE_LINES)
}

pub fn book_card_author(theme: BrowserTheme) -> Div {
    div()
        .w_full()
        .min_w(px(BOOK_COVER_WIDTH))
        .h(gpui::rems(BOOK_CARD_AUTHOR_HEIGHT_REM))
        .flex_none()
        .text_size(gpui::rems(BOOK_CARD_AUTHOR_SIZE / 16.0))
        .line_height(gpui::relative(BOOK_CARD_AUTHOR_LINE_HEIGHT / BOOK_CARD_AUTHOR_SIZE))
        .text_color(theme.text_muted)
        .line_clamp(1)
}

pub fn home_page() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(CAROUSEL_SECTION_GAP)).p(px(crate::CONTENT_PADDING)).overflow_y_scrollbar()
}

/// A heading and the row it names. The gap inside a section is smaller than
/// the one between sections, so a title belongs to the carousel under it
/// rather than floating equidistant between two rows.
pub fn carousel_section() -> Div {
    div().w_full().min_w_0().flex().flex_col().gap(px(CAROUSEL_CONTENT_GAP))
}

/// The title line: the section's name, and its scroll controls pushed to the
/// far end of the same row.
pub fn carousel_heading() -> Div {
    div().w_full().min_w_0().flex().flex_row().items_center().justify_between().gap(px(CAROUSEL_CONTENT_GAP))
}

pub fn carousel_title(text: impl Into<SharedString>) -> Div {
    div().flex_1().min_w_0().text_size(gpui::rems(CAROUSEL_TITLE_SIZE / 16.0)).font_weight(CAROUSEL_TITLE_WEIGHT).child(text.into())
}

pub fn carousel_controls() -> Div {
    div().flex_none().flex().flex_row().gap(px(CAROUSEL_CONTROL_GAP))
}

/// One step of a carousel. Square, quiet, and the same size on both frontends;
/// a horizontal row of cards is otherwise reachable only by a pointer that can
/// scroll sideways.
pub fn carousel_arrow_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    Button::new(id).icon(icon).secondary().outline().border_color(theme.border).text_color(theme.accent).w(px(crate::CAROUSEL_ARROW_SIZE)).h(px(crate::CAROUSEL_ARROW_SIZE)).rounded(px(RADIUS_SM)).tooltip(label.into())
}

pub fn authors_split_page() -> Div {
    div().w_full().max_w(px(AUTHOR_PAGE_MAX_WIDTH)).h_full().min_h_0().min_w_0().mx_auto().flex().flex_col()
}

pub fn author_list_panel(_width: f32, _theme: BrowserTheme) -> Div {
    div().w_full().h_full().min_h_0().flex().flex_col().gap(px(SPACE_MD)).p(px(SPACE_LG))
}

pub fn author_card_grid() -> Div {
    div().w_full().flex().flex_row().flex_wrap().items_start().gap(px(crate::BOOK_GRID_GAP))
}

/// The fixed width occupied by an author card with this many cover slots.
/// Kept alongside [`author_list_row`] so virtualized index rows use exactly
/// the same packing geometry as the cards they contain.
pub fn author_card_width(cover_count: usize) -> f32 {
    let cover_count = cover_count.clamp(1, AUTHOR_ROW_COVER_COUNT);
    AUTHOR_ROW_HORIZONTAL_PADDING * 2.0 + AUTHOR_ROW_COVER_WIDTH * cover_count as f32 + AUTHOR_ROW_COVER_GAP * cover_count.saturating_sub(1) as f32
}

/// One fixed-height row in the virtualized author index. The trailing grid
/// gap is included in the row height because `uniform_list` positions rows
/// directly adjacent to one another.
pub fn author_virtual_row() -> Div {
    div().w_full().h(px(AUTHOR_ROW_HEIGHT + crate::BOOK_GRID_GAP)).flex_none().flex().flex_row().items_start().gap(px(crate::BOOK_GRID_GAP))
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

/// One author card. Its width follows the one-to-three regular-size covers it
/// contains, while the name and vertical geometry remain identical.
pub fn author_list_row(id: impl Into<ElementId>, cover_count: usize, selected: bool, theme: BrowserTheme) -> Stateful<Div> {
    let width = author_card_width(cover_count);
    div()
        .id(id)
        .w(px(width))
        .min_h(px(AUTHOR_ROW_HEIGHT))
        .p(px(AUTHOR_ROW_HORIZONTAL_PADDING))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(AUTHOR_ROW_CONTENT_GAP))
        .rounded(px(RADIUS_SM))
        .shadow(vec![gpui_component::box_shadow(px(0.0), px(0.0), px(0.0), px(1.0), gpui::black())])
        .when(selected, |row| row.bg(theme.accent_soft))
        .when(!selected, |row| row.hover(move |style| style.bg(theme.hover)))
        .cursor_pointer()
}

pub fn author_row_covers(cover_count: usize) -> Div {
    let cover_count = cover_count.min(AUTHOR_ROW_COVER_COUNT);
    let width = AUTHOR_ROW_COVER_WIDTH * cover_count as f32 + AUTHOR_ROW_COVER_GAP * cover_count.saturating_sub(1) as f32;
    div().w(px(width)).h(px(AUTHOR_ROW_COVER_HEIGHT)).flex_none().flex().flex_row().gap(px(AUTHOR_ROW_COVER_GAP))
}

pub fn author_row_cover_frame() -> Div {
    div().w(px(AUTHOR_ROW_COVER_WIDTH)).h(px(AUTHOR_ROW_COVER_HEIGHT)).flex_none().flex().flex_col().items_center().justify_end().overflow_hidden()
}

pub fn author_row_cover_image(source: impl Into<ImageSource>, theme: BrowserTheme) -> Img {
    img(source).w(px(AUTHOR_ROW_COVER_WIDTH)).h_auto().max_h_full().border_1().border_color(theme.border).object_fit(ObjectFit::Contain)
}

/// The slot an author with one book — or one thumbnail — leaves empty. Drawn
/// rather than omitted, so the name keeps its place down the list.
pub fn author_row_missing_cover(title: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    author_row_cover_frame().border_1().border_color(theme.border).p(px(SPACE_MD)).child(
        div()
            .w_full()
            .line_clamp(4)
            .text_center()
            .text_size(gpui::rems(BOOK_CARD_AUTHOR_SIZE / 16.0))
            .line_height(gpui::rems(BOOK_CARD_AUTHOR_LINE_HEIGHT / 16.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text_muted)
            .child(title.into()),
    )
}

pub fn author_list_name(name: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .h(gpui::rems(BOOK_CARD_TITLE_HEIGHT_REM))
        .line_clamp(BOOK_CARD_TITLE_LINES)
        .text_size(gpui::rems(AUTHOR_ROW_LABEL_SIZE / 16.0))
        .line_height(gpui::rems(BOOK_CARD_TITLE_LINE_HEIGHT / 16.0))
        .font_weight(BOOK_CARD_TITLE_WEIGHT)
        .child(name.into())
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
pub fn hierarchy_cell(id: impl Into<ElementId>, label: impl Into<SharedString>, count: usize, icon: app_contract::NavigationIcon, end_action: Option<AnyElement>, width: f32, theme: BrowserTheme) -> Button {
    hierarchy_navigation_cell(id, label, Some(count), crate::navigation_icon(icon), end_action, width, theme)
}

/// A catalogue category/subject rendered with the same geometry as local
/// hierarchy cards. OPDS navigation counts are optional, so the trailing count
/// disappears when a catalogue does not provide one instead of claiming zero.
pub fn catalog_hierarchy_cell(id: impl Into<ElementId>, label: impl Into<SharedString>, count: Option<usize>, icon: IconName, width: f32, theme: BrowserTheme) -> Button {
    hierarchy_navigation_cell(id, label, count, gpui_component::Icon::new(icon), None, width, theme)
}

fn hierarchy_navigation_cell(id: impl Into<ElementId>, label: impl Into<SharedString>, count: Option<usize>, icon: gpui_component::Icon, end_action: Option<AnyElement>, width: f32, theme: BrowserTheme) -> Button {
    let label = label.into();
    let count = count.map(|count| div().flex_none().text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(count.to_string()));
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
                .child(icon.size(px(crate::HIERARCHY_ICON_SIZE)).text_color(theme.accent))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(gpui::rems(crate::HIERARCHY_LABEL_SIZE / 16.0)).font_weight(FontWeight::MEDIUM).text_color(theme.text).child(label))
                .children(count)
                .children(end_action),
        )
}

/// Compact trailing download status for a folder cell. An empty ring is 0%,
/// its accent arc grows with the downloaded share, and completion collapses
/// to a checkmark. The fixed slot keeps folder counts aligned.
pub fn hierarchy_download_action(id: impl Into<ElementId>, downloaded: usize, total: usize, label: impl Into<SharedString>, show_completion: bool, theme: BrowserTheme) -> Button {
    let progress = if total == 0 { 0.0 } else { downloaded.min(total) as f32 / total as f32 };
    let tooltip = if total == 0 { label.into() } else { format!("{downloaded} of {total} downloaded").into() };
    let button = Button::new(id).ghost().compact().w(px(22.0)).h(px(22.0)).p_0().text_color(theme.text_muted).tooltip(tooltip);
    if progress >= 1.0 && show_completion {
        return button.icon(IconName::Check).text_color(theme.accent);
    }
    if total == 0 || progress >= 1.0 {
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
        .rounded(px(RADIUS_SM))
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .tooltip(label.clone())
        .child(div().max_w(px(crate::FOLDER_CARD_WIDTH - 24.0)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

/// The catalogue page's controls on one line: search, the browse menu, and one
/// dropdown per facet axis.
///
/// Everything that narrows a catalogue shares this row rather than taking a
/// band of its own. Five stacked bands cost 365 px before the first book, and a
/// publication card is 424 px — so on a 700 px window nothing was legible until
/// the reader scrolled.
pub fn catalog_toolbar() -> Div {
    div().w_full().flex().flex_row().flex_wrap().items_center().gap(px(SPACE_XS))
}

/// The search field's slot in the toolbar. Bounded so a wide window spends its
/// room on results rather than on an oversized input.
pub fn catalog_toolbar_search() -> Div {
    div().flex_1().min_w(px(140.0)).max_w(px(CATALOG_SEARCH_WIDTH))
}

/// A toolbar dropdown trigger. `active` marks an axis the catalogue reports as
/// applied, so a filtered page reads as filtered with every menu closed.
pub fn catalog_dropdown_button(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool, disabled: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id)
        .label(label.clone())
        .secondary()
        .outline()
        .disabled(disabled)
        .h(px(CATALOG_CONTROL_HEIGHT))
        .max_w(px(crate::FOLDER_CARD_WIDTH))
        .px(px(SPACE_SM))
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .when(active, |button| button.border_color(theme.accent).text_color(theme.accent))
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .tooltip(label)
}

/// The line that says why the list is this short. Present only while something
/// is narrowing the results, so an unfiltered page pays nothing for it.
pub fn catalog_chip_row() -> Div {
    div().w_full().flex().flex_row().flex_wrap().items_center().gap(px(SPACE_XS))
}

/// One applied filter, named by its axis: `Language: Swedish`.
pub fn catalog_filter_chip(id: impl Into<ElementId>, axis: impl Into<SharedString>, value: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    let label = format!("{}: {}", axis.into(), value.into());
    Button::new(id)
        .ghost()
        .h(px(CATALOG_CHIP_HEIGHT))
        .px(px(SPACE_XS))
        .max_w(px(crate::FOLDER_CARD_WIDTH))
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.accent)
        .text_color(theme.accent)
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .tooltip(label.clone())
        .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
}

/// A chip whose value the page can actually undo, so it carries the affordance
/// that says so.
pub fn catalog_removable_chip(id: impl Into<ElementId>, axis: impl Into<SharedString>, value: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    catalog_filter_chip(id, axis, value, theme).icon(IconName::Close)
}

pub fn catalog_clear_filters_button(id: impl Into<ElementId>, label: impl Into<SharedString>, disabled: bool) -> Button {
    Button::new(id).label(label).ghost().disabled(disabled).h(px(CATALOG_CHIP_HEIGHT)).px(px(SPACE_XS)).text_size(gpui::rems(TEXT_XS / 16.0))
}

/// Results beside their filters. Used only where the width is there to spend;
/// below that the same controls render as [`catalog_toolbar`].
pub fn catalog_results_split() -> Div {
    div().w_full().flex_1().min_h_0().flex().flex_row().gap(px(SPACE_MD))
}

/// The filter column. Filters grow downward here into room the results were
/// never using, so the option list needs no dropdown to be read.
pub fn catalog_filter_rail(theme: BrowserTheme) -> Scrollable<Div> {
    div().w(px(CATALOG_FILTER_RAIL_WIDTH)).flex_none().h_full().min_h_0().p(px(SPACE_SM)).flex().flex_col().gap(px(SPACE_MD)).border_1().border_color(theme.border).bg(theme.raised_bg).overflow_y_scrollbar()
}

pub fn catalog_rail_group() -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_XXS))
}

pub fn catalog_rail_heading(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().pb(px(SPACE_XXS)).text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(text.into())
}

/// One facet option in the rail. The count sits in its own trailing column so
/// figures read down the list rather than landing wherever the label ended.
pub fn catalog_rail_option(id: impl Into<ElementId>, label: impl Into<SharedString>, count: Option<u32>, active: bool, disabled: bool, theme: BrowserTheme) -> Button {
    let label = label.into();
    let mut row = div().w_full().flex().items_center().justify_between().gap(px(SPACE_XS)).child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label.clone()));
    if let Some(count) = count {
        row = row.child(div().flex_none().text_color(theme.text_muted).child(count.to_string()));
    }
    Button::new(id)
        .ghost()
        .selected(active)
        .disabled(disabled)
        .w_full()
        .h(px(CATALOG_CONTROL_HEIGHT))
        .px(px(SPACE_XS))
        .justify_start()
        .rounded(px(RADIUS_SM))
        .text_size(gpui::rems(TEXT_XS / 16.0))
        .when(active, |button| button.bg(theme.accent_soft).text_color(theme.accent).font_weight(FontWeight::SEMIBOLD))
        .tooltip(label)
        .child(row)
}

/// Every catalogue control on one line: back, source, path, applied filters,
/// then the search, menus and pager pushed to the trailing edge.
///
/// Wraps rather than truncates, so a narrow window costs a second line only
/// when it has to. This replaces four separate bands — identity, toolbar,
/// chips and a centred pager — that between them cost about 130 px whether or
/// not they had anything to say.
pub fn catalog_bar() -> Div {
    div().w_full().flex_none().flex().flex_row().flex_wrap().items_center().gap(px(SPACE_XS))
}

/// Pushes what follows to the trailing edge of [`catalog_bar`].
pub fn catalog_bar_spacer() -> Div {
    div().flex_1().min_w(px(SPACE_MD))
}

/// A thin rule between groups sharing the bar, so the line reads as sections
/// rather than one undifferentiated run of controls.
pub fn catalog_bar_divider(theme: BrowserTheme) -> Div {
    div().w(px(1.0)).h(px(CATALOG_CONTROL_HEIGHT - SPACE_SM)).flex_none().bg(theme.border)
}

/// Page stepping as two icons on the bar. The labelled buttons this replaces
/// sat in a centred row of their own under the results.
pub fn catalog_pager_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).icon(icon).ghost().disabled(disabled).size(px(CATALOG_CONTROL_HEIGHT)).rounded(px(RADIUS_SM)).tooltip(label)
}

/// Section title and count on one line rather than stacked.
///
/// Virtual rows declare their height in advance, so a header that stacks a
/// 21 px title over a 12 px detail forces every section to reserve 58 px. One
/// line needs 34, and the count reads better beside the title than under it.
pub fn catalog_section_heading(title: impl Into<SharedString>, detail: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    let detail = detail.into();
    let mut heading = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_row()
        .items_baseline()
        .gap(px(SPACE_XS))
        .child(div().flex_shrink(1.0).min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(gpui::rems(TEXT_MD / 16.0)).font_weight(FontWeight::SEMIBOLD).child(title.into()));
    if !detail.is_empty() {
        heading = heading.child(div().flex_none().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(detail));
    }
    heading
}

pub fn catalog_source_name(text: impl Into<SharedString>) -> Div {
    div().flex_none().text_size(gpui::rems(TEXT_SM / 16.0)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn catalog_back_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).icon(IconName::ArrowLeft).ghost().h(px(CATALOG_CONTROL_HEIGHT)).px(px(SPACE_XS)).text_size(gpui::rems(TEXT_XS / 16.0))
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
        .hover(move |style| style.bg(theme.hover))
}

pub fn book_row_cover(theme: BrowserTheme) -> Div {
    div()
        .w(px(DETAILED_ROW_THUMBNAIL_WIDTH))
        .h(px(DETAILED_ROW_THUMBNAIL_HEIGHT))
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .justify_end()
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .cursor_pointer()
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

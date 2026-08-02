use crate::components::buttons::secondary_button;
use crate::components::empty_state::inline_message;
use crate::components::surface::page_surface;
use crate::components::text::{subtle_label, title_label, ui_label};
use crate::components::tokens::TOKENS;
use crate::style::palette;
use floem::event::{Event, EventPropagation};
use floem::prelude::{container, h_stack, v_stack, Button, Decorators, Label, View};
use floem::style::{AlignItems, FlexWrap};
use floem::views::stack_from_iter;
use floem::views::ScrollExt;
use floem::IntoView;

const CAROUSEL_GAP: i32 = 16;
const CAROUSEL_TITLE_SIZE: i32 = 20;
const CAROUSEL_ROW_HEIGHT: i32 = 380;
const CAROUSEL_PROGRESS_WIDTH: i32 = 200;

// --- Book grids ---

pub fn books_grid<V>(books: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(books).style(|s| s.gap(28).flex_row().flex_wrap(FlexWrap::Wrap).width_full().min_width(0))
}

pub fn books_grid_with_empty_state(book_count: usize, grid: impl View + 'static) -> impl View {
    v_stack((inline_message("No books in this folder").style(move |s| if book_count == 0 { s } else { s.hide() }), grid)).style(|s| s.width_full().flex_col().gap(10))
}

// --- Authors tab ---

pub fn author_back_with_title(back_button: impl View + 'static, title: String) -> impl View {
    h_stack((back_button, title_label(move || title.clone()).style(|s| s.font_size(18).padding_left(8))))
        .style(|s| s.width_full().align_items(AlignItems::Center).padding_horiz(TOKENS.spacing.lg).padding_top(TOKENS.spacing.md).padding_bottom(TOKENS.spacing.sm).gap(TOKENS.spacing.sm))
}

pub fn author_scroll_content(child: impl View + 'static) -> impl View {
    child.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding_horiz(TOKENS.spacing.lg).padding_bottom(TOKENS.spacing.lg))
}

pub fn author_cards_page(cards: impl View + 'static, load_more: impl View + 'static) -> impl View {
    page_surface(v_stack((
        cards.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0)),
        load_more.style(|s| s.width_full().padding_top(TOKENS.spacing.md).justify_center().align_items(AlignItems::Center)),
    )))
    .style(|s| s.padding(TOKENS.spacing.lg))
}

pub fn author_cards_grid<V>(cards: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(cards).style(|s| s.width_full().min_width(0).gap(TOKENS.spacing.md).flex_row().flex_wrap(FlexWrap::Wrap))
}

pub fn author_group_title(folder_path: String) -> impl View {
    ui_label(move || folder_path.clone()).style(|s| s.font_size(16).font_bold().padding_bottom(8))
}

pub fn author_group_stack<V>(groups: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(groups).style(|s| s.width_full().gap(14))
}

pub fn author_load_more_button(show_more: bool) -> Button {
    secondary_button("Load more authors").style(move |s| if show_more { s } else { s.hide() })
}

// --- Folders tab ---

pub fn folder_tree_panel(child: impl View + 'static) -> impl View {
    container(child).style(|s| {
        let theme = palette();
        s.width_full().height_full().min_height(0).padding(TOKENS.spacing.sm).background(theme.surface_bg).border_right(1).border_color(theme.border_color)
    })
}

/// Scrollable wrapper for the folder tree inside `folder_tree_panel`.
pub fn folder_tree_scroll(child: impl View + 'static) -> impl View {
    child.style(|s| s.width_full().min_width(0)).scroll().style(|s| s.min_width(0).height_full().flex_grow(1.0).min_height(0))
}

/// Sizes the (dynamically rebuilt) tree side of the folders split.
pub fn folder_tree_slot(view: impl View + 'static) -> impl View {
    view.style(|s| s.height_full().min_height(0))
}

/// Sizes the (dynamically rebuilt) books side of the folders split.
pub fn folder_books_slot(view: impl View + 'static) -> impl View {
    view.style(|s| s.width_full().height_full().flex_grow(1.0).flex_basis(0).min_height(0).min_width(0))
}

/// Scrollable wrapper for a books grid.
pub fn books_scroll(child: impl View + 'static) -> impl View {
    child.scroll().style(|s| s.size_full().min_height(0).min_width(0))
}

/// Outer frame around the folders split view.
pub fn folder_workspace(split: impl View + 'static) -> impl View {
    split.style(|s| s.width_full().height_full().flex_grow(1.0).flex_basis(0).min_height(0).min_width(0).padding(TOKENS.spacing.lg))
}

pub fn folder_books_panel(breadcrumb: impl View + 'static, books_scroll: impl View + 'static) -> impl View {
    v_stack((breadcrumb, books_scroll.style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0))))
        .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0).padding(TOKENS.spacing.lg).gap(TOKENS.spacing.md))
}

/// Shared crumb chrome: click-target padding + hover highlight.
fn breadcrumb_crumb(crumb: Label) -> Label {
    crumb.style(|s| s.padding_horiz(TOKENS.spacing.sm).padding_vert(TOKENS.spacing.xs).hover(|s| s.background(palette().active_highlight)))
}

pub fn folder_breadcrumb_root(title: String, on_click: impl FnMut(&Event) -> EventPropagation + 'static) -> impl View {
    breadcrumb_crumb(title_label(move || title.clone())).on_click(on_click).style(|s| s.font_size(TOKENS.typography.title_lg).color(palette().accent).font_bold())
}

fn folder_breadcrumb_separator() -> impl View {
    subtle_label(|| "/".to_string()).style(|s| s.padding_left(TOKENS.spacing.xs).padding_right(TOKENS.spacing.xs).font_size(TOKENS.typography.title_md))
}

/// One "/ name" crumb that shows itself only while `visible` returns true.
pub fn folder_breadcrumb_crumb(part: impl View + 'static, visible: impl Fn() -> bool + 'static) -> impl View {
    h_stack((folder_breadcrumb_separator(), part)).style(move |s| if visible() { s.align_items(AlignItems::Center) } else { s.hide() })
}

pub fn folder_breadcrumb_slots<V>(slots: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(slots).style(|s| s.align_items(AlignItems::Center))
}

pub fn folder_breadcrumb_part(text: impl Fn() -> String + 'static, on_click: impl FnMut(&Event) -> EventPropagation + 'static) -> impl View {
    breadcrumb_crumb(ui_label(text)).on_click(on_click).style(|s| s.font_size(TOKENS.typography.title_md))
}

pub fn folder_breadcrumb_bar(root_crumb: impl View + 'static, crumb_slots: impl View + 'static) -> impl View {
    container(h_stack((root_crumb, crumb_slots))).style(|s| s.width_full().align_items(AlignItems::Center).justify_start().padding_vert(TOKENS.spacing.sm))
}

// --- Home tab carousels ---

pub fn carousel_section(title: &'static str, row: impl View + 'static) -> impl View {
    v_stack((carousel_title(title), row)).style(|s| s.gap(CAROUSEL_GAP).width_full().min_width(0).max_width_full())
}

pub fn carousel_row(child: impl View + 'static) -> impl View {
    child.style(|s| s.flex_row().gap(CAROUSEL_GAP).padding_bottom(6).height(CAROUSEL_ROW_HEIGHT).min_width(0).max_width_full()).scroll().style(|s| s.width_full().height(CAROUSEL_ROW_HEIGHT).min_width(0).max_width_full())
}

pub fn carousel_stack<V>(sections: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(sections).style(|s| s.flex_col().width_full().padding(CAROUSEL_GAP).gap(CAROUSEL_GAP).min_width(0).max_width_full())
}

fn carousel_title(title: &'static str) -> impl View {
    title_label(move || title.to_string()).style(|s| s.font_size(CAROUSEL_TITLE_SIZE))
}

pub fn carousel_progress_label(show_progress: bool, progress_text: String) -> impl View {
    subtle_label(move || if show_progress { format!("Progress {progress_text}") } else { String::new() }).style(|s| s.font_size(TOKENS.typography.text_xs).width(CAROUSEL_PROGRESS_WIDTH).padding_left(4))
}

/// Scrollable page wrapper for the home-tab carousel stack.
pub fn carousel_page_scroll(child: impl View + 'static) -> impl View {
    child.style(|s| s.width_full().height_full().min_height(0).min_width(0).max_width_full()).scroll().style(|s| s.width_full().height_full().min_height(0).min_width(0))
}

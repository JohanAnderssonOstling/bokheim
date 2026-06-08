use crate::style::palette;
use floem::prelude::{button, container, h_stack, label, v_stack, Button, Decorators, View};
use floem::style::{AlignItems, FlexWrap};
use floem::views::ScrollExt;
use floem::views::stack_from_iter;
use floem::IntoView;

const HOME_CAROUSEL_PADDING: i32 = 16;
const HOME_CAROUSEL_SECTION_GAP: i32 = 16;
const HOME_CAROUSEL_ROW_GAP: i32 = 16;
const HOME_CAROUSEL_CARD_GAP: i32 = 4;
const HOME_CAROUSEL_TITLE_SIZE: i32 = 20;
const HOME_CAROUSEL_PROGRESS_SIZE: i32 = 12;
const HOME_CAROUSEL_ROW_HEIGHT: i32 = 380;
const HOME_CAROUSEL_PROGRESS_WIDTH: i32 = 200;

pub fn section_title(text: &'static str) -> impl View {
    label(move || text).style(|s| {
        let theme = palette();
        s.font_size(14).font_family(theme.title_font_family.to_string()).color(theme.ink_text)
    })
}

pub fn message_row(text: &'static str) -> impl View {
    container(label(move || text).style(|s| {
        let theme = palette();
        s.padding_horiz(6).padding_vert(4).font_family(theme.ui_font_family.to_string()).color(theme.subtle_text)
    }))
    .style(|s| s.width_full())
}

pub fn panel_card(child: impl View + 'static) -> impl View {
    container(child).style(|s| {
        let theme = palette();
        s.width_full().padding(12).background(theme.surface_bg).border(1).border_color(theme.border_color).border_radius(12)
    })
}

pub fn back_with_title(title: String, on_back: impl FnMut(&floem::event::Event) -> floem::event::EventPropagation + 'static) -> impl View {
    h_stack((
        button(label(|| "Back")).on_click(on_back),
        label(move || title.clone()).style(|s| {
            let theme = palette();
            s.font_size(18).font_family(theme.title_font_family.to_string()).color(theme.ink_text).padding_left(8)
        }),
    ))
    .style(|s| s.width_full().align_items(AlignItems::Center).padding_horiz(16).padding_top(14).padding_bottom(10).gap(6))
}

pub fn surface(child: impl View + 'static) -> impl View {
    container(child).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).background(palette().white_surface))
}

pub fn home_carousel_scroll_area(child: impl View + 'static) -> impl View {
    child
        .style(|s| s.flex_col().width_full().height_full().flex_grow(1.0).min_height(0).padding(HOME_CAROUSEL_PADDING).gap(HOME_CAROUSEL_SECTION_GAP))
        .scroll()
        .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0))
}

pub fn home_carousel_section(title: &'static str, row: impl View + 'static) -> impl View {
    v_stack((home_carousel_title(title), row)).style(|s| s.gap(HOME_CAROUSEL_ROW_GAP).width_full())
}

pub fn home_carousel_row(child: impl View + 'static) -> impl View {
    child.style(|s| s.flex_row().gap(HOME_CAROUSEL_ROW_GAP).padding_bottom(6).width_full().height(HOME_CAROUSEL_ROW_HEIGHT))
}

pub fn home_book_card_stack(cover: impl View + 'static, progress: impl View + 'static) -> impl View {
    v_stack((cover, progress)).style(|s| s.gap(HOME_CAROUSEL_CARD_GAP))
}

pub fn home_carousel_title(title: &'static str) -> impl View {
    label(move || title).style(|s| {
        let theme = palette();
        s.font_size(HOME_CAROUSEL_TITLE_SIZE).font_family(theme.title_font_family.to_string()).color(theme.ink_text)
    })
}

pub fn home_progress_label(show_progress: bool, progress_label: String) -> impl View {
    label(move || if show_progress { format!("Progress {progress_label}") } else { String::new() })
        .style(|s| {
            let theme = palette();
            s.font_size(HOME_CAROUSEL_PROGRESS_SIZE).font_family(theme.ui_font_family.to_string()).color(theme.subtle_text).width(HOME_CAROUSEL_PROGRESS_WIDTH).padding_left(4)
        })
}

pub fn library_scroll_content(child: impl View + 'static) -> impl View {
    container(child).scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding_horiz(16).padding_bottom(16))
}

pub fn author_cards_page(cards: impl View + 'static, load_more: impl View + 'static) -> impl View {
    container(v_stack((
        cards.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0)),
        container(load_more).style(|s| s.width_full().padding_top(10).justify_center().align_items(AlignItems::Center)),
    )))
    .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding(16).background(palette().white_surface))
}

pub fn author_cards_grid<V>(cards: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(cards).style(|s| s.width_full().gap(12).flex_row().flex_wrap(FlexWrap::Wrap))
}

pub fn author_card_button(author_name: String, count: usize) -> Button {
    let name_for_label = author_name.clone();
    button(v_stack((
        label(move || name_for_label.clone()).style(|s| s.font_size(16).font_bold().width(260).text_ellipsis()),
        label(move || format!("({count})")).style(|s| s.font_size(13).color(palette().subtle_text)),
    )))
    .style(|s| {
        let theme = palette();
        s.width(280).padding_horiz(12).padding_vert(10).justify_start().align_items(AlignItems::Start).border(1).border_color(theme.border_color).border_radius(10).background(theme.white_surface).hover(|s| s.background(theme.active_highlight))
    })
}

pub fn load_more_button(show_more: bool) -> Button {
    button(label(|| "Load more authors"))
        .style(move |s| {
            let theme = palette();
            if show_more { s.padding_horiz(12).padding_vert(8).border(1).border_color(theme.border_color).border_radius(8).background(theme.white_surface) } else { s.hide() }
        })
}

pub fn author_group_title(folder_path: String) -> impl View {
    label(move || folder_path.clone()).style(|s| s.font_size(16).font_bold().padding_bottom(8))
}

pub fn author_group_stack<V>(groups: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    container(stack_from_iter(groups).style(|s| s.width_full().gap(14))).style(|s| s.width_full())
}

pub fn books_grid<V>(books: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(books).style(|s| s.gap(28).flex_row().flex_wrap(FlexWrap::Wrap).width_full())
}

pub fn books_grid_with_empty_state(book_count: usize, grid: impl View + 'static) -> impl View {
    v_stack((
        label(move || "No books in this folder".to_string()).style(move |s| {
            if book_count == 0 {
                let theme = palette();
                s.padding_horiz(6).padding_vert(4).color(theme.subtle_text).font_family(theme.ui_font_family.to_string())
            } else {
                s.hide()
            }
        }),
        grid,
    ))
    .style(|s| s.width_full().flex_col().gap(10))
}

pub fn folder_layout_bar(split: impl View + 'static, dashboard: impl View + 'static) -> impl View {
    h_stack((
        label(|| "Layout").style(|s| {
            let theme = palette();
            s.font_size(14).font_family(theme.ui_font_family.to_string()).color(theme.subtle_text).padding_right(6)
        }),
        split,
        dashboard,
    ))
    .style(|s| {
        let theme = palette();
        s.width_full().padding_horiz(16).padding_top(10).padding_bottom(6).align_items(AlignItems::Center).gap(8).border_bottom(1).border_color(theme.border_color).background(theme.white_surface)
    })
}

pub fn folder_tree_panel(child: impl View + 'static) -> impl View {
    v_stack((child,)).style(|s| {
        let theme = palette();
        s.width_full().height_full().padding(12).background(theme.surface_bg).border(1).border_color(theme.border_color).border_radius(12).min_height(0)
    })
}

pub fn folder_panel_title(text: &'static str) -> impl View {
    label(move || text).style(|s| {
        let theme = palette();
        s.font_size(14).font_family(theme.title_font_family.to_string()).color(theme.ink_text).padding_bottom(4)
    })
}

pub fn dashboard_folder_list<V>(rows: impl IntoIterator<Item = V>) -> impl View
where
    V: IntoView + 'static,
{
    stack_from_iter(rows).style(|s| s.width_full().flex_col().gap(8))
}

pub fn dashboard_folder_section(has_folders: bool, folder_list: impl View + 'static) -> impl View {
    container(v_stack((folder_panel_title("Folders"), folder_list)))
        .style(move |s| {
            let theme = palette();
            if has_folders {
                s.width(320).padding(12).background(theme.surface_bg).border(1).border_color(theme.border_color).border_radius(12).height_full().min_height(0)
            } else {
                s.hide()
            }
        })
        .scroll()
        .style(move |s| {
            if has_folders {
                s.width(320).height_full().min_height(0)
            } else {
                s.hide()
            }
        })
}

pub fn dashboard_folder_row(dir_name: String, book_count: usize) -> Button {
    button(label(move || format!("📁 {dir_name} ({book_count})")).style(|s| {
        let theme = palette();
        s.font_size(14).font_family(theme.title_font_family.to_string()).text_ellipsis().width_full().color(theme.ink_text)
    }))
    .style(|s| {
        let theme = palette();
        s.width_full().padding_horiz(10).padding_vert(8).justify_start().align_items(AlignItems::Start).border(1).border_color(theme.border_color).border_radius(8).background(theme.white_surface).hover(|s| s.background(theme.active_highlight))
    })
}

pub fn breadcrumb_root(title: String) -> impl View {
    label(move || title.clone()).style(|s| {
        let theme = palette();
        s.padding_horiz(6).padding_vert(4).font_family(theme.title_font_family.to_string()).color(theme.accent).font_bold().hover(|s| s.background(theme.active_highlight))
    })
}

pub fn breadcrumb_separator() -> impl View {
    label(|| "/").style(|s| s.padding_left(2).padding_right(2))
}

pub fn breadcrumb_part(text: impl Fn() -> String + 'static) -> impl View {
    label(text).style(|s| {
        let theme = palette();
        s.padding_horiz(6).padding_vert(4).font_family(theme.ui_font_family.to_string()).color(theme.ink_text).hover(|s| s.background(theme.active_highlight))
    })
}

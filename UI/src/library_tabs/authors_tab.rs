use crate::library_tabs::folders_tab::create_books_grid;
use crate::page_navigation::Signals;
use crate::style::{ACTIVE_HIGHLIGHT, BORDER_COLOR, PANEL_BG};
use floem::event::EventPropagation;
use floem::peniko::Color;
use floem::prelude::{button, container, create_rw_signal, dyn_view, h_stack, label, v_stack, Decorators, ScrollExt, SignalGet, SignalUpdate};
use floem::reactive::create_effect;
use floem::style::{AlignItems, FlexWrap};
use floem::views::stack_from_iter;
use floem::View;
use std::cmp::Ordering;
use std::rc::Rc;

#[derive(Clone)]
struct AuthorSummary {
    name: String,
    book_count: usize,
}

#[derive(Clone)]
struct AuthorFolderGroup {
    folder_path: String,
    books: Vec<epub_svelte_lib::library::Book>,
}

pub fn authors_tab_view(library_id: String, signals: Signals) -> impl View {
    let selected_author = create_rw_signal(None::<String>);
    let author_summaries = create_rw_signal(Rc::new(Vec::<AuthorSummary>::new()));
    let author_groups = create_rw_signal(Rc::new(Vec::<AuthorFolderGroup>::new()));
    let visible_authors_count = create_rw_signal(200_usize);
    let library_loaded = create_rw_signal(false);

    {
        let library_id_for_index = library_id.clone();
        let signals_for_index = signals.clone();
        let author_summaries_for_index = author_summaries;
        let library_loaded_for_index = library_loaded;
        create_effect(move |_| {
            let _refresh_tick = signals_for_index.library_refresh_tick.get();
            let handles = signals_for_index.library_handles.get();
            let Some(library) = handles.get(&library_id_for_index) else {
                author_summaries_for_index.set(Rc::new(vec![]));
                library_loaded_for_index.set(false);
                return;
            };

            let mut summaries = library
                .get_authors()
                .into_iter()
                .map(|author| AuthorSummary { name: author.name, book_count: author.book_count })
                .collect::<Vec<_>>();
            summaries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            author_summaries_for_index.set(Rc::new(summaries));
            library_loaded_for_index.set(true);
        });
    }

    {
        let library_id_for_groups = library_id.clone();
        let signals_for_groups = signals.clone();
        let selected_author_for_groups = selected_author;
        let author_groups_for_groups = author_groups;
        create_effect(move |_| {
            let _refresh_tick = signals_for_groups.library_refresh_tick.get();
            let Some(author_name) = selected_author_for_groups.get() else {
                author_groups_for_groups.set(Rc::new(vec![]));
                return;
            };

            let handles = signals_for_groups.library_handles.get();
            let Some(library) = handles.get(&library_id_for_groups) else {
                author_groups_for_groups.set(Rc::new(vec![]));
                return;
            };

            let mut groups = library
                .get_author_folder_books(&author_name)
                .into_iter()
                .map(|group| AuthorFolderGroup { folder_path: group.folder_path, books: group.books })
                .collect::<Vec<_>>();
            groups.sort_by(|a, b| compare_folder_hierarchy(&a.folder_path, &b.folder_path));
            author_groups_for_groups.set(Rc::new(groups));
        });
    }

    dyn_view(move || {
        if !library_loaded.get() {
            return Box::new(container(label(|| "Library not loaded")).style(|s| s.padding(16))) as Box<dyn View>;
        }
        if let Some(author_name) = selected_author.get() {
            let groups = author_groups.get();
            let total_books: usize = groups.iter().map(|group| group.books.len()).sum();

            let sections = if groups.is_empty() {
                container(label(|| "No books found for this author").style(|s| s.padding_horiz(6).padding_vert(4).color(Color::rgb8(75, 85, 99)))).style(|s| s.width_full())
            } else {
                let library_id_for_sections = library_id.clone();
                let signals_for_sections = signals.clone();
                container(
                    stack_from_iter(groups.iter().cloned().map(move |group| {
                        let mut books = group.books;
                        books.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                        container(v_stack((
                            label(move || group.folder_path.clone()).style(|s| s.font_size(16).font_bold().padding_bottom(8)),
                            create_books_grid(books, library_id_for_sections.clone(), signals_for_sections.clone()),
                        )))
                        .style(|s| s.width_full().padding(12).background(PANEL_BG).border(1).border_color(BORDER_COLOR).border_radius(12))
                    }))
                    .style(|s| s.width_full().gap(14)),
                )
                .style(|s| s.width_full())
            };

            return Box::new(
                v_stack((
                    h_stack((
                        button(label(|| "Back")).on_click(move |_| {
                            selected_author.set(None);
                            EventPropagation::Continue
                        }),
                        label(move || format!("{author_name} ({total_books})")).style(|s| s.font_size(18).font_bold().padding_left(8)),
                    ))
                    .style(|s| s.width_full().align_items(AlignItems::Center).padding_horiz(16).padding_top(14).padding_bottom(10).gap(6)),
                    sections.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding_horiz(16).padding_bottom(16)),
                ))
                .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).background(Color::WHITE)),
            ) as Box<dyn View>;
        }

        let authors = author_summaries.get();
        if authors.is_empty() {
            return Box::new(container(label(|| "No authors found")).style(|s| s.padding(16))) as Box<dyn View>;
        }

        let visible_count = visible_authors_count.get().min(authors.len());
        let cards = stack_from_iter(authors.iter().take(visible_count).cloned().map(move |author| {
            let author_name = author.name.clone();
            let count = author.book_count;
            button(v_stack((
                label(move || author_name.clone()).style(|s| s.font_size(16).font_bold().width(260).text_ellipsis()),
                label(move || format!("({count})")).style(|s| s.font_size(13).color(Color::rgb8(75, 85, 99))),
            )))
            .on_click(move |_| {
                selected_author.set(Some(author.name.clone()));
                EventPropagation::Continue
            })
            .style(|s| s.width(280).padding_horiz(12).padding_vert(10).justify_start().align_items(AlignItems::Start).border(1).border_color(BORDER_COLOR).border_radius(10).background(Color::WHITE).hover(|s| s.background(ACTIVE_HIGHLIGHT)))
        }))
        .style(|s| s.width_full().gap(12).flex_row().flex_wrap(FlexWrap::Wrap));

        let show_more = visible_count < authors.len();
        let load_more = button(label(|| "Load more authors"))
            .on_click(move |_| {
                visible_authors_count.update(|count| *count += 200);
                EventPropagation::Continue
            })
            .style(move |s| if show_more { s.padding_horiz(12).padding_vert(8).border(1).border_color(BORDER_COLOR).border_radius(8).background(Color::WHITE) } else { s.hide() });

        Box::new(
            container(v_stack((
                cards.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0)),
                container(load_more).style(|s| s.width_full().padding_top(10).justify_center().align_items(AlignItems::Center)),
            )))
            .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding(16).background(Color::WHITE)),
        ) as Box<dyn View>
    })
}

fn compare_folder_hierarchy(a: &str, b: &str) -> Ordering {
    let a_parts: Vec<&str> = a.split('/').collect();
    let b_parts: Vec<&str> = b.split('/').collect();
    for (lhs, rhs) in a_parts.iter().zip(b_parts.iter()) {
        let cmp = lhs.to_lowercase().cmp(&rhs.to_lowercase());
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    a_parts.len().cmp(&b_parts.len()).then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
}

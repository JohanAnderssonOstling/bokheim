use crate::components::library_views::{
    author_card_button, author_cards_grid, author_cards_page, author_group_stack, author_group_title, back_with_title, library_scroll_content,
    load_more_button, message_row, panel_card, surface,
};
use crate::library_tabs::folders_tab::create_books_grid;
use crate::page_navigation::Signals;
use floem::event::EventPropagation;
use floem::prelude::{create_rw_signal, dyn_view, v_stack, Decorators, SignalGet, SignalUpdate};
use floem::reactive::create_effect;
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
            let _ = signals_for_index.libraries.get();
            let Some(state) = signals_for_index.library_by_id(&library_id_for_index) else {
                author_summaries_for_index.set(Rc::new(vec![]));
                library_loaded_for_index.set(false);
                return;
            };
            let library = state.handle(signals_for_index.library_event_counter.clone());

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

            let _ = signals_for_groups.libraries.get();
            let Some(state) = signals_for_groups.library_by_id(&library_id_for_groups) else {
                author_groups_for_groups.set(Rc::new(vec![]));
                return;
            };
            let library = state.handle(signals_for_groups.library_event_counter.clone());

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
            return Box::new(message_row("Library not loaded")) as Box<dyn View>;
        }
        if let Some(author_name) = selected_author.get() {
            let groups = author_groups.get();
            let total_books: usize = groups.iter().map(|group| group.books.len()).sum();

            let sections: Box<dyn View> = if groups.is_empty() {
                Box::new(message_row("No books found for this author"))
            } else {
                let library_id_for_sections = library_id.clone();
                let signals_for_sections = signals.clone();
                let groups = groups.as_ref().clone();
                Box::new(author_group_stack(groups.into_iter().map(move |group| {
                        let mut books = group.books;
                        books.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                        panel_card(v_stack((
                            author_group_title(group.folder_path),
                            create_books_grid(books, library_id_for_sections.clone(), signals_for_sections.clone()),
                        )))
                    })))
            };

            return Box::new(
                surface(v_stack((
                    back_with_title(format!("{author_name} ({total_books})"), move |_| {
                        selected_author.set(None);
                        EventPropagation::Continue
                    }),
                    library_scroll_content(sections),
                ))),
            ) as Box<dyn View>;
        }

        let authors = author_summaries.get();
        if authors.is_empty() {
            return Box::new(message_row("No authors found")) as Box<dyn View>;
        }

        let visible_count = visible_authors_count.get().min(authors.len());
        let visible_authors = authors.iter().take(visible_count).cloned().collect::<Vec<_>>();
        let cards = author_cards_grid(visible_authors.into_iter().map(move |author| {
            let author_name = author.name.clone();
            let count = author.book_count;
            author_card_button(author_name, count)
            .on_click(move |_| {
                selected_author.set(Some(author.name.clone()));
                EventPropagation::Continue
            })
        }));

        let show_more = visible_count < authors.len();
        let load_more = load_more_button(show_more)
            .on_click(move |_| {
                visible_authors_count.update(|count| *count += 200);
                EventPropagation::Continue
            });

        Box::new(author_cards_page(cards, load_more)) as Box<dyn View>
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

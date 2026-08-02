use crate::components::{
    author_back_with_title, author_cards_grid, author_cards_page, author_group_stack, author_group_title, author_load_more_button, author_scroll_content, card_surface, inline_message, library_item_card, page_surface, secondary_button,
};
use crate::library_tabs::create_books_grid;
use crate::page_navigation::Signals;
use floem::event::EventPropagation;
use floem::prelude::{create_rw_signal, dyn_view, v_stack, Decorators, SignalGet, SignalUpdate, View};
use floem::reactive::create_effect;
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
    let visible_authors_count = create_rw_signal(400_usize);
    let library_loaded = create_rw_signal(false);

    {
        let library_id_for_index = library_id.clone();
        let signals_for_index = signals.clone();
        let author_summaries_for_index = author_summaries;
        let library_loaded_for_index = library_loaded;
        create_effect(move |_| {
            let _refresh_tick = signals_for_index.library_refresh_tick.get();
            let _ = signals_for_index.libraries.get();
            if signals_for_index.library_by_id(&library_id_for_index).is_none() {
                author_summaries_for_index.set(Rc::new(vec![]));
                library_loaded_for_index.set(false);
                return;
            }

            let mut summaries = signals_for_index
                .app_core
                .authors_in(&library_id_for_index)
                .unwrap_or_else(|error| {
                    eprintln!("failed to load authors: {error}");
                    Vec::new()
                })
                .into_iter()
                .map(|author| AuthorSummary { name: author.name().as_str().to_owned(), book_count: author.book_count() })
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
            if signals_for_groups.library_by_id(&library_id_for_groups).is_none() {
                author_groups_for_groups.set(Rc::new(vec![]));
                return;
            }

            let mut groups = signals_for_groups
                .app_core
                .author_folder_books_in(&library_id_for_groups, &author_name)
                .unwrap_or_else(|error| {
                    eprintln!("failed to load books for author {author_name}: {error}");
                    Vec::new()
                })
                .into_iter()
                .map(|group| AuthorFolderGroup { folder_path: group.folder_path().as_str().to_owned(), books: group.books().to_vec() })
                .collect::<Vec<_>>();
            groups.sort_by(|a, b| compare_folder_hierarchy(&a.folder_path, &b.folder_path));
            author_groups_for_groups.set(Rc::new(groups));
        });
    }

    dyn_view(move || {
        if !library_loaded.get() {
            return Box::new(inline_message("Library not loaded")) as Box<dyn View>;
        }
        if let Some(author_name) = selected_author.get() {
            let groups = author_groups.get();
            let total_books: usize = groups.iter().map(|group| group.books.len()).sum();

            let sections: Box<dyn View> = if groups.is_empty() {
                Box::new(inline_message("No books found for this author"))
            } else {
                let library_id_for_sections = library_id.clone();
                let signals_for_sections = signals.clone();
                let groups = groups.as_ref().clone();
                Box::new(author_group_stack(groups.into_iter().map(move |group| {
                    let mut books = group.books;
                    books.sort_by(|a, b| a.title().as_str().to_lowercase().cmp(&b.title().as_str().to_lowercase()));
                    card_surface(v_stack((author_group_title(group.folder_path), create_books_grid(books, library_id_for_sections.clone(), signals_for_sections.clone()))))
                })))
            };

            let back_button = secondary_button("Back").on_click(move |_| {
                selected_author.set(None);
                EventPropagation::Continue
            });
            return Box::new(page_surface(v_stack((author_back_with_title(back_button, format!("{author_name} ({total_books})")), author_scroll_content(sections))))) as Box<dyn View>;
        }

        let authors = author_summaries.get();
        if authors.is_empty() {
            return Box::new(inline_message("No authors found")) as Box<dyn View>;
        }

        let visible_count = visible_authors_count.get().min(authors.len());
        let visible_authors = authors.iter().take(visible_count).cloned().collect::<Vec<_>>();
        let cards = author_cards_grid(visible_authors.into_iter().map(move |author| {
            let author_name = author.name.clone();
            let count = author.book_count;
            library_item_card(author_name, format!("({count})")).on_click(move |_| {
                selected_author.set(Some(author.name.clone()));
                EventPropagation::Continue
            })
        }));

        let show_more = visible_count < authors.len();
        let load_more = author_load_more_button(show_more).on_click(move |_| {
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

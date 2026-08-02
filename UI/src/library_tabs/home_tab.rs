use crate::components::{carousel_page_scroll, carousel_progress_label, carousel_row, carousel_section, carousel_stack, inline_message};
use crate::library_tabs::create_book_card;
use crate::page_navigation::Signals;
use epub_svelte_lib::library::Book;
use floem::prelude::{dyn_view, stack_from_iter, SignalGet};
use floem::{IntoView, View};

pub fn home_tab_view(library_id: String, signals: Signals) -> impl View {
    carousel_page_scroll(dyn_view(move || {
        let _refresh_tick = signals.library_refresh_tick.get();
        if signals.library_by_id(&library_id).is_none() {
            return Box::new(inline_message("Library not loaded")) as Box<dyn View>;
        }

        let mut recently_read = signals.app_core.recently_read_in(&library_id, 20).unwrap_or_else(|error| {
            eprintln!("failed to load recently read books: {error}");
            Vec::new()
        });
        let mut in_progress = Vec::new();
        let mut finished = Vec::new();
        for book in recently_read.drain(..) {
            if book.read_progress().value() >= 98.0 {
                finished.push(book);
            } else {
                in_progress.push(book);
            }
        }
        let recently_added = signals.app_core.recently_added_in(&library_id, 20).unwrap_or_else(|error| {
            eprintln!("failed to load recently added books: {error}");
            Vec::new()
        });

        Box::new(create_home_carousels(signals.clone(), library_id.clone(), in_progress, finished, recently_added)) as Box<dyn View>
    }))
}

fn create_home_carousels(signals: Signals, library_id: String, in_progress: Vec<Book>, finished: Vec<Book>, recently_added: Vec<Book>) -> impl View {
    let mut sections: Vec<Box<dyn View>> = Vec::new();
    if !in_progress.is_empty() {
        sections.push(Box::new(create_carousel_section("Recently read", in_progress, library_id.clone(), signals.clone(), true)));
    }
    if !finished.is_empty() {
        sections.push(Box::new(create_carousel_section("Finished", finished, library_id.clone(), signals.clone(), true)));
    }
    if !recently_added.is_empty() {
        sections.push(Box::new(create_carousel_section("Recently added", recently_added, library_id, signals, false)));
    }
    carousel_stack(sections)
}

fn create_carousel_section(title: &'static str, books: Vec<Book>, library_id: String, signals: Signals, show_progress: bool) -> impl View {
    let cards = stack_from_iter(books.into_iter().map(move |book| create_home_book_card(book, library_id.clone(), signals.clone(), show_progress)));

    carousel_section(title, carousel_row(cards)).into_view()
}

fn create_home_book_card(book: Book, library_id: String, signals: Signals, show_progress: bool) -> impl View {
    let progress_text = format!("{:.0}%", book.read_progress().value());
    create_book_card(book, library_id, signals, carousel_progress_label(show_progress, progress_text))
}

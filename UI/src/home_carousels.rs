use epub_svelte_lib::library::Book;
use floem::prelude::{dyn_view, label, stack_from_iter, Decorators, SignalGet};
use floem::{IntoView, View};

use crate::components::library_views::{home_book_card_stack, home_carousel_row, home_carousel_scroll_area, home_carousel_section, home_progress_label};
use crate::library_tabs::folders_tab::create_book_cover;
use crate::page_navigation::Signals;

pub fn library_home_carousels(library_id: String, signals: Signals) -> impl View {
    dyn_view(move || {
        let _refresh_tick = signals.library_refresh_tick.get();
        let Some(state) = signals.library_by_id(&library_id) else {
            return Box::new(label(|| "Library not loaded")) as Box<dyn View>;
        };
        let library = state.handle(signals.library_event_counter.clone());

        let mut recently_read = library.get_recently_read(200);
        let mut in_progress = Vec::new();
        let mut finished = Vec::new();
        for book in recently_read.drain(..) {
            if book.read_progress >= 98.0 {
                finished.push(book);
            } else {
                in_progress.push(book);
            }
        }
        let recently_added = library.get_recently_added(200);

        Box::new(create_home_carousels(signals.clone(), library_id.clone(), in_progress, finished, recently_added)) as Box<dyn View>
    })
    .style(|s| s.width_full().height_full().flex_grow(1.0))
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
    home_carousel_scroll_area(stack_from_iter(sections))
}

fn create_carousel_section(title: &'static str, books: Vec<Book>, library_id: String, signals: Signals, show_progress: bool) -> impl View {
    let cards = stack_from_iter(books.into_iter().map(move |book| create_home_book_card(book, library_id.clone(), signals.clone(), show_progress)));

    home_carousel_section(title, home_carousel_row(cards)).into_view()
}

fn create_home_book_card(book: Book, library_id: String, signals: Signals, show_progress: bool) -> impl View {
    let progress_label = format!("{:.0}%", book.read_progress.clamp(0.0, 100.0));
    let base_card = create_book_cover(book, library_id, signals);

    home_book_card_stack(base_card, home_progress_label(show_progress, progress_label))
}

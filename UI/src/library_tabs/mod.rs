pub mod authors_tab;
pub mod folders_tab;
pub mod home_tab;
pub mod search_tab;
pub mod store_tab;

use crate::components::{book_card, books_grid, books_grid_with_empty_state};
use crate::page_navigation::{Page, ReaderState, Signals};
use epub_svelte_lib::library::Book;
use floem::event::EventPropagation;
use floem::prelude::label;
use floem::View;

/// Shared by the folders, authors and home tabs: a wrapped grid of book cards
/// that navigate to the reader when clicked.
pub fn create_books_grid(books: Vec<Book>, library_id: String, signals: Signals) -> impl View {
    let book_count = books.len();
    let grid = books_grid(books.into_iter().map(move |book| create_book_card(book, library_id.clone(), signals.clone(), label(|| ""))));

    books_grid_with_empty_state(book_count, grid)
}

pub fn create_book_card(book: Book, library_id: String, signals: Signals, footer: impl View + 'static) -> impl View {
    let reader_state = ReaderState { book_id: book.id(), library_id, book_title: book.title().as_str().to_owned() };
    book_card(book, footer, move |_| {
        signals.navigate(Page::Reader(reader_state.clone()));
        EventPropagation::Continue
    })
}

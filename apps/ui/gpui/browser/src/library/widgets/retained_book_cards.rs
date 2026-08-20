use std::collections::HashMap;
use std::rc::Rc;

use app_contract::preferences::LibraryView;
use epub_svelte_lib::library::BookCardRow;
use gpui::{Context, Entity};

use crate::library::services::LibraryContext;
use crate::library::widgets::BookCardView;

/// Retained card entities shared by the grid and carousel layouts.
pub(super) struct RetainedBookCards {
    ui: LibraryContext,
    show_progress: bool,
    library_view: LibraryView,
    cards: Vec<RetainedBookCard>,
    views: Rc<Vec<Entity<BookCardView>>>,
    spanned_views: Rc<Vec<(Entity<BookCardView>, usize)>>,
}

struct RetainedBookCard {
    book: BookCardRow,
    view: Entity<BookCardView>,
}

impl RetainedBookCards {
    pub(super) fn new(ui: LibraryContext, show_progress: bool, library_view: LibraryView) -> Self {
        Self { ui, show_progress, library_view, cards: Vec::new(), views: Rc::new(Vec::new()), spanned_views: Rc::new(Vec::new()) }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub(super) fn views(&self) -> Rc<Vec<Entity<BookCardView>>> {
        self.views.clone()
    }

    pub(super) fn spanned_views(&self) -> Rc<Vec<(Entity<BookCardView>, usize)>> {
        self.spanned_views.clone()
    }

    pub(super) fn library_view(&self) -> LibraryView {
        self.library_view
    }

    pub(super) fn set_library_view<P: 'static>(&mut self, library_view: LibraryView, cx: &mut Context<P>) {
        if self.library_view == library_view {
            return;
        }
        self.library_view = library_view;
        for entry in &self.cards {
            entry.view.update(cx, |view, cx| view.set_library_view(library_view, cx));
        }
        cx.notify();
    }

    pub(super) fn set_books<P: 'static>(&mut self, books: Vec<BookCardRow>, cx: &mut Context<P>) {
        let previous_order = self.cards.iter().map(|entry| entry.book.content_hash.clone()).collect::<Vec<_>>();
        let mut existing = self.cards.drain(..).map(|entry| (entry.book.content_hash.clone(), entry)).collect::<HashMap<_, _>>();
        let mut cards = Vec::with_capacity(books.len());
        let mut titles_changed = false;
        for book in books {
            if let Some(mut entry) = existing.remove(&book.content_hash) {
                if entry.book != book {
                    titles_changed |= entry.book.title != book.title;
                    entry.view.update(cx, |view, cx| view.set_book(book.clone(), self.show_progress, cx));
                    entry.book = book;
                }
                cards.push(entry);
                continue;
            }
            let view = BookCardView::create(book.clone(), self.ui.clone(), self.show_progress, self.library_view, cx);
            cards.push(RetainedBookCard { book, view });
        }
        let order_changed = previous_order != cards.iter().map(|entry| entry.book.content_hash.clone()).collect::<Vec<_>>();
        self.cards = cards;
        if order_changed {
            self.rebuild_views();
        }
        if order_changed || titles_changed {
            cx.notify();
        }
    }

    fn rebuild_views(&mut self) {
        self.views = Rc::new(self.cards.iter().map(|entry| entry.view.clone()).collect());
        self.spanned_views = Rc::new(self.cards.iter().map(|entry| (entry.view.clone(), 1)).collect());
    }
}

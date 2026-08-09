//! Retained book-card collection and its virtualized grid renderer.
//!
//! Each retained card owns its own cover loading.

use std::collections::HashMap;
use std::rc::Rc;

use bokheim_ui_components as components;
use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AnyElement, AppContext, Context, Entity, SharedString, Window, px, size, uniform_list};
use gpui_component::h_virtual_list;

use crate::library::services::LibraryContext;
use crate::library::widgets::book_card::BookCardView;

/// Owns the ordered retained card entities for one grid.
pub(crate) struct BookGrid {
    ui: LibraryContext,
    show_progress: bool,
    cards: Vec<Entity<BookCardView>>,
}

impl BookGrid {
    pub(crate) fn new(ui: LibraryContext, show_progress: bool) -> Self {
        Self { ui, show_progress, cards: Vec::new() }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.cards.len()
    }

    /// Renders a page-sized virtualized grid. Width-to-column conversion lives
    /// here because it is an implementation detail of the one-dimensional
    /// virtual list, not page layout.
    pub(crate) fn render(&self, list_id: impl Into<SharedString>, window: &Window) -> AnyElement {
        let width = (f32::from(window.viewport_size().width) - components::NAV_RAIL_WIDTH - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH);
        self.render_at_width(list_id.into(), width, false)
    }

    /// Embedded grids need their containing panel's usable width, but still
    /// own the column and virtual-row calculation themselves.
    pub(crate) fn render_embedded(&self, list_id: impl Into<SharedString>, width: f32) -> AnyElement {
        self.render_at_width(list_id.into(), width, true)
    }

    pub(crate) fn render_carousel<P: gpui::Render + 'static>(&self, list_id: impl Into<gpui::ElementId>, cx: &mut Context<P>) -> AnyElement {
        let cards = self.cards.clone();
        let item_sizes = Rc::new(vec![size(px(components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP), px(components::BOOK_GRID_ROW_HEIGHT)); cards.len()]);
        h_virtual_list(cx.entity(), list_id, item_sizes, move |_, range, _, _| range.map(|index| cards[index].clone()).collect::<Vec<_>>()).h(px(components::BOOK_GRID_ROW_HEIGHT)).into_any_element()
    }

    fn render_at_width(&self, list_id: SharedString, width: f32, embedded: bool) -> AnyElement {
        let columns = (((width + components::BOOK_GRID_GAP) / (components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP)).floor() as usize).max(1);
        let card_views = self.cards.clone();
        let row_count = card_views.len().div_ceil(columns);
        let list = uniform_list(list_id, row_count, move |range, _, _cx| {
            range
                .map(|row| {
                    let start = row * columns;
                    let end = (start + columns).min(card_views.len());
                    let mut element = components::book_grid_row();
                    for card_view in &card_views[start..end] {
                        element = element.child(card_view.clone());
                    }
                    element
                })
                .collect::<Vec<_>>()
        });

        if embedded { components::embedded_virtual_list(list, row_count as f32 * components::BOOK_GRID_ROW_HEIGHT).into_any_element() } else { components::virtual_list(list).into_any_element() }
    }

    /// Reuses card entities for books that remain in the result set.
    pub(crate) fn set_books<P: 'static>(&mut self, books: Vec<BookCardRow>, cx: &mut Context<P>) {
        let previous_order = self.cards.iter().map(|card| card.read(cx).content_hash()).collect::<Vec<_>>();
        let mut existing = self.cards.drain(..).map(|card| (card.read(cx).content_hash(), card)).collect::<HashMap<_, _>>();
        let mut cards = Vec::with_capacity(books.len());
        for book in books {
            if let Some(view) = existing.remove(&book.content_hash) {
                view.update(cx, |view, cx| view.set_book(book, self.show_progress, cx));
                cards.push(view);
                continue;
            }
            let ui = self.ui.clone();
            let show_progress = self.show_progress;
            cards.push(cx.new(|cx| BookCardView::new(book, ui, show_progress, cx)));
        }
        let order_changed = previous_order != cards.iter().map(|card| card.read(cx).content_hash()).collect::<Vec<_>>();
        self.cards = cards;
        if order_changed {
            cx.notify();
        }
    }
}

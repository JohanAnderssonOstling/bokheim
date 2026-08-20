//! Retained book-card collection with bounded static rendering.
//!
//! Each retained card owns its own cover loading.

use std::rc::Rc;

use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Render, SharedString, Window, div};
use ui_components as components;

use app_contract::preferences::LibraryView;

use crate::library::services::LibraryContext;
use crate::library::widgets::book_card::BookCardView;
use crate::library::widgets::retained_book_cards::RetainedBookCards;
use crate::virtual_grid;

/// Owns the ordered retained card entities for one grid.
pub(crate) struct BookGrid {
    cards: RetainedBookCards,
}

impl BookGrid {
    /// A grid that always uses covers.
    pub(crate) fn new(ui: LibraryContext, show_progress: bool) -> Self {
        Self::with_library_view(ui, show_progress, LibraryView::Covers)
    }

    /// A grid the reader can switch between covers and detailed rows.
    pub(crate) fn browsable(ui: LibraryContext, show_progress: bool, library_view: LibraryView) -> Self {
        Self::with_library_view(ui, show_progress, library_view)
    }

    fn with_library_view(ui: LibraryContext, show_progress: bool, library_view: LibraryView) -> Self {
        Self { cards: RetainedBookCards::new(ui, show_progress, library_view) }
    }

    /// Pushes a changed display preference into every retained card. The cards
    /// are reused rather than rebuilt, so switching presentation does not
    /// re-read a single cover.
    pub(crate) fn set_library_view<P: 'static>(&mut self, library_view: LibraryView, cx: &mut Context<P>) {
        self.cards.set_library_view(library_view, cx);
    }

    pub(crate) fn virtual_views(&self) -> Rc<Vec<Entity<BookCardView>>> {
        self.cards.views()
    }

    pub(crate) fn virtual_spanned_views(&self) -> Rc<Vec<(Entity<BookCardView>, usize)>> {
        self.cards.spanned_views()
    }

    pub(crate) fn uses_detailed_rows(&self) -> bool {
        self.cards.library_view() == LibraryView::Detailed
    }

    /// Renders every retained card into normal document flow. This is for
    /// bounded collections inside an outer scrolling page, where pretending
    /// the full-height grid is virtualized only adds machinery.
    pub(crate) fn render_static<P: Render + 'static>(&self, grid_id: impl Into<SharedString>, window: &mut Window, cx: &mut Context<P>) -> AnyElement {
        let responsive = virtual_grid::ResponsiveColumns::for_element(grid_id.into(), virtual_grid::ColumnPolicy::MinimumItemWidth { width: components::BOOK_COVER_WIDTH, gap: components::BOOK_GRID_GAP }, window, cx);
        let card_views = self.cards.views();
        let (columns, item_width, column_gap) = responsive.geometry(card_views.len(), components::BOOK_COVER_WIDTH, cx);
        let row_height = components::book_grid_row_height_for_cover_width(item_width, f32::from(window.rem_size()));
        let rows = (0..virtual_grid::row_count(card_views.len(), columns)).map(|row| {
            let range = virtual_grid::row_range(row, columns, card_views.len());
            virtual_grid::flexible_entity_row(&card_views, range, columns, item_width, row_height, column_gap)
        });
        responsive.track(div().w_full().flex().flex_col().children(rows)).into_any_element()
    }

    /// Reuses card entities for books that remain in the result set.
    pub(crate) fn set_books<P: 'static>(&mut self, books: Vec<BookCardRow>, cx: &mut Context<P>) {
        self.cards.set_books(books, cx);
    }
}

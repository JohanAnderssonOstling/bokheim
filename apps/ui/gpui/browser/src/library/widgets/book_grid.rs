//! Retained book-card collection and its virtualized grid renderer.
//!
//! Each retained card owns its own cover loading.

use std::collections::HashMap;
use std::rc::Rc;

use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Render, SharedString, Window, container_query, div, px, size, uniform_list};
use gpui_component::{VirtualListScrollHandle, h_virtual_list};
use ui_components as components;

use app_contract::preferences::LibraryView;

use crate::library::services::LibraryContext;
use crate::library::widgets::BookCardView;
use crate::library::widgets::book_card::BookManagement;
use crate::virtual_grid;

/// Owns the ordered retained card entities for one grid.
pub(crate) struct BookGrid {
    ui: LibraryContext,
    show_progress: bool,
    library_view: LibraryView,
    cards: Vec<BookGridEntry>,
    /// Stable view handles reused by every unfiltered render. Rebuilding this
    /// on every resize made virtualization O(total books) before it could
    /// discard offscreen rows.
    all_views: Rc<Vec<Entity<BookCardView>>>,
    management: Option<BookManagement>,
}

struct BookGridEntry {
    book: BookCardRow,
    view: Entity<BookCardView>,
}

impl BookGrid {
    /// A grid that is always covers. Home's carousels scroll horizontally, so a
    /// full-width detailed row has nowhere to go in one.
    pub(crate) fn new(ui: LibraryContext, show_progress: bool) -> Self {
        Self::with_library_view(ui, show_progress, LibraryView::Covers)
    }

    /// A grid the reader can switch between covers and detailed rows.
    pub(crate) fn browsable(ui: LibraryContext, show_progress: bool, library_view: LibraryView) -> Self {
        Self::with_library_view(ui, show_progress, library_view)
    }

    fn with_library_view(ui: LibraryContext, show_progress: bool, library_view: LibraryView) -> Self {
        Self { ui, show_progress, library_view, cards: Vec::new(), all_views: Rc::new(Vec::new()), management: None }
    }

    /// Pushes a changed display preference into every retained card. The cards
    /// are reused rather than rebuilt, so switching presentation does not
    /// re-read a single cover.
    pub(crate) fn set_library_view<P: 'static>(&mut self, library_view: LibraryView, cx: &mut Context<P>) {
        if self.library_view == library_view {
            return;
        }
        self.library_view = library_view;
        for entry in &self.cards {
            entry.view.update(cx, |view, cx| view.set_library_view(library_view, cx));
        }
        cx.notify();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub(crate) fn virtual_views(&self) -> Rc<Vec<Entity<BookCardView>>> {
        self.all_views.clone()
    }

    pub(crate) fn uses_detailed_rows(&self) -> bool {
        self.library_view == LibraryView::Detailed
    }

    /// Reorders retained cards without rebuilding their views or reloading covers.
    pub(crate) fn sort_by<P: 'static>(&mut self, mut compare: impl FnMut(&BookCardRow, &BookCardRow) -> std::cmp::Ordering, cx: &mut Context<P>) {
        let previous_order = self.cards.iter().map(|entry| entry.book.content_hash.clone()).collect::<Vec<_>>();
        self.cards.sort_by(|left, right| compare(&left.book, &right.book));
        if previous_order != self.cards.iter().map(|entry| entry.book.content_hash.clone()).collect::<Vec<_>>() {
            self.all_views = Rc::new(self.cards.iter().map(|entry| entry.view.clone()).collect());
            cx.notify();
        }
    }

    pub(crate) fn set_management<P: 'static>(&mut self, management: Option<BookManagement>, cx: &mut Context<P>) {
        self.management = management.clone();
        for entry in &self.cards {
            entry.view.update(cx, |view, cx| view.set_management(management.clone(), cx));
        }
    }

    /// Renders all retained cards in a parent-sized virtualized grid.
    pub(crate) fn render_virtualized<P: Render + 'static>(&self, list_id: impl Into<SharedString>, window: &mut Window, cx: &mut Context<P>) -> AnyElement {
        let list_id = list_id.into();
        if self.library_view == LibraryView::Detailed {
            return self.render_virtual_rows(list_id);
        }
        let _ = cx;
        let rem_size = f32::from(window.rem_size());
        let card_views = self.all_views.clone();
        let query = container_query(move |size, _, _| {
            let width = f32::from(size.width);
            let capacity = virtual_grid::columns_for_width(width, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP);
            let (columns, item_width) = virtual_grid::grid_geometry(width, capacity, card_views.len(), components::BOOK_COVER_WIDTH, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP);
            let row_height = components::book_grid_row_height_for_cover_width(item_width, rem_size);
            let row_count = virtual_grid::row_count(card_views.len(), columns);
            let cards = card_views.clone();
            let list = uniform_list(list_id.clone(), row_count, move |range, _, _| {
                range
                    .map(|row| {
                        let range = virtual_grid::row_range(row, columns, cards.len());
                        virtual_grid::flexible_entity_row(&cards, range, columns, item_width, row_height, components::BOOK_GRID_GAP)
                    })
                    .collect::<Vec<_>>()
            });
            // A container query lays its result out as a root. `virtual_list`
            // sizes itself with `flex_1`, which only has meaning inside a flex
            // parent; returned bare here, its viewport can collapse to zero.
            // Keep that sizing boundary inside the query so folder pages show
            // the same retained cards that static subject grids already show.
            div().size_full().flex().flex_col().child(components::virtual_list(list))
        });
        // The query fills its parent with relative sizing. Make that parent a
        // flex sizing boundary so the virtual list receives the dashboard's
        // remaining height instead of resolving its Covers viewport to zero.
        div().w_full().flex_1().min_h_0().flex().flex_col().child(query).into_any_element()
    }

    /// Renders every retained card into normal document flow. This is for
    /// bounded collections inside an outer scrolling page, where pretending
    /// the full-height grid is virtualized only adds machinery.
    pub(crate) fn render_static<P: Render + 'static>(&self, grid_id: impl Into<SharedString>, window: &mut Window, cx: &mut Context<P>) -> AnyElement {
        let responsive = virtual_grid::ResponsiveColumns::for_element(grid_id.into(), virtual_grid::ColumnPolicy::MinimumItemWidth { width: components::BOOK_COVER_WIDTH, gap: components::BOOK_GRID_GAP }, window, cx);
        let (columns, item_width) = responsive.geometry(self.all_views.len(), components::BOOK_COVER_WIDTH, cx);
        let row_height = components::book_grid_row_height_for_cover_width(item_width, f32::from(window.rem_size()));
        let card_views = self.all_views.clone();
        let rows = (0..virtual_grid::row_count(card_views.len(), columns)).map(|row| {
            let range = virtual_grid::row_range(row, columns, card_views.len());
            virtual_grid::flexible_entity_row(&card_views, range, columns, item_width, row_height, components::BOOK_GRID_GAP)
        });
        responsive.track(div().w_full().flex().flex_col().children(rows)).into_any_element()
    }

    /// The horizontal row. `scroll` is the caller's, so the arrows beside the
    /// section title can step the same list the pointer scrolls.
    pub(crate) fn render_carousel<P: gpui::Render + 'static>(&self, list_id: impl Into<gpui::ElementId>, scroll: &VirtualListScrollHandle, window: &Window, cx: &mut Context<P>) -> AnyElement {
        let cards = self.all_views.clone();
        let row_height = components::book_grid_row_height(f32::from(window.rem_size()));
        let item_sizes = Rc::new(vec![size(px(components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP), px(row_height)); cards.len()]);
        h_virtual_list(cx.entity(), list_id, item_sizes, move |_, range, _, _| range.map(|index| div().w(px(components::BOOK_COVER_WIDTH)).flex_none().child(cards[index].clone())).collect::<Vec<_>>())
            .track_scroll(scroll)
            .h(px(row_height))
            .into_any_element()
    }

    /// One card per row, so the virtual list indexes cards directly and the
    /// fixed row height the components declare is what keeps it valid.
    fn render_virtual_rows(&self, list_id: SharedString) -> AnyElement {
        let card_views = self.all_views.clone();
        let list = uniform_list(list_id, card_views.len(), move |range, _, _cx| range.map(|index| card_views[index].clone().into_any_element()).collect::<Vec<_>>());
        components::virtual_list(list).into_any_element()
    }

    /// Reuses card entities for books that remain in the result set.
    pub(crate) fn set_books<P: 'static>(&mut self, books: Vec<BookCardRow>, cx: &mut Context<P>) {
        let previous_order = self.cards.iter().map(|entry| entry.book.content_hash.clone()).collect::<Vec<_>>();
        let mut existing = self.cards.drain(..).map(|entry| (entry.book.content_hash.clone(), entry)).collect::<HashMap<_, _>>();
        let mut cards = Vec::with_capacity(books.len());
        let mut titles_changed = false;
        for book in books {
            if let Some(mut entry) = existing.remove(&book.content_hash) {
                // The common refresh path changes nothing. Keep the retained
                // entity entirely untouched instead of scheduling an update
                // closure for every book in a large folder.
                if entry.book != book {
                    titles_changed |= entry.book.title != book.title;
                    entry.view.update(cx, |view, cx| view.set_book(book.clone(), self.show_progress, cx));
                    entry.book = book;
                }
                cards.push(entry);
                continue;
            }
            let ui = self.ui.clone();
            let show_progress = self.show_progress;
            let view = BookCardView::create(book.clone(), ui, show_progress, self.library_view, cx);
            view.update(cx, |view, cx| view.set_management(self.management.clone(), cx));
            cards.push(BookGridEntry { book, view });
        }
        let order_changed = previous_order != cards.iter().map(|entry| entry.book.content_hash.clone()).collect::<Vec<_>>();
        self.cards = cards;
        if order_changed {
            self.all_views = Rc::new(self.cards.iter().map(|entry| entry.view.clone()).collect());
        }
        if order_changed || titles_changed {
            cx.notify();
        }
    }
}

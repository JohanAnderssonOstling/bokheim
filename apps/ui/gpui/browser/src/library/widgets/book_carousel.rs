//! A retained horizontal row of book covers with its own scroll controls.

use std::rc::Rc;

use app_contract::preferences::LibraryView;
use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Render, SharedString, Window, div, px, size};
use gpui_component::{IconName, VirtualListScrollHandle, h_virtual_list};
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::widgets::retained_book_cards::RetainedBookCards;

pub(crate) struct BookCarousel {
    title: SharedString,
    cards: RetainedBookCards,
    scroll: VirtualListScrollHandle,
}

impl BookCarousel {
    pub(crate) fn new(title: impl Into<SharedString>, ui: LibraryContext, show_progress: bool) -> Self {
        Self { title: title.into(), cards: RetainedBookCards::new(ui, show_progress, LibraryView::Covers), scroll: VirtualListScrollHandle::new() }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub(crate) fn set_books<P: 'static>(&mut self, books: Vec<BookCardRow>, cx: &mut Context<P>) {
        self.cards.set_books(books, cx);
    }

    pub(crate) fn render<P: Render + 'static>(&self, index: usize, window: &Window, cx: &mut Context<P>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let source = format!("home-carousel-{index}");
        let heading = components::carousel_heading().child(components::carousel_title(self.title.clone())).child(self.controls(index, theme));
        let cards = self.cards.views();
        let row_height = components::book_grid_row_height(f32::from(window.rem_size()));
        let item_sizes = Rc::new(vec![size(px(components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP), px(row_height)); cards.len()]);
        let body = h_virtual_list(cx.entity(), source, item_sizes, move |_, range, _, _| range.map(|index| div().w(px(components::BOOK_COVER_WIDTH)).flex_none().child(cards[index].clone())).collect::<Vec<_>>())
            .track_scroll(&self.scroll)
            .h(px(row_height));
        components::carousel_section().child(heading).child(body).into_any_element()
    }

    fn controls(&self, index: usize, theme: components::BrowserTheme) -> impl IntoElement {
        let step = |scroll: VirtualListScrollHandle, direction: f32| {
            move |_: &gpui::ClickEvent, window: &mut Window, _: &mut gpui::App| {
                components::step_carousel(scroll.base_handle(), direction, components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP, window);
            }
        };
        components::carousel_controls()
            .child(components::carousel_arrow_button(format!("home-carousel-{index}-previous"), "Previous", IconName::ChevronLeft, theme).on_click(step(self.scroll.clone(), -1.0)))
            .child(components::carousel_arrow_button(format!("home-carousel-{index}-next"), "Next", IconName::ChevronRight, theme).on_click(step(self.scroll.clone(), 1.0)))
    }
}

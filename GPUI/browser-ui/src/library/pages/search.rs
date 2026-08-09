//! Library search.
//!
//! Owns its input and performs the cheap local SQLite search immediately.

use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{Context, Entity, Focusable, IntoElement, Render, SharedString, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::library::services::LibraryContext;
use crate::library::widgets::BookGrid;

pub(crate) struct SearchPage {
    ui: LibraryContext,
    input: Entity<InputState>,
    books: BookGrid,
    error: Option<SharedString>,
    _input_subscription: Subscription,
}

impl SearchPage {
    pub(crate) fn new(ui: LibraryContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let books = BookGrid::new(ui.clone(), false);
        let input_subscription = cx.subscribe(&input, |page: &mut Self, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = input.read(cx).value().to_string();
                page.search(query, cx);
            }
        });
        Self { ui, input, books, error: None, _input_subscription: input_subscription }
    }

    pub(crate) fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        self.search(query, cx);
    }

    fn search(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_owned();
        self.error = None;
        if query.is_empty() {
            self.install(Vec::new(), cx);
            return;
        }
        let limit = usize::from(self.ui.contracts().browsing_content.search_result_limit);
        match self.ui.library.search(&query, limit) {
            Ok(books) => self.install(books, cx),
            Err(error) => {
                log::error!("failed to search books: {error}");
                self.error = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn install(&mut self, books: Vec<epub_svelte_lib::library::BookCardRow>, cx: &mut Context<Self>) {
        self.books.set_books(books, cx);
        self.error = None;
        cx.notify();
    }
}

impl Render for SearchPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let mut page = components::browser_search_page().child(components::browser_search_box().child(Input::new(&self.input)));

        if !self.input.read(cx).value().trim().is_empty() {
            let body = if self.books.is_empty() { components::browser_inline_message(self.ui.contracts().browsing_states.no_search_results_label.clone(), theme).into_any_element() } else { self.books.render("search-results", window) };
            page = page.child(body);
        }

        let entity = cx.entity();
        let retry_label = self.ui.contracts().browsing_states.retry_label.clone();
        components::browser_error_banner(self.error.clone(), retry_label.into(), theme, page.into_any_element(), move |cx| {
            entity.update(cx, |page, cx| page.refresh(cx));
        })
    }
}

//! Author index on the left, the selected author's books on the right.
//!
//! The eager catalogue-availability sweep that used to pre-warm every author,
//! and the two filter toggles that depended on it, are not carried over. Both
//! can return once the structure settles.

mod discovery;

use bokheim_ui_components as components;
use bokheim_ui_components::ScrollableElement;
use epub_svelte_lib::AuthorId;
use epub_svelte_lib::library::AuthorSummary;
use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Subscription, Window, uniform_list};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::library::services::{AuthorBrowseRows, LibraryContext};
use crate::library::widgets::BookGrid;
use crate::stores::heading_font;
use crate::{ActivateAuthor, FocusNextListItem, FocusPreviousListItem};

use self::discovery::AuthorDiscoveryView;

pub(crate) struct AuthorsPage {
    ui: LibraryContext,
    filter_input: Entity<InputState>,
    authors: Vec<AuthorSummary>,
    selected: Option<AuthorId>,
    books: BookGrid,
    error: Option<SharedString>,
    discovery: Entity<AuthorDiscoveryView>,
    _input_subscription: Subscription,
}

impl AuthorsPage {
    pub(crate) fn new(ui: LibraryContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx));
        let books = BookGrid::new(ui.clone(), false);
        let discovery = cx.new(|_| AuthorDiscoveryView::new(ui.clone()));
        let input_subscription = cx.subscribe(&filter_input, |_: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self { ui, filter_input, authors: Vec::new(), selected: None, books, error: None, discovery, _input_subscription: input_subscription }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected;
        match self.ui.library.authors(selected) {
            Ok(AuthorBrowseRows { authors, books }) => {
                self.error = None;
                self.authors = authors;
                if let Some(books) = books {
                    self.books.set_books(books, cx);
                }
                cx.notify();
            }
            Err(error) => {
                log::error!("failed to load Authors section: {error}");
                self.error = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn select_author(&mut self, author_id: AuthorId, cx: &mut Context<Self>) {
        if self.selected == Some(author_id) {
            return;
        }
        self.selected = Some(author_id);
        match self.ui.library.author_books(author_id) {
            Ok(books) => {
                self.error = None;
                self.books.set_books(books, cx);
                cx.notify();
                self.discovery.update(cx, |view, cx| view.set_author(author_id, cx));
            }
            Err(error) => {
                log::error!("failed to load author books: {error}");
                self.error = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn choose_book_to_add(&mut self, cx: &mut Context<Self>) {
        self.ui.clone().choose_and_import_book("__root__".to_owned(), cx, |page, result, cx| match result {
            Ok(()) => page.refresh(cx),
            Err(error) => {
                page.error = Some(format!("Could not add book: {error}").into());
                cx.notify();
            }
        });
    }

    fn render_index(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let heading_font = heading_font(cx);
        let filter = self.filter_input.read(cx).value().trim().to_lowercase();
        let authors = self.authors.iter().filter(|author| filter.is_empty() || author.name().as_str().to_lowercase().contains(&filter)).cloned().collect::<Vec<_>>();
        let has_authors = !authors.is_empty();
        let selected = self.selected;
        let page_width = (f32::from(window.viewport_size().width) - components::NAV_RAIL_WIDTH).min(components::AUTHOR_PAGE_MAX_WIDTH);

        let row_font = heading_font.clone();
        let emitter = cx.entity();
        let list = components::virtual_list(uniform_list("browser-authors", authors.len(), move |range, _, _| {
            range
                .map(|index| {
                    let author = &authors[index];
                    let author_id = author.id();
                    let name = author.name().as_str().to_owned();
                    let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
                    let activate = emitter.clone();
                    let click = emitter.clone();
                    components::author_list_row(format!("author-{author_id}"), selected == Some(author_id), theme)
                        .tab_index(index as isize)
                        .key_context("BrowserAuthorList")
                        .child(components::author_avatar(initial, row_font.clone(), theme))
                        .child(components::author_list_identity().child(components::author_list_name(name, row_font.clone())).child(components::author_book_count(author.book_count(), theme)))
                        .child(components::author_list_chevron(theme))
                        .on_action(move |_: &ActivateAuthor, _, cx| activate.update(cx, |page, cx| page.select_author(author_id, cx)))
                        .on_click(move |_, _, cx| click.update(cx, |page, cx| page.select_author(author_id, cx)))
                })
                .collect::<Vec<_>>()
        }));

        let empty_emitter = cx.entity();
        let empty_action = components::topbar_action_button("authors-add-book", self.ui.contracts().library_actions.add_book_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
            empty_emitter.update(cx, |page, cx| page.choose_book_to_add(cx));
        });

        components::author_list_panel((page_width / 3.0).max(240.0), theme)
            .key_context("BrowserArrowList")
            .on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx))
            .on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx))
            .child(components::author_filter_box().child(Input::new(&self.filter_input)))
            .when(self.authors.is_empty(), |panel| panel.child(components::browser_actionable_empty("No authors", "", empty_action, heading_font, theme)))
            .when(!self.authors.is_empty() && !has_authors, |panel| panel.child(components::browser_inline_message("No matching authors", theme)))
            .when(has_authors, |panel| panel.child(list))
            .into_any_element()
    }

    fn render_books(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let heading_font = heading_font(cx);
        let page_width = (f32::from(window.viewport_size().width) - components::NAV_RAIL_WIDTH).min(components::AUTHOR_PAGE_MAX_WIDTH);
        let width = (page_width * 2.0 / 3.0 - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH);

        let Some(author) = self.selected.and_then(|selected| self.authors.iter().find(|author| author.id() == selected)).map(|author| SharedString::from(author.name().as_str().to_owned())) else {
            return components::author_books_panel().child(components::author_selection_hint("Select an author to view their books", theme)).overflow_y_scrollbar().into_any_element();
        };

        let mut panel = components::author_books_panel().child(components::author_books_heading(author, self.books.len(), heading_font.clone(), theme));
        if self.books.is_empty() {
            panel = panel.child(components::browser_inline_message("No books found for this author", theme));
        } else {
            panel = panel.child(self.books.render_embedded("author-books", width));
        }

        panel.child(self.discovery.clone()).overflow_y_scrollbar().into_any_element()
    }
}

impl Render for AuthorsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let split = components::authors_split_page().child(self.render_index(window, cx)).child(self.render_books(window, cx));
        let entity = cx.entity();
        let retry_label = self.ui.contracts().browsing_states.retry_label.clone();
        components::browser_error_banner(self.error.clone(), retry_label.into(), theme, split.into_any_element(), move |cx| {
            entity.update(cx, |page, cx| page.refresh(cx));
        })
    }
}

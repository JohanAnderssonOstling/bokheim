//! Author index on the left, the selected author's books on the right.
//!
//! The eager catalogue-availability sweep that used to pre-warm every author,
//! and the two filter toggles that depended on it, are not carried over. Both
//! can return once the structure settles.

mod discovery;

use std::rc::Rc;

use epub_svelte_lib::AuthorId;
use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Subscription, Window, uniform_list};
use gpui_component::input::{InputEvent, InputState};
use ui_components as components;
use ui_components::ScrollableElement;

use epub_svelte_lib::library::AuthorSort;

use crate::library::services::{AuthorBrowseRows, LatestRequest, LibraryContext};
use crate::library::title_filter::normalize_filter;
use crate::library::widgets::{AuthorRowView, BookGrid};
use crate::{FocusNextListItem, FocusPreviousListItem};

use self::discovery::AuthorDiscoveryView;

pub(crate) struct AuthorsPage {
    ui: LibraryContext,
    filter_input: Entity<InputState>,
    /// How the index is ordered. Filing order groups under letters; book count
    /// does not, so the dividers go with it.
    sort: AuthorSort,
    /// Every author's retained row, in filing order. Rows survive filtering so
    /// that clearing a filter costs no thumbnail reads.
    rows: Vec<Entity<AuthorRowView>>,
    /// The rows the current filter leaves.
    visible: Vec<Entity<AuthorRowView>>,
    selected: Option<AuthorId>,
    books: BookGrid,
    error: Option<SharedString>,
    discovery: Entity<AuthorDiscoveryView>,
    refresh_request: LatestRequest,
    books_request: LatestRequest,
    _input_subscription: Subscription,
}

impl AuthorsPage {
    pub(crate) fn new(ui: LibraryContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx));
        let books = BookGrid::browsable(ui.clone(), false, ui.library_view());
        let discovery = cx.new(|_| AuthorDiscoveryView::new(ui.clone()));
        let input_subscription = cx.subscribe(&filter_input, |page: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                page.rebuild_visible(cx);
                cx.notify();
            }
        });
        Self {
            ui,
            filter_input,
            sort: AuthorSort::default(),
            rows: Vec::new(),
            visible: Vec::new(),
            selected: None,
            books,
            error: None,
            discovery,
            refresh_request: LatestRequest::default(),
            books_request: LatestRequest::default(),
            _input_subscription: input_subscription,
        }
    }

    /// Rebuilds the retained rows from a fresh author list. Rows are recreated
    /// rather than reconciled: the index changes only when the library does.
    fn install_authors(&mut self, authors: Vec<epub_svelte_lib::library::AuthorSummary>, cx: &mut Context<Self>) {
        let selected = self.selected;
        let page = cx.entity();
        let select: crate::library::widgets::SelectAuthor = Rc::new(move |author_id, cx| {
            page.update(cx, |page, cx| page.select_author(author_id, cx));
        });
        self.rows = authors
            .into_iter()
            .enumerate()
            .map(|(index, author)| {
                let is_selected = selected == Some(author.id());
                let row = AuthorRowView::create(author, self.ui.clone(), index as isize, select.clone(), cx);
                if is_selected {
                    row.update(cx, |row, cx| row.set_selected(true, cx));
                }
                row
            })
            .collect();
        self.rebuild_visible(cx);
    }

    /// Applies the filter. Tab order follows what is on screen, so it is
    /// assigned here rather than fixed when the rows were built.
    fn rebuild_visible(&mut self, cx: &mut Context<Self>) {
        let filter = normalize_filter(self.filter_input.read(cx).value().as_ref());
        let mut visible = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let matches = {
                let author = row.read(cx).author();
                // Either form matches: the name as printed, or the one the
                // index files under.
                filter.is_empty() || author.name().as_str().to_lowercase().contains(&filter) || author.sort_name().to_lowercase().contains(&filter)
            };
            if !matches {
                continue;
            }
            let position = visible.len() as isize;
            row.update(cx, |row, cx| row.set_tab_index(position, cx));
            visible.push(row.clone());
        }
        self.visible = visible;
    }

    /// Re-asks the backend for the index in the chosen order. The sort lives
    /// there, so the two frontends cannot disagree about ties.
    fn set_sort(&mut self, sort: AuthorSort, cx: &mut Context<Self>) {
        if self.sort == sort {
            return;
        }
        self.sort = sort;
        self.refresh(cx);
        cx.notify();
    }

    pub(crate) fn set_library_view(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<Self>) {
        self.books.set_library_view(library_view, cx);
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected;
        self.books_request.begin();
        let generation = self.refresh_request.begin();
        let load = self.ui.load_authors(selected, self.sort);
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.refresh_request.is_current(generation) || page.selected != selected {
                    return;
                }
                match result {
                    Ok(AuthorBrowseRows { authors, books }) => {
                        page.error = None;
                        page.install_authors(authors, cx);
                        if let Some(books) = books {
                            page.books.set_books(books, cx);
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        log::error!("failed to load Authors section: {error}");
                        page.error = Some(error.into());
                        cx.notify();
                    }
                }
            });
        });
        self.refresh_request.install(task);
    }

    fn select_author(&mut self, author_id: AuthorId, cx: &mut Context<Self>) {
        if self.selected == Some(author_id) {
            return;
        }
        self.selected = Some(author_id);
        for row in &self.rows {
            let is_selected = row.read(cx).author().id() == author_id;
            row.update(cx, |row, cx| row.set_selected(is_selected, cx));
        }
        self.refresh_request.begin();
        let generation = self.books_request.begin();
        let load = self.ui.load_author_books(author_id);
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let _ = page.update(cx, |page, cx| {
                if !page.books_request.is_current(generation) || page.selected != Some(author_id) {
                    return;
                }
                match result {
                    Ok(books) => {
                        page.error = None;
                        page.books.set_books(books, cx);
                        cx.notify();
                        page.discovery.update(cx, |view, cx| view.set_author(author_id, cx));
                    }
                    Err(error) => {
                        log::error!("failed to load author books: {error}");
                        page.error = Some(error.into());
                        cx.notify();
                    }
                }
            });
        });
        self.books_request.install(task);
    }

    fn choose_book_to_add(&mut self, cx: &mut Context<Self>) {
        self.ui.clone().choose_and_import_book(epub_svelte_lib::ROOT_DIR_ID, cx, |page, result, cx| match result {
            Ok(()) => page.refresh(cx),
            Err(error) => {
                page.error = Some(format!("Could not add book: {error}").into());
                cx.notify();
            }
        });
    }

    fn render_index(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let has_authors = !self.visible.is_empty();
        let page_width = (f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size()))).min(components::AUTHOR_PAGE_MAX_WIDTH);

        // The browser's own virtual list: it fills the panel, carries the
        // scrollbar, and pages by the viewport on left/right, like every other
        // list here.
        let rows = self.visible.clone();
        let list = components::virtual_list(uniform_list("browser-authors", rows.len(), move |range, _, _| range.map(|index| rows[index].clone()).collect::<Vec<_>>()));

        let empty_emitter = cx.entity();
        let empty_action = components::topbar_action_button("authors-add-book", self.ui.contracts().library_actions.add_book_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
            empty_emitter.update(cx, |page, cx| page.choose_book_to_add(cx));
        });

        components::author_list_panel((page_width / 3.0).max(240.0), theme)
            .key_context("BrowserArrowList")
            .on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx))
            .on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx))
            .child(components::author_filter_box().child(components::browser_search_input(&self.filter_input)))
            .child(self.render_sort_control(theme, cx))
            .when(self.rows.is_empty(), |panel| panel.child(components::browser_actionable_empty("No authors", "", empty_action, theme)))
            .when(!self.rows.is_empty() && !has_authors, |panel| panel.child(components::browser_inline_note("No matching authors", theme)))
            .when(has_authors, |panel| panel.child(list))
            .into_any_element()
    }

    /// The two orders the contract defines, as a pair of chips.
    fn render_sort_control(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let choose = |sort: AuthorSort, page: Entity<Self>| move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| page.update(cx, |page, cx| page.set_sort(sort, cx));
        components::author_sort_control()
            .child(components::author_sort_option("authors-sort-name", "A–Z", self.sort == AuthorSort::Name, theme).on_click(choose(AuthorSort::Name, cx.entity())))
            .child(components::author_sort_option("authors-sort-books", "Books", self.sort == AuthorSort::BookCount, theme).on_click(choose(AuthorSort::BookCount, cx.entity())))
    }

    fn render_books(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let page_width = (f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size()))).min(components::AUTHOR_PAGE_MAX_WIDTH);
        let width = (page_width * 2.0 / 3.0 - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH);

        let Some(author) = self.selected.and_then(|selected| self.rows.iter().find(|row| row.read(cx).author().id() == selected)).map(|row| SharedString::from(row.read(cx).author().name().as_str().to_owned())) else {
            // Nothing chosen yet: an empty pane, not a sentence explaining that
            // the list beside it can be clicked.
            return components::author_books_panel().overflow_y_scrollbar().into_any_element();
        };

        let mut panel = components::author_books_panel().child(components::author_books_heading(author, theme));
        if self.books.is_empty() {
            panel = panel.child(components::browser_inline_note("No books found for this author", theme));
        } else {
            // Still the static grid: the panel scrolls as one column so that
            // the catalogue disclosure sits under the books rather than in a
            // second scroll area, and a virtualized list cannot size itself
            // inside a parent that grows to its content. Virtualizing this
            // wants the panel split into its own scroll regions first.
            panel = panel.child(self.books.render_static(width, window));
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

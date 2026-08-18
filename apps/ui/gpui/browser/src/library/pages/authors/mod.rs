//! Author index on the left, the selected author's books on the right.
//!
//! The eager catalogue-availability sweep that used to pre-warm every author,
//! and the two filter toggles that depended on it, are not carried over. Both
//! can return once the structure settles.

mod discovery;

use std::rc::Rc;

use epub_svelte_lib::AuthorId;
use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Subscription, Window, container_query, div, px, size, uniform_list};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::v_virtual_list;
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
    /// Changes whenever the filtered index is rebuilt, giving a replacement
    /// virtual list fresh scroll state instead of retaining an out-of-range
    /// offset from a previous query.
    visible_generation: usize,
    selected: Option<AuthorId>,
    books: BookGrid,
    error: Option<SharedString>,
    discovery: Entity<AuthorDiscoveryView>,
    refresh_request: LatestRequest,
    books_request: LatestRequest,
    _input_subscription: Subscription,
    _discovery_subscription: Subscription,
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
        let discovery_subscription = cx.observe(&discovery, |_, _, cx| cx.notify());
        Self {
            ui,
            filter_input,
            sort: AuthorSort::default(),
            rows: Vec::new(),
            visible: Vec::new(),
            visible_generation: 0,
            selected: None,
            books,
            error: None,
            discovery,
            refresh_request: LatestRequest::default(),
            books_request: LatestRequest::default(),
            _input_subscription: input_subscription,
            _discovery_subscription: discovery_subscription,
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
        self.visible_generation = self.visible_generation.wrapping_add(1);
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

    fn clear_author(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.books_request.begin();
        self.books.set_books(Vec::new(), cx);
        for row in &self.rows {
            row.update(cx, |row, cx| row.set_selected(false, cx));
        }
        cx.notify();
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
        let visible_cover_limit = if f32::from(window.viewport_size().width) <= 480.0 {
            1
        } else if f32::from(window.viewport_size().width) <= 700.0 {
            2
        } else {
            components::AUTHOR_ROW_COVER_COUNT
        };
        let cards = Rc::new(
            self.visible
                .iter()
                .map(|row| {
                    let cover_count = row.read(cx).author().cover_hashes().len().min(visible_cover_limit);
                    (row.clone(), components::author_card_width(cover_count))
                })
                .collect::<Vec<_>>(),
        );
        let generation = self.visible_generation;
        let cards_view = container_query(move |size, _, _| {
            let available_width = f32::from(size.width);
            let mut packed = Vec::<Vec<Entity<AuthorRowView>>>::new();
            let mut current = Vec::new();
            let mut occupied = 0.0;
            for (card, card_width) in cards.iter() {
                let required = if current.is_empty() { *card_width } else { components::BOOK_GRID_GAP + *card_width };
                if !current.is_empty() && occupied + required > available_width {
                    packed.push(std::mem::take(&mut current));
                    occupied = 0.0;
                }
                occupied += if current.is_empty() { *card_width } else { components::BOOK_GRID_GAP + *card_width };
                current.push(card.clone());
            }
            if !current.is_empty() {
                packed.push(current);
            }
            let packed = Rc::new(packed);
            let row_count = packed.len();
            let rows = packed.clone();
            let list = uniform_list(format!("authors-index-virtual-{generation}"), row_count, move |range, _, _| range.map(|index| components::author_virtual_row().children(rows[index].clone())).collect::<Vec<_>>());
            div().size_full().flex().flex_col().child(components::virtual_list(list))
        });

        let empty_emitter = cx.entity();
        let empty_action = components::topbar_action_button("authors-add-book", self.ui.contracts().library_actions.add_book_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
            empty_emitter.update(cx, |page, cx| page.choose_book_to_add(cx));
        });

        components::author_list_panel(components::AUTHOR_PAGE_MAX_WIDTH, theme)
            .key_context("BrowserArrowList")
            .on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx))
            .on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx))
            .child(components::author_filter_box().child(components::browser_search_input(&self.filter_input)))
            .when(self.rows.is_empty(), |panel| panel.child(components::browser_actionable_empty("No authors", "", empty_action, theme)))
            .when(!self.rows.is_empty() && !has_authors, |panel| panel.child(components::browser_inline_note("No matching authors", theme)))
            .when(has_authors, |panel| panel.child(div().w_full().min_h_0().flex_1().flex().flex_col().child(cards_view)))
            .into_any_element()
    }

    fn render_books(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);

        let Some(author) = self.selected.and_then(|selected| self.rows.iter().find(|row| row.read(cx).author().id() == selected)).map(|row| SharedString::from(row.read(cx).author().name().as_str().to_owned())) else {
            // Nothing chosen yet: an empty pane, not a sentence explaining that
            // the list beside it can be clicked.
            return components::author_books_panel().overflow_y_scrollbar().into_any_element();
        };

        let page = cx.entity();
        let back = components::topbar_action_button("authors-back", "Authors", gpui_component::IconName::ArrowLeft, theme).on_click(move |_, _, cx| {
            page.update(cx, |page, cx| page.clear_author(cx));
        });
        let heading = div().flex_none().flex().flex_row().items_center().gap(px(components::SPACE_MD)).child(back).child(components::author_books_heading(author, theme));
        let book_views = self.books.virtual_views();
        let detailed = self.books.uses_detailed_rows();
        let discovery = self.discovery.clone();
        let page = cx.entity();
        let rem_size = f32::from(window.rem_size());
        let contract = &self.ui.contracts().store;
        let publication_unit_width = crate::catalog::catalog_grid_unit_width(contract.publication_min_width_px);
        let publication_height = contract.publication_height_px as f32;
        let combined = container_query(move |available, _, cx| {
            let width = f32::from(available.width);
            let book_capacity = if detailed { 1 } else { crate::virtual_grid::columns_for_width(width, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP) };
            let (book_columns, book_width) =
                if detailed { (1, width) } else { crate::virtual_grid::grid_geometry(width, book_capacity, book_views.len(), components::BOOK_COVER_WIDTH, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP) };
            let book_row_height = if detailed { components::DETAILED_ROW_HEIGHT } else { components::book_grid_row_height_for_cover_width(book_width, rem_size) };
            let book_row_count = crate::virtual_grid::row_count(book_views.len(), book_columns);
            let local_row_count = book_row_count.max(1);
            let discovery_columns = crate::virtual_grid::columns_for_width(width, publication_unit_width, 16.0);
            let discovery_rows = Rc::new(discovery.read(cx).virtual_rows(discovery_columns));

            let mut item_sizes = Vec::with_capacity(local_row_count + discovery_rows.len());
            item_sizes.extend((0..local_row_count).map(|row| {
                let height = if book_views.is_empty() { 66.0 } else { book_row_height };
                let trailing_gap = if row + 1 == local_row_count { components::SPACE_LG } else { 0.0 };
                size(px(width), px(height + trailing_gap))
            }));
            item_sizes.extend(discovery_rows.iter().enumerate().map(|(index, row)| {
                let trailing_gap = if index + 1 == discovery_rows.len() { 0.0 } else { components::SPACE_MD };
                size(px(width), px(row.content_height(publication_height) + trailing_gap))
            }));

            let render_books = book_views.clone();
            let render_discovery = discovery.clone();
            let render_rows = discovery_rows.clone();
            let list = v_virtual_list(page.clone(), "author-books-and-discovery", Rc::new(item_sizes), move |_, range, _, _| {
                range
                    .map(|index| {
                        if index < local_row_count {
                            let trailing_gap = if index + 1 == local_row_count { components::SPACE_LG } else { 0.0 };
                            if render_books.is_empty() {
                                div().w_full().h(px(66.0 + trailing_gap)).flex_none().child(components::browser_inline_note("No books found for this author", theme)).into_any_element()
                            } else if detailed {
                                div().w_full().h(px(book_row_height + trailing_gap)).flex_none().child(render_books[index].clone()).into_any_element()
                            } else {
                                let book_range = crate::virtual_grid::row_range(index, book_columns, render_books.len());
                                div()
                                    .w_full()
                                    .h(px(book_row_height + trailing_gap))
                                    .flex_none()
                                    .child(crate::virtual_grid::flexible_entity_row(&render_books, book_range, book_columns, book_width, book_row_height, components::BOOK_GRID_GAP))
                                    .into_any_element()
                            }
                        } else {
                            let discovery_index = index - local_row_count;
                            let trailing_gap = if discovery_index + 1 == render_rows.len() { 0.0 } else { components::SPACE_MD };
                            AuthorDiscoveryView::render_virtual_row(&render_rows[discovery_index], discovery_index, render_discovery.clone(), discovery_columns, publication_unit_width, publication_height, trailing_gap, theme)
                        }
                    })
                    .collect::<Vec<_>>()
            });
            div().size_full().min_h_0().flex().flex_col().child(components::sized_virtual_list(list).flex_1().min_h_0())
        });

        components::author_books_panel().child(heading).child(div().w_full().flex_1().min_h_0().flex().flex_col().child(combined)).into_any_element()
    }
}

impl Render for AuthorsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let page = if self.selected.is_some() { self.render_books(window, cx) } else { self.render_index(window, cx) };
        let split = components::authors_split_page().child(page);
        let entity = cx.entity();
        let retry_label = self.ui.contracts().browsing_states.retry_label.clone();
        components::browser_error_banner(self.error.clone(), retry_label.into(), theme, split.into_any_element(), move |cx| {
            entity.update(cx, |page, cx| page.refresh(cx));
        })
    }
}

//! One page of shelves, drawn twice: Home and the author index.
//!
//! Both are the same object — a vertical paginator of full-width rows, each a
//! heading over a horizontal shelf of book cards — and they differed only in
//! where the rows came from and what a heading says. That difference is a
//! [`ShelfSource`]; everything else (loading, reuse across reloads, the keyboard
//! cursor, paging, the error banner) lives here once, the way folder and subject
//! browsing share one page behind an adapter.
//!
//! The vertical axis is paged rather than scrolled. A paginator turns the wheel
//! into a page it animates, so the page always shows whole rows instead of
//! stopping halfway down a cover.

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;

use library_backend::LibraryClient;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, SharedString, Subscription, Window, div};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{Disableable, IconName};
use library_model::BookCardRow;
use ui_components as components;

use crate::library::pages::browse::controls::SearchField;
use crate::library::pages::library_empty_state;
use crate::library::services::{LatestRequest, LibraryContentsChanged, LibraryContext, LibraryScanChanged};
use crate::library::widgets::{BookCardView, CursorMove, Paginator, PaginatorAxis, PaginatorChild, PaginatorGroup, shelf_card_policy, shelf_rows_policy};

pub(crate) type ShelfFuture = Pin<Box<dyn Future<Output = Result<Vec<ShelfRow>, String>>>>;

/// One row of the page, as its source describes it.
#[derive(Clone)]
pub(crate) struct ShelfRow {
    /// Stable identity across reloads. A row that comes back under the same id
    /// keeps its shelf — and with it the page the reader had turned to.
    pub(crate) id: SharedString,
    pub(crate) title: SharedString,
    /// What the shelf is about, drawn as chips beside the title. Home's sections
    /// are about nothing in particular and leave this empty.
    /// Each chip with the parent it was reached through, where it has one.
    pub(crate) subjects: Vec<(Option<SharedString>, SharedString)>,
    pub(crate) books: Vec<BookCardRow>,
}

/// An order a shelf page can be read in. Only sources that offer a choice
/// declare any; the rest load in whatever order they already had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShelfSort {
    Name,
    BookCount,
}

impl ShelfSort {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::BookCount => "Most books",
        }
    }
}

/// Where a page's rows come from, and what its chrome says about them.
pub(crate) trait ShelfSource: 'static {
    /// Prefix for the page's element ids.
    const ID: &'static str;
    /// How many rows of subject chips a heading reserves beside its title. Zero
    /// where the rows have nothing to qualify, which is what keeps Home's
    /// headings as short as the name they state.
    const SUBJECT_ROWS: usize = 0;
    /// Shown while the first load is outstanding.
    const LOADING_MESSAGE: &'static str;
    /// The page's topbar title. `None` draws no topbar, which is what Home
    /// wants: its rows say what they are and nothing above them has to.
    const TITLE: Option<&'static str> = None;

    /// The orders the reader may choose between, first one default. Empty draws
    /// no control: Home's rows are sections, and their order is the point.
    const SORTS: &'static [ShelfSort] = &[];

    /// The topbar's detail line, given how many rows came back.
    fn count_label(_rows: usize) -> String {
        String::new()
    }

    /// `sort` is one of [`Self::SORTS`], or `None` where the source declares none.
    fn load(library: LibraryClient, sort: Option<ShelfSort>) -> ShelfFuture;
}

/// One row as the page holds it. The heading is data; the shelf is an entity
/// because a carousel owns a page offset that has to survive a repaint.
struct ShelfEntry {
    id: SharedString,
    title: SharedString,
    subjects: Vec<(Option<SharedString>, SharedString)>,
    shelf: Entity<Paginator>,
    cards: Vec<Entity<BookCardView>>,
}

pub(crate) struct ShelfPage<S: ShelfSource> {
    ui: LibraryContext,
    /// One entry per *shown* row, in the order the source returned them.
    /// Parallel to the outer paginator's children, so the paginator's cursor
    /// indexes it.
    rows: Vec<ShelfEntry>,
    /// Every row the source returned, before the search narrowed them. Retained
    /// so typing filters what is already here instead of asking the library
    /// again: the rows are a whole-library answer either way.
    source_rows: Vec<ShelfRow>,
    /// The order the rows were asked for, from [`ShelfSource::SORTS`].
    sort: Option<ShelfSort>,
    search: SharedString,
    search_field: Option<SearchField>,
    search_open: bool,
    _search_subscription: Option<Subscription>,
    error: Option<SharedString>,
    refresh: LatestRequest,
    /// Whether the current load has resolved. No rows means "still loading"
    /// before the first result and "nothing to show" after it.
    loaded: bool,
    paginator: Entity<Paginator>,
    focus: FocusHandle,
    _updates: Vec<Subscription>,
    source: PhantomData<S>,
}

impl<S: ShelfSource> ShelfPage<S> {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let paginator = cx.new(|_| Paginator::new(format!("{}-rows", S::ID), []));
        let updates = ui.updates();
        let update_subscriptions = vec![
            cx.subscribe(&updates, |page, _, _: &LibraryContentsChanged, cx| page.refresh(cx)),
            cx.subscribe(&updates, |_, _, _: &LibraryScanChanged, cx| cx.notify()),
            cx.observe_global::<crate::stores::CoverTextSetting>(|page, cx| page.refresh_cover_text(cx)),
        ];
        let mut page = Self {
            ui,
            rows: Vec::new(),
            source_rows: Vec::new(),
            sort: S::SORTS.first().copied(),
            search: SharedString::default(),
            search_field: None,
            search_open: false,
            _search_subscription: None,
            error: None,
            refresh: LatestRequest::default(),
            loaded: false,
            paginator,
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            _updates: update_subscriptions,
            source: PhantomData,
        };
        page.refresh(cx);
        page
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let generation = self.refresh.begin();
        let library_id = *self.ui.backend().id();
        if S::ID == "home" {
            crate::services::library_add_trace::mark(library_id, "home_request_started");
        }
        let load = S::load(self.ui.backend().clone(), self.sort);
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            if S::ID == "home" {
                crate::services::library_add_trace::mark(library_id, "home_response_received");
            }
            cx.defer_update(page, move |page, cx| {
                if !page.refresh.is_current(generation) {
                    return;
                }
                // Either outcome settles the question of whether the page is
                // still waiting: an error surfaces its own banner and must not
                // leave the body claiming to be loading forever.
                if S::ID == "home" {
                    crate::services::library_add_trace::mark(library_id, if result.is_ok() { "home_result_installed" } else { "home_load_failed" });
                }
                page.loaded = true;
                match result {
                    Ok(rows) => {
                        page.error = None;
                        page.source_rows = rows;
                        page.install_matching_rows(cx);
                    }
                    Err(error) => {
                        log::error!("failed to load {}: {error}", S::ID);
                        page.error = Some(error.into());
                    }
                }
                cx.notify();
            });
        });
        self.refresh.install(task);
    }

    /// Narrows the loaded rows to the search and installs what is left.
    ///
    /// A row survives on its own name or on a book's, and a row kept only for
    /// its books is narrowed to those: searching an author shows their whole
    /// shelf, searching a title shows the shelves it appears on holding it
    /// alone. An empty query keeps everything, which is the unsearched page.
    fn install_matching_rows(&mut self, cx: &mut Context<Self>) {
        let query = self.search.trim().to_lowercase();
        let rows = if query.is_empty() {
            self.source_rows.clone()
        } else {
            self.source_rows
                .iter()
                .filter_map(|row| {
                    let matches = |text: &str| text.to_lowercase().contains(&query);
                    if matches(&row.title) {
                        return Some(row.clone());
                    }
                    let books = row.books.iter().filter(|book| matches(&book.title) || matches(&book.author)).cloned().collect::<Vec<_>>();
                    (!books.is_empty()).then(|| ShelfRow { books, ..row.clone() })
                })
                .collect()
        };
        self.install(rows, cx);
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        let Some(search_field) = &self.search_field else { return };
        let query = SharedString::from(search_field.query(cx));
        if self.search == query {
            return;
        }
        self.search = query;
        // The rows are already here, so the page narrows on the keystroke
        // rather than after a debounce that would only wait for itself.
        self.install_matching_rows(cx);
        cx.notify();
    }

    /// Choosing an order is a new question for the library, not a narrowing of
    /// the answer it already gave, so this reloads where the search does not.
    fn select_sort(&mut self, sort: ShelfSort, cx: &mut Context<Self>) {
        if self.sort == Some(sort) {
            return;
        }
        self.sort = Some(sort);
        self.refresh(cx);
        cx.notify();
    }

    fn render_sort_control(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let default_sort = S::SORTS.first().copied()?;
        let selected = self.sort.unwrap_or(default_sort);
        let page = cx.entity();
        // An accent edge marks a non-default order as applied, matching how the
        // browse page marks its own filters. It is not the selected treatment:
        // this is a dropdown trigger, not a thing chosen in the toolbar.
        let trigger = components::outlined_icon_button(format!("{}-sort", S::ID), "Sort", IconName::Settings2, theme).when(selected != default_sort, |button| button.border_color(theme.accent).text_color(theme.accent));
        Some(
            trigger
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                    let mut menu = menu.item(components::menu_section("Sort"));
                    for sort in S::SORTS.iter().copied() {
                        let page = page.clone();
                        menu = menu.item(
                            components::menu_item(sort.label(), move |_, _, cx| {
                                page.update(cx, |page, cx| page.select_sort(sort, cx));
                            })
                            .checked(sort == selected),
                        );
                    }
                    menu
                })
                .into_any_element(),
        )
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.search_open = false;
            self.focus.focus(window, cx);
        } else {
            if self.search_field.is_none() {
                let (search_field, subscription) = SearchField::new(Some("Search books or authors"), |page: &mut Self, cx| page.search_changed(cx), window, cx);
                self.search_field = Some(search_field);
                self._search_subscription = Some(subscription);
            }
            self.search_open = true;
            if let Some(search) = &self.search_field {
                search.focus(window, cx);
            }
        }
        cx.notify();
    }

    fn install(&mut self, rows: Vec<ShelfRow>, cx: &mut Context<Self>) {
        // A reload is not a new page. Cards carry a cover that was fetched and
        // faded in once; rebuilding them for every library update refetches
        // every visible thumbnail and replays every fade. Keep the shelf and the
        // card that already stand for this row and this book, and replace only
        // what they hold.
        //
        // A row states a title and its chips over a shelf entity, so those and
        // their order are all a rebuilt page would say differently; a shelf's
        // own contents are replaced through the entity the row already holds.
        let unchanged = self.rows.len() == rows.len() && self.rows.iter().zip(&rows).all(|(entry, row)| entry.id == row.id && entry.title == row.title && entry.subjects == row.subjects);
        let mut reusable = self.rows.drain(..).map(|entry| (entry.id.clone(), entry)).collect::<HashMap<_, _>>();
        for row in rows {
            let mut entry = reusable.remove(&row.id).unwrap_or_else(|| ShelfEntry {
                id: row.id.clone(),
                title: SharedString::default(),
                subjects: Vec::new(),
                shelf: cx.new(|_| Paginator::new(format!("{}-shelf-{}", S::ID, row.id), []).max_rows(1).flow_axis(PaginatorAxis::Horizontal).horizontal_only()),
                cards: Vec::new(),
            });
            entry.title = row.title;
            entry.subjects = row.subjects;

            let shelf_unchanged = entry.cards.iter().map(|card| card.read(cx).content_hash()).eq(row.books.iter().map(|book| book.content_hash));
            let mut cards = entry.cards.drain(..).map(|card| (card.read(cx).content_hash(), card)).collect::<HashMap<_, _>>();
            for book in row.books {
                let card = match cards.remove(&book.content_hash) {
                    Some(card) => {
                        card.update(cx, |card, _| card.replace_row(book, None));
                        card
                    }
                    None => BookCardView::create(book, self.ui.clone(), cx),
                };
                entry.cards.push(card);
            }
            // Rebuilding the children resets the shelf to its first page and
            // discards what it has prefetched. When the same books are still
            // here in the same order, there is nothing to rebuild.
            if !shelf_unchanged {
                let children = Self::shelf_children(&entry.cards);
                let cover_text = crate::stores::current_cover_text(cx);
                entry.shelf.update(cx, |shelf, cx| shelf.set_groups([PaginatorGroup::new(shelf_card_policy(cover_text), children)], cx));
            }
            self.rows.push(entry);
        }
        if unchanged {
            return;
        }
        let groups = self.row_groups(cx);
        self.paginator.update(cx, |paginator, cx| paginator.set_groups(groups, cx));
    }

    /// One shelf's cards as the paginator wants them, in the order the shelf
    /// holds them. Shared between `install`, which builds this the first time
    /// a shelf's books change, and the "cover text" observer, which rebuilds
    /// it for every shelf when the reserved row height has to change with it.
    fn shelf_children(cards: &[Entity<BookCardView>]) -> Vec<PaginatorChild> {
        cards
            .iter()
            .map(|card| {
                let card = card.clone();
                let prepared_card = card.clone();
                let open_card = card.clone();
                PaginatorChild::new(move |_, _| div().w_full().min_w_0().child(card.clone()).into_any_element()).with_activate(move |window, cx| BookCardView::activate(&open_card, window, cx)).with_prepare(move |window, cx| {
                    let resolution = BookCardView::thumbnail_resolution(window);
                    prepared_card.update(cx, |card, cx| card.prepare(resolution, cx))
                })
            })
            .collect()
    }

    /// Rebuilds every shelf's row height and the page's own row heights for
    /// the current "cover text" setting.
    ///
    /// Unlike `install`, this cannot skip a shelf whose books are unchanged —
    /// the setting changed, not the books, so every shelf's reserved height is
    /// stale regardless of whether its cards are.
    fn refresh_cover_text(&mut self, cx: &mut Context<Self>) {
        let cover_text = crate::stores::current_cover_text(cx);
        for entry in &self.rows {
            let children = Self::shelf_children(&entry.cards);
            entry.shelf.update(cx, |shelf, cx| shelf.set_groups([PaginatorGroup::new(shelf_card_policy(cover_text), children)], cx));
        }
        let groups = self.row_groups(cx);
        self.paginator.update(cx, |paginator, cx| paginator.set_groups(groups, cx));
        cx.notify();
    }

    /// The rows, as one group of full-width children.
    ///
    /// `cursor` carries the paginator's keyboard cursor into the row closures,
    /// which run inside the paginator's own render: reading the entity back
    /// there would borrow something already borrowed.
    fn row_groups(&self, cx: &mut Context<Self>) -> Vec<PaginatorGroup> {
        let cursor = Rc::new(std::cell::Cell::new(self.paginator.read(cx).cursor()));
        let show_controls = !cx.supports_touch_input();
        let children = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let id = entry.id.clone();
                let title = entry.title.clone();
                let subjects = entry.subjects.clone();
                let shelf = entry.shelf.clone();
                let cursor = cursor.clone();
                PaginatorChild::new(move |_, cx| {
                    let theme = components::browser_theme(cx);
                    let mut heading = components::shelf_heading(S::SUBJECT_ROWS).child(components::shelf_title(title.clone(), cursor.get() == Some(index), theme)).child(components::shelf_subjects(subjects.iter().cloned(), theme));
                    if show_controls {
                        heading = heading.child(shelf_controls(&id, &shelf, theme, cx));
                    }
                    components::shelf_row().child(heading).child(components::shelf_carousel().child(shelf.clone())).into_any_element()
                })
            })
            .collect::<Vec<_>>();
        if children.is_empty() {
            return Vec::new();
        }
        vec![PaginatorGroup::new(shelf_rows_policy(crate::stores::current_cover_text(cx), S::SUBJECT_ROWS), children)]
    }

    /// Up and down walk the rows and turn the page at the edges; left and right
    /// walk the cards of the row the cursor is on. The two axes belong to two
    /// different paginators, which is what makes a shelf browsable without
    /// leaving the page.
    fn navigate(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }
        // Escape leaves the search before anything else reads the key, and while
        // the field has focus the arrows belong to the text, not to the shelves.
        if self.search_open && event.keystroke.key == "escape" {
            self.toggle_search(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.search_field.as_ref().is_some_and(|search| search.is_focused(window, cx)) {
            return;
        }
        let handled = match event.keystroke.key.as_str() {
            // Above the first row is the library the page belongs to, so the
            // cursor leaves upwards into the switcher rather than stopping.
            "up" => {
                self.move_row(CursorMove::Up, cx) || {
                    window.dispatch_action(Box::new(crate::OpenLibrarySwitcher), cx);
                    true
                }
            }
            "down" => self.move_row(CursorMove::Down, cx),
            "left" => self.move_card(CursorMove::Left, cx),
            "right" => self.move_card(CursorMove::Right, cx),
            "pageup" => self.selected_shelf(cx).is_some_and(|shelf| shelf.update(cx, |shelf, cx| shelf.previous(cx))),
            "pagedown" => self.selected_shelf(cx).is_some_and(|shelf| shelf.update(cx, |shelf, cx| shelf.next(cx))),
            "enter" => self.selected_shelf(cx).is_some_and(|shelf| {
                window.defer(cx, move |window, cx| {
                    if let Some(activate) = shelf.read(cx).cursor_activation() {
                        activate(window, cx);
                    }
                });
                true
            }),
            _ => false,
        };
        if handled {
            self.sync_cursor(cx);
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// The cursor lives on the outer paginator, but the rows were built with the
    /// value it held when they were built. Rebuilding them is what carries the
    /// move into the heading's tint.
    fn move_row(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        if !self.paginator.update(cx, |paginator, cx| paginator.move_cursor(movement, cx)) {
            return false;
        }
        // Entering a row puts the cursor on a book, so the next left or right is
        // a step along the shelf rather than the keystroke that places it.
        if let Some(shelf) = self.selected_shelf(cx) {
            shelf.update(cx, |shelf, cx| {
                if shelf.cursor().is_none() {
                    shelf.move_cursor(CursorMove::Right, cx);
                }
            });
        }
        let groups = self.row_groups(cx);
        self.paginator.update(cx, |paginator, cx| paginator.refresh_groups(groups, cx));
        true
    }

    /// Left and right move along the selected shelf. With no row selected yet
    /// the first press selects one, so the cursor appears where the reader is
    /// looking rather than nowhere.
    fn move_card(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        let Some(shelf) = self.selected_shelf(cx) else {
            return self.move_row(CursorMove::Down, cx);
        };
        shelf.update(cx, |shelf, cx| shelf.move_cursor(movement, cx))
    }

    fn selected_shelf(&self, cx: &gpui::App) -> Option<Entity<Paginator>> {
        let index = self.paginator.read(cx).cursor()?;
        self.rows.get(index).map(|entry| entry.shelf.clone())
    }

    /// A card knows what focused looks like; the page knows where the cursor is.
    /// Only the selected row has one, so a shelf the reader has left keeps its
    /// page but not its highlight.
    fn sync_cursor(&self, cx: &mut Context<Self>) {
        let selected = self.paginator.read(cx).cursor();
        for (index, entry) in self.rows.iter().enumerate() {
            let cursor = (selected == Some(index)).then(|| entry.shelf.read(cx).cursor()).flatten();
            for (card_index, card) in entry.cards.iter().enumerate() {
                card.update(cx, |card, cx| card.set_focused(cursor == Some(card_index), cx));
            }
        }
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        // Nothing to show is either a library that is still arriving or one that
        // holds no books. Saying nothing at all leaves a blank page in both.
        if self.rows.is_empty() && self.error.is_none() {
            return if self.ui.updates().read(cx).scanning() {
                components::browser_loading_message("Scanning library…", theme).into_any_element()
            } else if self.loaded && !self.ui.updates().read(cx).loading() {
                library_empty_state(theme)
            } else {
                components::browser_loading_message(S::LOADING_MESSAGE, theme).into_any_element()
            };
        }
        let body = components::shelf_page().child(self.paginator.clone()).into_any_element();
        if let Some(error) = self.error.clone() {
            let page = cx.entity();
            let retry = components::topbar_action_button(format!("{}-retry", S::ID), "Retry", IconName::Redo2, theme).on_click(move |_, _, cx| page.update(cx, |page, cx| page.refresh(cx)));
            components::browser_error_banner(error, retry, body).into_any_element()
        } else {
            body
        }
    }

    fn render_topbar(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let title = S::TITLE?;
        let theme = components::browser_theme(cx);
        let paginator = self.paginator.read(cx);
        let has_previous = paginator.has_previous();
        let has_next = paginator.has_next();
        let can_previous = paginator.can_previous();
        let can_next = paginator.can_next();
        let previous = has_previous.then(|| {
            components::outlined_icon_button(format!("{}-previous", S::ID), "Previous", IconName::ChevronUp, theme)
                .disabled(!can_previous)
                .on_click(cx.listener(|page, _, _, cx| {
                    page.paginator.update(cx, |paginator, cx| paginator.previous_with_axis(PaginatorAxis::Vertical, cx));
                }))
                .into_any_element()
        });
        let next = has_next.then(|| {
            components::outlined_icon_button(format!("{}-next", S::ID), "Next", IconName::ChevronDown, theme)
                .disabled(!can_next)
                .on_click(cx.listener(|page, _, _, cx| {
                    page.paginator.update(cx, |paginator, cx| paginator.next_with_axis(PaginatorAxis::Vertical, cx));
                }))
                .into_any_element()
        });
        let search_label = if self.search_open { "Hide search" } else { "Search books or authors" };
        let controls = components::browser_topbar_right()
            .when(self.search_open, |controls| controls.flex_1())
            .children(self.search_open.then(|| div().flex_1().min_w(gpui::rems(8.0)).child(self.search_field.as_ref().expect("search field exists while the search is open").render())))
            .child(
                components::outlined_icon_button(format!("{}-search", S::ID), search_label, IconName::Search, theme)
                    .when(self.search_open || !self.search.is_empty(), |button| button.bg(theme.accent).text_color(theme.accent_text))
                    .on_click(cx.listener(|page, _, window, cx| page.toggle_search(window, cx))),
            )
            .children(self.render_sort_control(theme, cx))
            .child(components::paginator_controls(previous, next));
        Some(
            components::browser_topbar(theme)
                .child(if components::uses_mobile_navigation(window) {
                    let back = (!components::has_native_back_button()).then(|| components::mobile_back_button(theme).on_click(|_, window, cx| window.dispatch_action(Box::new(crate::DismissSettings), cx)));
                    components::mobile_navigation_heading(title, back)
                } else {
                    components::browser_topbar_left().child(components::browser_page_title(title, S::count_label(self.rows.len()), theme))
                })
                .child(controls)
                .into_any_element(),
        )
    }
}

/// A shelf's own controls, which step it sideways. They act on the shelf entity
/// directly: a row is drawn inside the outer paginator's render, where the page
/// itself is not reachable.
fn shelf_controls(id: &SharedString, shelf: &Entity<Paginator>, theme: components::BrowserTheme, cx: &gpui::App) -> gpui::Div {
    let state = shelf.read(cx);
    let (has_previous, has_next, can_previous, can_next) = (state.has_previous(), state.has_next(), state.can_previous(), state.can_next());
    let previous = has_previous.then(|| {
        let shelf = shelf.clone();
        components::outlined_icon_button(format!("shelf-{id}-previous"), "Previous", IconName::ChevronLeft, theme)
            .disabled(!can_previous)
            .on_click(move |_, _, cx| {
                shelf.update(cx, |shelf, cx| shelf.previous(cx));
            })
            .into_any_element()
    });
    let next = has_next.then(|| {
        let shelf = shelf.clone();
        components::outlined_icon_button(format!("shelf-{id}-next"), "Next", IconName::ChevronRight, theme)
            .disabled(!can_next)
            .on_click(move |_, _, cx| {
                shelf.update(cx, |shelf, cx| shelf.next(cx));
            })
            .into_any_element()
    });
    components::paginator_controls(previous, next)
}

impl<S: ShelfSource> Render for ShelfPage<S> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.loaded && self.error.is_none() && S::ID == "home" {
            let library_id = *self.ui.backend().id();
            if crate::services::library_add_trace::mark(library_id, "home_content_render") {
                window.on_next_frame(move |_, _| {
                    crate::services::library_add_trace::mark(library_id, "home_frame_completed");
                });
            }
        }
        if self.loaded {}
        let topbar = self.render_topbar(window, cx);
        let body = self.render_body(cx);
        components::full_size_column().track_focus(&self.focus).on_key_down(cx.listener(Self::navigate)).children(topbar).child(components::remaining_space_column().child(body))
    }
}

impl<S: ShelfSource> Focusable for ShelfPage<S> {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

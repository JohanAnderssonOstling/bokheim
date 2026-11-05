//! Annotations and book text share book rows, controls and page loading.
use ui_text::text;

mod overlay;
mod row;

use super::pagination::{compatible_pages, merge_page, page_request_for_slots};
use crate::library::services::{LibraryAnnotationsChanged, LibraryContentsChanged, LibraryContext, LibraryDownloadChanged};
use crate::library::widgets::{BookCardView, Paginator, PaginatorAxis, PaginatorChild, PaginatorChildren, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy};
use crate::navigation::LibraryRoute;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FocusHandle, Focusable, Font, Modifiers, Pixels, Render, SharedString, Subscription, Task, Window, div, px, rems};
use gpui_component::{
    IconName,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenu},
};
use library_model::{BrowsePageRequest, BrowsePageState, CardKey, LibraryFileTypeFilter, NotebookQuery, NotebookRequest, NotebookSort, TextSearchRemoval};
use row::{NotebookRow, RowData};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem,
    time::Duration,
};
use ui_components as components;

const BOOK_PAGE_SIZE: usize = 8;

struct ResultPage {
    books: Vec<RowData>,
    page: BrowsePageState,
}

pub(crate) struct NotebookPage {
    content_search: bool,
    library: LibraryContext,
    query: NotebookQuery,
    displayed_query: Option<NotebookQuery>,
    rows: BTreeMap<usize, Entity<NotebookRow>>,
    cached: HashMap<CardKey, (Entity<NotebookRow>, [Subscription; 2])>,
    page: Option<BrowsePageState>,
    refresh_pending: bool,
    snapshot_loading: bool,
    removals: Vec<TextSearchRemoval>,
    request: Option<Task<()>>,
    error: Option<SharedString>,
    search: Option<(Entity<InputState>, Subscription)>,
    flow: Entity<Paginator>,
    layout: Option<(Pixels, Font, u32, Pixels)>,
    reset_flow: bool,
    focus: FocusHandle,
    options_open: bool,
    _cover_text_updates: Subscription,
    _flow_updates: Subscription,
    _updates: Vec<Subscription>,
}

impl NotebookPage {
    pub(crate) fn new(library: LibraryContext, cx: &mut Context<Self>) -> Self {
        Self::new_kind(library, false, cx)
    }
    pub(crate) fn text_search(library: LibraryContext, cx: &mut Context<Self>) -> Self {
        Self::new_kind(library, true, cx)
    }
    fn new_kind(library: LibraryContext, content_search: bool, cx: &mut Context<Self>) -> Self {
        let flow = cx.new(|_| Paginator::new("notebook-books", []).flow_axis(PaginatorAxis::Vertical));
        let flow_updates = cx.observe(&flow, move |page: &mut Self, _, cx| {
            page.load_visible_page(cx);
        });
        let cover_text_updates = cx.observe_global::<crate::stores::CoverTextSetting>(|page: &mut Self, cx| {
            page.layout = None;
            cx.notify();
        });
        let updates = library.updates();
        let subscriptions = vec![
            cx.subscribe(&updates, |page: &mut Self, _, _: &LibraryContentsChanged, cx| {
                if !page.content_search {
                    page.invalidate(cx);
                }
            }),
            cx.subscribe(&updates, |page: &mut Self, _, _: &LibraryDownloadChanged, cx| {
                if !page.content_search {
                    page.invalidate(cx);
                }
            }),
            cx.subscribe(&updates, |page: &mut Self, _, _: &LibraryAnnotationsChanged, cx| {
                if !page.content_search {
                    page.invalidate(cx);
                }
            }),
            cx.subscribe(&updates, |page: &mut Self, _, removal: &TextSearchRemoval, cx| page.remove_snapshot_books(removal.clone(), cx)),
        ];
        let mut page = Self {
            content_search,
            library,
            query: NotebookQuery::default(),
            displayed_query: None,
            rows: BTreeMap::new(),
            cached: HashMap::new(),
            page: None,
            refresh_pending: false,
            snapshot_loading: false,
            removals: Vec::new(),
            request: None,
            error: None,
            search: None,
            flow,
            layout: None,
            reset_flow: true,
            focus: cx.focus_handle(),
            options_open: false,
            _cover_text_updates: cover_text_updates,
            _flow_updates: flow_updates,
            _updates: subscriptions,
        };
        page.load(None, cx);
        page
    }

    fn invalidate(&mut self, cx: &mut Context<Self>) {
        if !self.refresh_pending {
            self.refresh_pending = true;
            cx.notify();
        }
    }

    fn remove_snapshot_books(&mut self, removal: TextSearchRemoval, cx: &mut Context<Self>) {
        if !self.content_search {
            return;
        }
        if self.snapshot_loading && self.request.is_some() {
            self.removals.push(removal);
            return;
        }
        if self.page.as_ref().and_then(|page| page.result_revision.as_deref()) != Some(removal.revision.as_str()) {
            return;
        }
        self.request = None;
        self.rows = mem::take(&mut self.rows)
            .into_iter()
            .filter_map(|(index, row)| {
                if removal.positions.binary_search(&index).is_ok() {
                    return None;
                }
                let shift = removal.positions.partition_point(|removed| *removed < index);
                Some((index - shift, row))
            })
            .collect();
        let remaining = self.rows.values().map(|row| row.entity_id()).collect::<HashSet<_>>();
        self.cached.retain(|_, (row, _)| remaining.contains(&row.entity_id()));
        if let Some(page) = &mut self.page {
            page.book_total = page.book_total.saturating_sub(removal.positions.len());
        }
        self.layout = None;
        self.error = None;
        cx.notify();
    }

    fn load(&mut self, continuation: Option<BrowsePageRequest>, cx: &mut Context<Self>) {
        self.snapshot_loading = self.content_search && continuation.is_none();
        if continuation.is_none() {
            self.refresh_pending = false;
            self.removals.clear();
            self.error = None;
        }
        if self.content_search && self.query.search.trim().chars().take(3).count() < 3 {
            self.request = None;
            self.snapshot_loading = false;
            self.rows.clear();
            self.cached.clear();
            self.page = None;
            self.displayed_query = None;
            self.layout = None;
            self.reset_flow = true;
            cx.notify();
            return;
        }
        let same_query = self.displayed_query.as_ref() == Some(&self.query);
        let content_search = self.content_search;
        let limit = if same_query && !content_search { self.rows.len().max(BOOK_PAGE_SIZE) } else { BOOK_PAGE_SIZE };
        let offset = if same_query && content_search { self.flow.read(cx).visible_child_range().map_or(0, |range| range.start / BOOK_PAGE_SIZE * BOOK_PAGE_SIZE) } else { 0 };
        let request = NotebookRequest {
            page: continuation.unwrap_or(BrowsePageRequest { section_offset: 0, section_limit: 0, book_offset: offset, book_limit: limit }),
            revision: continuation.and_then(|_| self.page.as_ref()?.result_revision.clone()),
            annotation_offsets: if same_query && continuation.is_none() { self.rows.values().filter_map(|row| row.read(cx).annotation_window()).filter(|(_, offset)| *offset > 0).collect() } else { HashMap::new() },
        };
        let query = self.query.clone();
        let library = self.library.backend().clone();
        self.request = Some(cx.spawn(async move |page, cx| {
            // Coalesce search keystrokes; viewport requests start immediately.
            if continuation.is_none() {
                cx.background_executor().timer(Duration::from_millis(60)).await;
            }
            let result = if content_search {
                library.text_search(query.clone(), request).await.map(|page| page.map(|page| ResultPage { books: page.books.into_iter().map(RowData::Text).collect(), page: page.page }))
            } else {
                library.notebook(query.clone(), request).await.map(|page| page.map(|page| ResultPage { books: page.books.into_iter().map(RowData::Note).collect(), page: page.page }))
            };
            let _ = page.update(cx, |page, cx| {
                page.request = None;
                page.snapshot_loading = false;
                match result {
                    Ok(Some(contents)) => {
                        if continuation.is_some() && !page.page.as_ref().is_some_and(|current| compatible_pages(current, &contents.page)) {
                            if content_search {
                                // A trash update may have shortened the retained selection.
                                page.layout = None;
                                cx.notify();
                            } else {
                                page.load(None, cx);
                            }
                            return;
                        }
                        page.error = None;
                        page.reset_flow |= page.displayed_query.as_ref() != Some(&query);
                        page.layout = None;
                        page.displayed_query = Some(query.clone());
                        if continuation.is_none() {
                            page.rows.clear();
                        }
                        let revision = contents.page.result_revision.clone().unwrap_or_default();
                        let library = page.library.clone();
                        let cached = &mut page.cached;
                        merge_page(&mut page.rows, contents.page.book_offset, contents.books, |data| {
                            let book = data.book().key();
                            if let Some((row, _)) = cached.get(&book) {
                                row.update(cx, |row, cx| row.replace_data(data, query.clone(), &revision, cx));
                                row.clone()
                            } else {
                                let row = cx.new(|cx| NotebookRow::from_data(data, query.clone(), &revision, library.clone(), cx));
                                let book_card = row.read(cx).book_card().clone();
                                let subscription = cx.observe(&book_card, |page: &mut Self, _, cx| {
                                    page.layout = None;
                                    cx.notify();
                                });
                                let stale = cx.subscribe(&row, |page: &mut Self, _, _: &row::Stale, cx| {
                                    if page.content_search {
                                        page.error = Some(text!(browse::search_again_to_refresh_these_results).into());
                                        cx.notify();
                                    } else {
                                        page.invalidate(cx);
                                    }
                                });
                                cached.insert(book, (row.clone(), [subscription, stale]));
                                row
                            }
                        });
                        page.page = Some(contents.page);
                        if continuation.is_none() {
                            let visible = page.rows.values().map(|row| row.entity_id()).collect::<HashSet<_>>();
                            page.cached.retain(|_, (row, _)| visible.contains(&row.entity_id()));
                        }
                        for removal in mem::take(&mut page.removals) {
                            page.remove_snapshot_books(removal, cx);
                        }
                    }
                    Ok(None) if content_search => page.error = Some(text!(browse::search_again_to_refresh_these_results).into()),
                    Ok(None) => {
                        page.load(None, cx);
                        return;
                    }
                    Err(error) => {
                        page.error = Some(components::user_facing_error(&error, if content_search { text!(browse::could_not_search_book_text_please_try_again) } else { text!(browse::could_not_load_annotations_please_try_again) }).into())
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn load_visible_page(&mut self, cx: &mut Context<Self>) {
        if self.request.is_some() || self.error.is_some() || self.displayed_query.as_ref() != Some(&self.query) {
            return;
        }
        let Some(page) = &self.page else { return };
        let Some(range) = self.flow.read(cx).visible_and_adjacent_child_range() else { return };
        if let Some(mut request) = page_request_for_slots(range, 0, 0, page.book_total, |_| true, |index| self.rows.contains_key(&index)) {
            // Rows include annotations as well as a cover, so use a smaller
            // hydration window with the same range selection as Browse.
            request.book_limit = BOOK_PAGE_SIZE;
            self.load(Some(request), cx);
        }
    }

    fn groups(&self, window: &Window, cx: &App) -> Vec<PaginatorGroup> {
        let total = self.page.as_ref().map_or(0, |page| page.book_total);
        let mut groups = Vec::new();
        let mut position = 0;
        let placeholder_height = self.rows.values().next().map_or(rems(18.0).to_pixels(window.rem_size()), |row| row.read(cx).height(window, cx));
        let policy = |height| PaginatorGroupPolicy::new(PaginatorWidthPolicy::pixels(window.viewport_size().width), PaginatorSizing::pixels(height), px(0.0)).flush_edges();
        let placeholder = PaginatorChild::new(|_, _, cx| div().size_full().bg(components::browser_theme(cx).hover).into_any_element());
        for (&index, row) in &self.rows {
            if index > position {
                groups.push(PaginatorGroup::from_slots(policy(placeholder_height), PaginatorChildren::sparse(index - position, placeholder.clone(), BTreeMap::new())));
            }
            let book = row.read(cx).book_card().clone();
            let id = format!("notebook-book-{}", book.read(cx).key());
            let height = row.read(cx).height(window, cx);
            let row = row.clone();
            let child = PaginatorChild::new(move |_, _, _| row.clone().into_any_element()).with_id(id).with_own_cell().with_prepare(move |window, cx| BookCardView::prefetch_for_window(&book, window, cx));
            groups.push(PaginatorGroup::new(policy(height), [child]));
            position = index + 1;
        }
        if position < total {
            groups.push(PaginatorGroup::from_slots(policy(placeholder_height), PaginatorChildren::sparse(total - position, placeholder, BTreeMap::new())));
        }
        groups
    }

    fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        self.search
            .get_or_insert_with(|| {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder(if self.content_search { text!(browse::search_book_text) } else { text!(browse::search_notes) }));
                let subscription = cx.subscribe(&input, |page, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        page.query.search = input.read(cx).value().to_string();
                        page.load(None, cx);
                    }
                });
                (input, subscription)
            })
            .0
            .clone()
    }

    pub(crate) fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        if self.options_open {
            self.options_open = false;
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(None, cx);
    }

    fn choose(&mut self, choice: Choice, cx: &mut Context<Self>) {
        match choice {
            Choice::Sort(sort) => self.query.sort = sort,
            Choice::Format(format) => match self.query.formats.iter().position(|value| *value == format) {
                Some(index) => {
                    self.query.formats.remove(index);
                }
                None => self.query.formats.push(format),
            },
            Choice::Clear => self.query.formats.clear(),
        }
        self.load(None, cx);
    }

    fn options_menu(menu: PopupMenu, page: Entity<Self>, cx: &mut Context<PopupMenu>) -> PopupMenu {
        let query = page.read(cx).query.clone();
        let mut menu = menu.dismiss_on_select(false).item(components::menu_section(text!(browse::sort_by)));
        for (label, choice, checked) in choices(&query, page.read(cx).content_search) {
            let page = page.clone();
            let rebuild = cx.weak_entity();
            menu = menu.item(
                components::menu_item(label, move |_, window, cx| {
                    page.update(cx, |page, cx| page.choose(choice, cx));
                    let page = page.clone();
                    let rebuild = rebuild.clone();
                    window.defer(cx, move |window, cx| {
                        if let Some(menu) = rebuild.upgrade() {
                            menu.update(cx, |menu, cx| menu.rebuild(window, cx, |menu, _, cx| Self::options_menu(menu, page, cx)));
                        }
                    });
                })
                .checked(checked),
            );
            if matches!(choice, Choice::Sort(NotebookSort::BookName)) {
                menu = menu.separator().item(components::menu_section(text!(browse::format)));
            }
        }
        menu
    }

    fn options_sheet(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let mut rows = div().flex().flex_col().child(components::bottom_sheet_section(text!(browse::sort_by), theme));
        for (index, (label, choice, checked)) in choices(&self.query, self.content_search).into_iter().enumerate() {
            rows = rows.child(components::bottom_sheet_row(("notebook-option", index), label, checked, theme).on_click(cx.listener(move |page, _, _, cx| page.choose(choice, cx))));
            if matches!(choice, Choice::Sort(NotebookSort::BookName)) {
                rows = rows.child(components::bottom_sheet_divider(theme)).child(components::bottom_sheet_section(text!(browse::format), theme));
            }
        }
        let header = components::bottom_sheet_header(text!(browse::sort_filter), theme).child(components::bottom_sheet_close_button("notebook-options-close", theme).on_click(cx.listener(|page, _, _, cx| {
            page.options_open = false;
            cx.notify();
        })));
        components::bottom_sheet_overlay()
            .child(components::modal_scrim("notebook-options-scrim", theme).on_click(cx.listener(|page, _, _, cx| {
                page.options_open = false;
                cx.notify();
            })))
            .child(components::bottom_sheet_surface(theme).child(header).child(div().id("notebook-options-rows").flex_1().min_h_0().overflow_y_scroll().child(rows)))
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
enum Choice {
    Sort(NotebookSort),
    Format(LibraryFileTypeFilter),
    Clear,
}

fn choices(query: &NotebookQuery, content_search: bool) -> Vec<(&'static str, Choice, bool)> {
    vec![
        (if content_search { text!(browse::most_recently_added) } else { text!(browse::most_recent_annotations) }, Choice::Sort(NotebookSort::Recent), query.sort == NotebookSort::Recent),
        (text!(browse::book_name), Choice::Sort(NotebookSort::BookName), query.sort == NotebookSort::BookName),
        (text!(browse::books), Choice::Format(LibraryFileTypeFilter::Book), query.formats.contains(&LibraryFileTypeFilter::Book)),
        (text!(browse::pdf), Choice::Format(LibraryFileTypeFilter::Pdf), query.formats.contains(&LibraryFileTypeFilter::Pdf)),
        (text!(browse::clear_filters), Choice::Clear, false),
    ]
}

impl Focusable for NotebookPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NotebookPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.refresh_pending && self.request.is_none() {
            self.refresh(cx);
        }
        let layout = (window.rem_size(), window.text_style().font(), window.scale_factor().to_bits(), window.viewport_size().width);
        if self.layout.as_ref() != Some(&layout) {
            let groups = self.groups(window, cx);
            let refresh = !std::mem::take(&mut self.reset_flow);
            self.flow.update(cx, |flow, cx| flow.install_groups(groups, refresh, None, cx));
            self.layout = Some(layout);
        }
        let theme = components::browser_theme(cx);
        let input = self.search_input(window, cx);
        let compact = components::WindowWidthClass::for_window(window).is_compact();
        let trigger = components::selectable_button(components::outlined_icon_button("notebook-options", text!(browse::sort_filter), IconName::Settings2), !self.query.formats.is_empty(), theme);
        let options = if compact || components::touch_ui() || cfg!(feature = "kobo") {
            trigger
                .on_click(cx.listener(|page, _, _, cx| {
                    page.options_open = !page.options_open;
                    cx.notify();
                }))
                .into_any_element()
        } else {
            let page = cx.entity();
            trigger.dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| Self::options_menu(menu, page.clone(), cx)).into_any_element()
        };
        let heading = components::uses_mobile_navigation(window).then(|| crate::navigation::library_root_heading(if self.content_search { LibraryRoute::Search } else { LibraryRoute::Notebook }, theme));
        let topbar = components::topbar(theme).children(heading).child(components::browser_search_control(&input)).child(options);
        let mut body = div()
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|page, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.modifiers == Modifiers::default() && matches!(event.keystroke.key.as_str(), "pageup" | "pagedown") {
                    page.flow.update(cx, |flow, cx| {
                        if event.keystroke.key == "pagedown" {
                            flow.next_with_axis(PaginatorAxis::Vertical, cx);
                        } else {
                            flow.previous_with_axis(PaginatorAxis::Vertical, cx);
                        }
                    });
                    cx.stop_propagation();
                }
            }))
            .child(topbar)
            .child(div().flex_1().min_h_0().px(rems(0.5)).child(self.flow.clone()));
        if let Some(error) = self.error.clone() {
            body = body.child(div().p(rems(1.0)).child(components::error_text(theme).child(error)).child(components::outlined_text_button("notebook-retry", text!(common::retry)).on_click(cx.listener(|page, _, _, cx| page.load(None, cx)))));
        }
        if self.options_open {
            body = body.child(self.options_sheet(cx));
        }
        body
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests;

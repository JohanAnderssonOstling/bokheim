//! Shared state and presentation for hierarchical library pages.
//!
//! Loading, mutations, empty-state copy, and events remain with each concrete
//! page. This module only owns behavior that is identical for every hierarchy.

use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, Focusable, IntoElement, ScrollHandle, Subscription, WeakEntity, Window, container_query, div, px, size};
use gpui_component::IconName;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt, PopupMenu};
use gpui_component::scroll::ScrollableElement as ComponentScrollableElement;
use gpui_component::v_virtual_list;
use std::cell::Cell;
use std::rc::Rc;
use ui_components as components;
use ui_components::ScrollableElement;

use crate::library::services::LibraryContext;
use crate::library::title_filter::normalize_filter;
use crate::library::widgets::{BookGrid, BookManagement, LibraryDrag, LibraryDragPreview};
use epub_svelte_lib::library::LibraryHierarchyEntry;

struct HierarchySplitPane<P> {
    id: u64,
    page: Entity<P>,
    _subscriptions: Vec<Subscription>,
}

/// Flat, equal-width pane ownership shared by hierarchical library pages.
///
/// Concrete pages still decide how a new pane is initialized and refreshed;
/// this type owns the lifecycle, common controls, and horizontal layout.
pub(crate) struct HierarchySplits<P> {
    panes: Vec<HierarchySplitPane<P>>,
    root: Option<WeakEntity<P>>,
    pane_id: u64,
    next_pane_id: u64,
}

impl<P: Render + 'static> HierarchySplits<P> {
    pub(crate) fn new(root: Option<WeakEntity<P>>, pane_id: u64) -> Self {
        Self { panes: Vec::new(), root, pane_id, next_pane_id: 1 }
    }

    pub(crate) fn root(&self) -> Option<&WeakEntity<P>> {
        self.root.as_ref()
    }

    pub(crate) fn is_secondary(&self) -> bool {
        self.root.is_some()
    }

    pub(crate) fn pane_id(&self) -> u64 {
        self.pane_id
    }

    pub(crate) fn next_pane_id(&mut self) -> u64 {
        let pane_id = self.next_pane_id;
        self.next_pane_id = self.next_pane_id.wrapping_add(1).max(1);
        pane_id
    }

    pub(crate) fn push(&mut self, id: u64, page: Entity<P>, subscriptions: Vec<Subscription>) {
        self.panes.push(HierarchySplitPane { id, page, _subscriptions: subscriptions });
    }

    pub(crate) fn close(&mut self, pane_id: u64) -> bool {
        let previous_len = self.panes.len();
        self.panes.retain(|pane| pane.id != pane_id);
        self.panes.len() != previous_len
    }

    pub(crate) fn pages(&self) -> impl Iterator<Item = &Entity<P>> {
        self.panes.iter().map(|pane| &pane.page)
    }

    pub(crate) fn actions<Add, Close>(&self, id_prefix: &'static str, page: Entity<P>, add: Add, close: Close, cx: &mut Context<P>) -> gpui::Div
    where
        Add: Fn(Entity<P>, &mut Window, &mut App) + 'static,
        Close: Fn(Entity<P>, &mut App) + 'static,
    {
        let theme = components::browser_theme(cx);
        let add_page = page.clone();
        let mut actions = components::browser_topbar_right()
            .child(components::topbar_icon_action_button(format!("add-{id_prefix}-split-{}", self.pane_id), "Add split", IconName::PanelRightOpen, theme).on_click(move |_, window, cx| add(add_page.clone(), window, cx)));
        if self.is_secondary() {
            actions = actions.child(components::topbar_icon_action_button(format!("close-{id_prefix}-split-{}", self.pane_id), "Close this split", IconName::Close, theme).on_click(move |_, _, cx| close(page.clone(), cx)));
        }
        actions
    }

    pub(crate) fn render(&self, pane: AnyElement, window: &Window, cx: &mut Context<P>) -> AnyElement {
        if self.is_secondary() || self.panes.is_empty() {
            return pane;
        }
        let theme = components::browser_theme(cx);
        let pane_count = self.panes.len() + 1;
        let available_width = f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size()));
        let pane_width = (available_width / pane_count as f32).max(420.0);
        let mut panes = div().size_full().min_h_0().min_w_0().flex().flex_row().overflow_x_scrollbar().child(div().w(px(pane_width)).h_full().min_h_0().flex_none().border_r_1().border_color(theme.border).child(pane));
        for split in &self.panes {
            panes = panes.child(div().w(px(pane_width)).h_full().min_h_0().flex_none().border_r_1().border_color(theme.border).child(split.page.clone()));
        }
        panes.into_any_element()
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HierarchySort {
    #[default]
    Name,
}

impl HierarchySort {
    fn apply<P: 'static>(self, grid: &mut BookGrid, cx: &mut Context<P>) {
        grid.sort_by(move |left, right| self.compare(left, right), cx);
    }

    fn compare(self, left: &BookCardRow, right: &BookCardRow) -> std::cmp::Ordering {
        left.title.to_lowercase().cmp(&right.title.to_lowercase()).then_with(|| left.title.cmp(&right.title)).then_with(|| left.content_hash.as_str().cmp(right.content_hash.as_str()))
    }
}

/// Flattens a search into the single grid the page draws.
///
/// The backend answers with title matches and author matches separately. They
/// overlap whenever a book matches both, so author matches are appended only
/// where the title list did not already claim the book — otherwise a search for
/// an author who titles books after themselves shows each one twice.
pub(crate) fn merge_matches(result: epub_svelte_lib::library::LibrarySearchResult) -> Vec<BookCardRow> {
    let mut books = result.title_matches;
    let claimed = books.iter().map(|book| book.content_hash.clone()).collect::<std::collections::HashSet<_>>();
    books.extend(result.author_matches.into_iter().filter(|book| !claimed.contains(&book.content_hash)));
    books
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(hash: &str, title: &str, added_at: i64) -> BookCardRow {
        BookCardRow {
            content_hash: epub_svelte_lib::ContentHash::new(&format!("{hash:0>64}")),
            title: title.to_owned(),
            author: String::new(),
            description: String::new(),
            progress: 0.0,
            downloaded: false,
            download_requested: false,
            audiobook: false,
            audiobook_duration_ms: None,
            audiobook_chapter_count: None,
            added_at,
        }
    }

    #[test]
    fn hierarchy_sorts_books_by_name_case_insensitively() {
        let mut books = [book("2", "zebra", 20), book("1", "Alpha", 10)];
        books.sort_by(|left, right| HierarchySort::Name.compare(left, right));
        assert_eq!(books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["Alpha", "zebra"]);
    }
}

pub(crate) trait HierarchyCrumb: Clone + PartialEq + 'static {
    fn hierarchy_name(&self) -> &str;
    fn hierarchy_key(&self) -> String;
    fn hierarchy_book_count(&self) -> usize;
    fn hierarchy_downloaded_book_count(&self) -> usize;
}

impl<Id> HierarchyCrumb for LibraryHierarchyEntry<Id>
where
    Id: Clone + PartialEq + ToString + 'static,
{
    fn hierarchy_name(&self) -> &str {
        &self.name
    }

    fn hierarchy_key(&self) -> String {
        self.id.to_string()
    }

    fn hierarchy_book_count(&self) -> usize {
        self.book_count
    }

    fn hierarchy_downloaded_book_count(&self) -> usize {
        self.downloaded_book_count
    }
}

pub(crate) struct HierarchyState<C> {
    path: Vec<C>,
    children: Rc<Vec<C>>,
    books: BookGrid,
    filter_input: Entity<InputState>,
    /// Results for the query in the box, covering everything beneath the node
    /// being browsed. `None` means no query is active and `books` is showing.
    results: Option<BookGrid>,
    /// Empty children/books are not an empty-state result until the current
    /// browse request has settled.
    loading: bool,
    /// Rejects a slow search that resolves after a newer one.
    search_generation: u64,
    error: Option<gpui::SharedString>,
    hierarchy_columns: Rc<Cell<usize>>,
    sort: HierarchySort,
    file_type: epub_svelte_lib::library::LibraryFileTypeFilter,
    breadcrumb_scroll: ScrollHandle,
}

impl<C: HierarchyCrumb> HierarchyState<C> {
    pub(crate) fn new<P: 'static>(ui: LibraryContext, path: Vec<C>, search_placeholder: Option<&'static str>, on_query_changed: impl Fn(&mut P, &mut Context<P>) + 'static, window: &mut Window, cx: &mut Context<P>) -> (Self, Subscription) {
        let filter_input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            match search_placeholder {
                Some(placeholder) => input.placeholder(placeholder),
                None => input,
            }
        });
        let input_subscription = cx.subscribe(&filter_input, move |page: &mut P, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                on_query_changed(page, cx);
                cx.notify();
            }
        });
        (
            Self {
                path,
                children: Rc::new(Vec::new()),
                books: BookGrid::browsable(ui.clone(), false, ui.library_view()),
                filter_input,
                results: None,
                loading: true,
                search_generation: 0,
                error: None,
                hierarchy_columns: Rc::new(Cell::new(1)),
                sort: HierarchySort::default(),
                file_type: epub_svelte_lib::library::LibraryFileTypeFilter::All,
                breadcrumb_scroll: ScrollHandle::new(),
            },
            input_subscription,
        )
    }

    pub(crate) fn set_library_view<P: 'static>(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<P>) {
        self.books.set_library_view(library_view, cx);
    }

    pub(crate) fn path(&self) -> &[C] {
        &self.path
    }

    pub(crate) fn last(&self) -> Option<&C> {
        self.path.last()
    }

    /// Moves to `path`, clearing the title filter.
    ///
    /// The filter reads as "within what is shown here", so carrying it into a
    /// different folder produces "No matching titles" in a folder that is not
    /// empty, with nothing on screen explaining why.
    pub(crate) fn set_path<P: 'static>(&mut self, path: Vec<C>, window: &mut Window, cx: &mut Context<P>) -> bool {
        if self.path == path {
            return false;
        }
        self.path = path;
        self.filter_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.clear_search();
        true
    }

    pub(crate) fn install<P: 'static>(&mut self, children: Vec<C>, books: Vec<BookCardRow>, cx: &mut Context<P>) {
        let was_loading = self.loading;
        self.loading = false;
        let children_changed = self.children.as_ref() != &children;
        if children_changed {
            self.children = Rc::new(children);
        }
        self.books.set_books(books, cx);
        self.sort.apply(&mut self.books, cx);
        if was_loading || children_changed {
            cx.notify();
        }
    }

    pub(crate) fn begin_load<P: 'static>(&mut self, cx: &mut Context<P>) {
        if !self.loading {
            self.loading = true;
            cx.notify();
        }
    }

    pub(crate) fn finish_load<P: 'static>(&mut self, cx: &mut Context<P>) {
        if self.loading {
            self.loading = false;
            cx.notify();
        }
    }

    pub(crate) fn set_book_management<P: 'static>(&mut self, management: Option<BookManagement>, cx: &mut Context<P>) {
        self.books.set_management(management, cx);
    }

    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut gpui::App) {
        self.filter_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn query(&self, cx: &gpui::App) -> String {
        normalize_filter(self.filter_input.read(cx).value().as_ref())
    }

    pub(crate) fn clear_error(&mut self) {
        self.error = None;
    }

    pub(crate) fn set_error<P: 'static>(&mut self, error: impl Into<gpui::SharedString>, cx: &mut Context<P>) {
        self.error = Some(error.into());
        cx.notify();
    }

    /// Bumps the search generation and returns it. A result carrying an older
    /// generation is a slow query that lost, and is dropped.
    pub(crate) fn begin_search(&mut self) -> u64 {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_generation
    }

    pub(crate) fn is_current_search(&self, generation: u64) -> bool {
        self.search_generation == generation
    }

    pub(crate) fn set_results<P: 'static>(&mut self, books: Vec<BookCardRow>, ui: &LibraryContext, cx: &mut Context<P>) {
        let mut grid = self.results.take().unwrap_or_else(|| BookGrid::browsable(ui.clone(), false, ui.library_view()));
        grid.set_books(books, cx);
        self.sort.apply(&mut grid, cx);
        self.results = Some(grid);
        cx.notify();
    }

    pub(crate) fn file_type(&self) -> epub_svelte_lib::library::LibraryFileTypeFilter {
        self.file_type
    }

    pub(crate) fn set_file_type<P: 'static>(&mut self, file_type: epub_svelte_lib::library::LibraryFileTypeFilter, cx: &mut Context<P>) -> bool {
        if self.file_type == file_type {
            return false;
        }
        self.file_type = file_type;
        self.clear_search();
        cx.notify();
        true
    }

    /// Abandons any query in flight and the results it would have produced.
    pub(crate) fn clear_search(&mut self) {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.results = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render<P, SetPath, Retry>(
        &self, root_label: &'static str, hierarchy_icon: app_contract::NavigationIcon, button_prefix: &'static str, grid_id: &'static str, retry_label: gpui::SharedString, searching_label: gpui::SharedString, breadcrumb_actions: gpui::Div,
        control_actions: gpui::Div, empty: gpui::AnyElement, loading_label: gpui::SharedString, page: Entity<P>, set_path: SetPath, retry: Retry, child_action: Option<Rc<dyn Fn(&C, Entity<P>, &mut Context<P>) -> AnyElement>>,
        child_menu: Option<Rc<dyn Fn(PopupMenu, C, Entity<P>, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>>, child_override: Option<Rc<dyn Fn(&C, Entity<P>, f32, &mut Context<P>) -> Option<AnyElement>>>, show_books: bool,
        lead: Option<AnyElement>, footer: Option<AnyElement>, child_drag: Option<Rc<dyn Fn(&C) -> LibraryDrag>>, child_can_drop: Option<Rc<dyn Fn(&C, &LibraryDrag, Entity<P>, &mut gpui::App) -> bool>>,
        child_drop: Option<Rc<dyn Fn(&C, &LibraryDrag, Entity<P>, &mut gpui::App)>>, window: &mut Window, cx: &mut Context<P>,
    ) -> gpui::AnyElement
    where
        P: Render + 'static,
        SetPath: Fn(&mut P, Vec<C>, &mut Window, &mut Context<P>) + Clone + 'static,
        Retry: Fn(&mut P, &mut Context<P>) + 'static,
    {
        let theme = components::browser_theme(cx);
        let header = self.render_header(root_label, breadcrumb_actions, control_actions, show_books, page.clone(), set_path.clone(), cx);
        let query = self.query(cx);
        let has_lead = lead.is_some();
        // A virtual list owns the remaining viewport and its own scroll model.
        // Content appended after it can therefore never become part of that
        // scroll range. A footer still requires one outer document scroller,
        // but a bounded lead can sit above the virtual list without expanding
        // the book grid. This matters for Uncategorized, where the ISBN review
        // panel must not cause hundreds of retained book cards to render.
        let document_scroll = query.is_empty() && footer.is_some();

        let mut dashboard = components::folder_dashboard();
        if query.is_empty() && self.loading {
            dashboard = dashboard.child(components::browser_loading_message(loading_label, theme));
        } else if !show_books {
            dashboard = match lead {
                Some(lead) => dashboard.child(div().size_full().min_h_0().flex().flex_col().child(lead)),
                None => dashboard.child(empty),
            };
        } else if query.is_empty() {
            // No query: the page is the node's own children and its own books.
            if self.children.is_empty() && self.books.is_empty() && !has_lead {
                dashboard = dashboard.child(empty);
            } else if !self.children.is_empty() && !has_lead && !document_scroll {
                dashboard = dashboard.child(self.render_virtualized_hierarchy(
                    grid_id,
                    button_prefix,
                    hierarchy_icon,
                    page.clone(),
                    set_path.clone(),
                    child_action.clone(),
                    child_menu.clone(),
                    child_override.clone(),
                    child_drag.clone(),
                    child_can_drop.clone(),
                    child_drop.clone(),
                ));
            } else {
                if !self.children.is_empty() {
                    let hierarchy_measure = crate::virtual_grid::ResponsiveColumns::for_element(
                        format!("{grid_id}-hierarchy"),
                        crate::virtual_grid::ColumnPolicy::MinimumItemWidth { width: components::hierarchy_cell_min_width(f32::from(window.rem_size())) + components::HIERARCHY_CELL_MARGIN * 2.0, gap: 0.0 },
                        window,
                        cx,
                    );
                    let rem_size = f32::from(window.rem_size());
                    let hierarchy_width = (hierarchy_measure.width(cx) - components::HIERARCHY_LAYOUT_SAFETY).max(components::hierarchy_cell_min_width(rem_size));
                    let (columns, cell_width) = components::hierarchy_grid_layout(hierarchy_width, self.children.len(), self.hierarchy_columns.get(), rem_size);
                    self.hierarchy_columns.set(columns);
                    let mut cells = self.children.iter().map(|child| {
                        Self::render_child(
                            &self.path,
                            child,
                            button_prefix,
                            hierarchy_icon,
                            page.clone(),
                            set_path.clone(),
                            child_action.clone(),
                            child_menu.clone(),
                            child_override.clone(),
                            child_drag.clone(),
                            child_can_drop.clone(),
                            child_drop.clone(),
                            cell_width,
                            cx,
                        )
                    });
                    let mut rows = Vec::new();
                    loop {
                        let row = cells.by_ref().take(columns).collect::<Vec<_>>();
                        if row.is_empty() {
                            break;
                        }
                        rows.push(components::hierarchy_grid_row().children(row));
                    }
                    dashboard = dashboard.child(hierarchy_measure.track(components::hierarchy_grid(theme).children(rows)));
                }
                if let Some(lead) = lead {
                    dashboard = dashboard.child(div().w_full().flex_none().child(lead));
                }
                dashboard = dashboard.child(if document_scroll { self.books.render_static(format!("{grid_id}-static"), window, cx) } else { self.books.render_virtualized(grid_id, window, cx) });
            }
            if let Some(footer) = footer {
                dashboard = dashboard.child(footer);
            }
        } else {
            // A query covers everything beneath this node, so the child list no
            // longer describes what is on screen and is not drawn.
            match &self.results {
                // The result grid must not reuse the ordinary page grid's
                // identity. GPUI retains virtual-list state by id; sharing it
                // allowed the empty/loading transition to leave a populated
                // result set attached to a stale zero-row list.
                Some(results) if !results.is_empty() => {
                    dashboard = dashboard.child(results.render_virtualized(format!("{grid_id}-search-{}", self.search_generation), window, cx));
                }
                Some(_) => dashboard = dashboard.child(components::browser_inline_note("No matching books", theme)),
                None => dashboard = dashboard.child(components::browser_loading_message(searching_label, theme)),
            }
        }

        let dashboard = if document_scroll { dashboard.overflow_y_scrollbar_with_id(format!("{grid_id}-document-scroll")).into_any_element() } else { dashboard.into_any_element() };
        let retry_page = page;
        let body = components::browser_error_banner(self.error.clone(), retry_label, theme, dashboard, move |cx| {
            retry_page.update(cx, |page, cx| retry(page, cx));
        });
        components::library_section_content().child(header).child(body).into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_virtualized_hierarchy<P, SetPath>(
        &self, grid_id: &'static str, button_prefix: &'static str, hierarchy_icon: app_contract::NavigationIcon, page: Entity<P>, set_path: SetPath, child_action: Option<Rc<dyn Fn(&C, Entity<P>, &mut Context<P>) -> AnyElement>>,
        child_menu: Option<Rc<dyn Fn(PopupMenu, C, Entity<P>, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>>, child_override: Option<Rc<dyn Fn(&C, Entity<P>, f32, &mut Context<P>) -> Option<AnyElement>>>,
        child_drag: Option<Rc<dyn Fn(&C) -> LibraryDrag>>, child_can_drop: Option<Rc<dyn Fn(&C, &LibraryDrag, Entity<P>, &mut gpui::App) -> bool>>, child_drop: Option<Rc<dyn Fn(&C, &LibraryDrag, Entity<P>, &mut gpui::App)>>,
    ) -> AnyElement
    where
        P: Render + 'static,
        SetPath: Fn(&mut P, Vec<C>, &mut Window, &mut Context<P>) + Clone + 'static,
    {
        let children = self.children.clone();
        let base_path = self.path.clone();
        let hierarchy_columns = self.hierarchy_columns.clone();
        let book_views = self.books.virtual_views();
        let detailed = self.books.uses_detailed_rows();
        let list_id: gpui::SharedString = format!("{grid_id}-with-hierarchy").into();
        let list_page = page.clone();
        let query = container_query(move |available, window, _| {
            let width = f32::from(available.width);
            let rem_size = f32::from(window.rem_size());
            let hierarchy_width = (width - components::HIERARCHY_LAYOUT_SAFETY).max(components::hierarchy_cell_min_width(rem_size));
            let (hierarchy_column_count, cell_width) = components::hierarchy_grid_layout(hierarchy_width, children.len(), hierarchy_columns.get(), rem_size);
            hierarchy_columns.set(hierarchy_column_count);
            let hierarchy_row_count = crate::virtual_grid::row_count(children.len(), hierarchy_column_count);
            let hierarchy_row_height = components::hierarchy_cell_height(rem_size) + components::HIERARCHY_CELL_MARGIN * 2.0;

            let book_column_count = if detailed { 1 } else { crate::virtual_grid::columns_for_width(width, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP) };
            let (book_column_count, book_width, book_column_gap) =
                if detailed { (1, width, 0.0) } else { crate::virtual_grid::grid_geometry(width, book_column_count, book_views.len(), components::BOOK_COVER_WIDTH, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP) };
            let book_row_height = if detailed { components::DETAILED_ROW_HEIGHT } else { components::book_grid_row_height_for_cover_width(book_width, f32::from(window.rem_size())) };
            let book_row_count = crate::virtual_grid::row_count(book_views.len(), book_column_count);
            let mut item_sizes = Vec::with_capacity(hierarchy_row_count + book_row_count);
            item_sizes.extend((0..hierarchy_row_count).map(|row| {
                let trailing_gap = if row + 1 == hierarchy_row_count { components::SPACE_LG } else { 0.0 };
                size(px(width), px(hierarchy_row_height + trailing_gap))
            }));
            item_sizes.extend((0..book_row_count).map(|_| size(px(width), px(book_row_height))));

            let render_children = children.clone();
            let render_books = book_views.clone();
            let render_path = base_path.clone();
            let render_page = page.clone();
            let render_set_path = set_path.clone();
            let render_child_action = child_action.clone();
            let render_child_menu = child_menu.clone();
            let render_child_override = child_override.clone();
            let render_child_drag = child_drag.clone();
            let render_child_can_drop = child_can_drop.clone();
            let render_child_drop = child_drop.clone();
            let list = v_virtual_list(list_page.clone(), list_id.clone(), Rc::new(item_sizes), move |_, range, _, cx| {
                range
                    .map(|row| {
                        if row < hierarchy_row_count {
                            let child_range = crate::virtual_grid::row_range(row, hierarchy_column_count, render_children.len());
                            let cells = render_children[child_range]
                                .iter()
                                .map(|child| {
                                    Self::render_child(
                                        &render_path,
                                        child,
                                        button_prefix,
                                        hierarchy_icon,
                                        render_page.clone(),
                                        render_set_path.clone(),
                                        render_child_action.clone(),
                                        render_child_menu.clone(),
                                        render_child_override.clone(),
                                        render_child_drag.clone(),
                                        render_child_can_drop.clone(),
                                        render_child_drop.clone(),
                                        cell_width,
                                        cx,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let trailing_gap = if row + 1 == hierarchy_row_count { components::SPACE_LG } else { 0.0 };
                            div().w_full().h(px(hierarchy_row_height + trailing_gap)).flex_none().child(components::hierarchy_grid_row().children(cells)).into_any_element()
                        } else {
                            let book_row = row - hierarchy_row_count;
                            if detailed {
                                render_books[book_row].clone().into_any_element()
                            } else {
                                let book_range = crate::virtual_grid::row_range(book_row, book_column_count, render_books.len());
                                crate::virtual_grid::flexible_entity_row(&render_books, book_range, book_column_count, book_width, book_row_height, book_column_gap).into_any_element()
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            });
            div().size_full().min_h_0().flex().flex_col().child(components::sized_virtual_list(list).flex_1().min_h_0())
        });
        div().w_full().flex_1().min_h_0().flex().flex_col().child(query).into_any_element()
    }

    fn render_header<P, SetPath>(&self, root_label: &'static str, split_actions: gpui::Div, control_actions: gpui::Div, show_controls: bool, page: Entity<P>, set_path: SetPath, cx: &mut Context<P>) -> gpui::AnyElement
    where
        P: 'static,
        SetPath: Fn(&mut P, Vec<C>, &mut Window, &mut Context<P>) + Clone + 'static,
    {
        let theme = components::browser_theme(cx);
        let mut breadcrumb = components::folder_breadcrumb();
        if self.path.is_empty() {
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(root_label, true));
        } else {
            let root_page = page.clone();
            let root_set_path = set_path.clone();
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(root_label, false).on_click(move |_, window, cx| {
                root_page.update(cx, |page, cx| root_set_path(page, Vec::new(), window, cx));
            }));
        }
        for (index, crumb) in self.path.iter().enumerate() {
            if index + 1 == self.path.len() {
                breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(crumb.hierarchy_name().to_owned(), true));
            } else {
                let path = self.path[..=index].to_vec();
                let path_page = page.clone();
                let path_setter = set_path.clone();
                breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(crumb.hierarchy_name().to_owned(), false).on_click(move |_, window, cx| {
                    path_page.update(cx, |page, cx| path_setter(page, path.clone(), window, cx));
                }));
            }
        }
        let controls = components::browser_topbar_center().child(components::browser_search_box().child(components::browser_search_input(&self.filter_input)));
        // Actions stay outside the breadcrumb viewport, so a long path can
        // never overlap them. Scrolling the trailing sentinel into view only
        // changes the offset when the left-aligned breadcrumb actually
        // overflows; short paths therefore remain at the left edge.
        self.breadcrumb_scroll.scroll_to_item(1);
        let breadcrumb_viewport = gpui::div()
            .id("hierarchy-breadcrumb-scroll")
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.breadcrumb_scroll)
            .flex()
            .items_center()
            .justify_start()
            .child(breadcrumb.flex_none())
            .child(gpui::div().w(gpui::px(1.0)).h(gpui::px(1.0)).flex_none());
        let breadcrumb_row = gpui::div().w_full().min_w_0().flex().items_center().child(breadcrumb_viewport);
        let control_row = gpui::div().w_full().min_w_0().flex().items_center().gap(gpui::px(components::SPACE_MD)).child(controls.min_w_0().justify_start()).child(control_actions.flex_none()).child(split_actions.flex_none());
        components::browser_stacked_topbar(theme).child(breadcrumb_row).when(show_controls, |header| header.child(control_row)).into_any_element()
    }

    fn render_child<P, SetPath>(
        base_path: &[C], child: &C, button_prefix: &'static str, hierarchy_icon: app_contract::NavigationIcon, page: Entity<P>, set_path: SetPath, child_action: Option<Rc<dyn Fn(&C, Entity<P>, &mut Context<P>) -> AnyElement>>,
        child_menu: Option<Rc<dyn Fn(PopupMenu, C, Entity<P>, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>>, child_override: Option<Rc<dyn Fn(&C, Entity<P>, f32, &mut Context<P>) -> Option<AnyElement>>>,
        child_drag: Option<Rc<dyn Fn(&C) -> LibraryDrag>>, child_can_drop: Option<Rc<dyn Fn(&C, &LibraryDrag, Entity<P>, &mut gpui::App) -> bool>>, child_drop: Option<Rc<dyn Fn(&C, &LibraryDrag, Entity<P>, &mut gpui::App)>>,
        cell_width: f32, cx: &mut Context<P>,
    ) -> gpui::AnyElement
    where
        P: 'static,
        SetPath: Fn(&mut P, Vec<C>, &mut Window, &mut Context<P>) + Clone + 'static,
    {
        if let Some(render) = child_override {
            if let Some(element) = render(child, page.clone(), cell_width, cx) {
                return element;
            }
        }
        let theme = components::browser_theme(cx);
        let child = child.clone();
        let label = child.hierarchy_name().to_owned();
        let count = child.hierarchy_book_count();
        let id = format!("{button_prefix}-{}", child.hierarchy_key());
        let drag_id = format!("{id}-drag");
        let end_action = child_action.map(|render| render(&child, page.clone(), cx));
        let mut path = base_path.to_vec();
        path.push(child.clone());
        let click_page = page.clone();
        let button = components::hierarchy_cell(id, label, count, hierarchy_icon, end_action, cell_width, theme).on_click(move |_, window, cx| {
            click_page.update(cx, |page, cx| set_path(page, path.clone(), window, cx));
        });
        // gpui-component's Button is interactive but not stateful, while GPUI
        // drag sources require a stable element id. The wrapper supplies that
        // identity without replacing the existing button presentation.
        let mut cell = gpui::div().id(drag_id).flex_none().child(button);
        if let Some(make_drag) = child_drag {
            cell = cell.cursor_move().on_drag(make_drag(&child), |item, _, _, cx| cx.new(|_| LibraryDragPreview::new(item.clone())));
        }
        if let Some(can_drop) = child_can_drop {
            let drop_child = child.clone();
            let drop_page = page.clone();
            cell = cell.can_drop(move |value, _, cx| value.downcast_ref::<LibraryDrag>().is_some_and(|drag| can_drop(&drop_child, drag, drop_page.clone(), cx)));
        }
        if let Some(drop) = child_drop {
            let drop_child = child.clone();
            let drop_page = page.clone();
            cell = cell.drag_over::<LibraryDrag>(move |style, _, _, _| style.border_1().border_color(theme.accent).bg(theme.accent_soft)).on_drop(move |drag: &LibraryDrag, _, cx| drop(&drop_child, drag, drop_page.clone(), cx));
        }
        match child_menu {
            Some(render_menu) => {
                let menu_child = child.clone();
                let menu_page = page.clone();
                cell.context_menu(move |menu, window, cx| render_menu(menu, menu_child.clone(), menu_page.clone(), window, cx)).into_any_element()
            }
            None => cell.into_any_element(),
        }
    }
}

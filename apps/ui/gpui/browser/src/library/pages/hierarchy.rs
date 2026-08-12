//! Shared state and presentation for hierarchical library pages.
//!
//! Loading, mutations, empty-state copy, and events remain with each concrete
//! page. This module only owns behavior that is identical for every hierarchy.

use epub_svelte_lib::library::BookCardRow;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Focusable, IntoElement, Subscription, Window};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt, PopupMenu};
use std::cell::Cell;
use std::rc::Rc;
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::title_filter::normalize_filter;
use crate::library::widgets::{BookGrid, BookManagement};
use crate::virtual_grid;
use epub_svelte_lib::library::LibraryHierarchyEntry;

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
    /// Rejects a slow search that resolves after a newer one.
    search_generation: u64,
    error: Option<gpui::SharedString>,
    hierarchy_columns: Cell<usize>,
}

impl<C: HierarchyCrumb> HierarchyState<C> {
    pub(crate) fn new<P: 'static>(ui: LibraryContext, path: Vec<C>, on_query_changed: impl Fn(&mut P, &mut Context<P>) + 'static, window: &mut Window, cx: &mut Context<P>) -> (Self, Subscription) {
        let filter_input = cx.new(|cx| InputState::new(window, cx));
        let input_subscription = cx.subscribe(&filter_input, move |page: &mut P, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                on_query_changed(page, cx);
                cx.notify();
            }
        });
        (Self { path, children: Rc::new(Vec::new()), books: BookGrid::browsable(ui.clone(), false, ui.library_view()), filter_input, results: None, search_generation: 0, error: None, hierarchy_columns: Cell::new(1) }, input_subscription)
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
        let children_changed = self.children.as_ref() != &children;
        if children_changed {
            self.children = Rc::new(children);
        }
        self.books.set_books(books, cx);
        if children_changed {
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
        self.results = Some(grid);
        cx.notify();
    }

    /// Abandons any query in flight and the results it would have produced.
    pub(crate) fn clear_search(&mut self) {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.results = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render<P, SetPath, Retry>(
        &self, root_label: &'static str, button_prefix: &'static str, grid_id: &'static str, retry_label: gpui::SharedString, searching_label: gpui::SharedString, header_actions: gpui::Div, empty: gpui::AnyElement, page: Entity<P>,
        set_path: SetPath, retry: Retry, child_action: Option<Rc<dyn Fn(&C, Entity<P>, &mut Context<P>) -> AnyElement>>, child_menu: Option<Rc<dyn Fn(PopupMenu, C, Entity<P>, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>>,
        child_override: Option<Rc<dyn Fn(&C, Entity<P>, f32, &mut Context<P>) -> Option<AnyElement>>>, window: &Window, cx: &mut Context<P>,
    ) -> gpui::AnyElement
    where
        P: 'static,
        SetPath: Fn(&mut P, Vec<C>, &mut Window, &mut Context<P>) + Clone + 'static,
        Retry: Fn(&mut P, &mut Context<P>) + 'static,
    {
        let theme = components::browser_theme(cx);
        let header = self.render_header(root_label, header_actions, page.clone(), set_path.clone(), cx);
        let query = self.query(cx);

        let mut dashboard = components::folder_dashboard();
        if query.is_empty() {
            // No query: the page is the node's own children and its own books.
            if self.children.is_empty() && self.books.is_empty() {
                dashboard = dashboard.child(empty);
            } else {
                if !self.children.is_empty() {
                    let available_width = f32::from(window.viewport_size().width) - components::nav_rail_width(f32::from(window.rem_size())) - components::CONTENT_PADDING * 2.0;
                    let book_columns = virtual_grid::columns_for_width(available_width, components::BOOK_COVER_WIDTH, components::BOOK_GRID_GAP);
                    let (_, inset) = virtual_grid::even_spacing(available_width, book_columns, components::BOOK_COVER_WIDTH, components::CONTENT_PADDING);
                    let hierarchy_width = (available_width - inset * 2.0 - components::HIERARCHY_LAYOUT_SAFETY).max(components::HIERARCHY_CELL_MIN_WIDTH);
                    let (columns, cell_width) = components::hierarchy_grid_layout(hierarchy_width, self.children.len(), self.hierarchy_columns.get());
                    self.hierarchy_columns.set(columns);
                    let mut cells = self.children.iter().map(|child| self.render_child(child, button_prefix, page.clone(), set_path.clone(), child_action.clone(), child_menu.clone(), child_override.clone(), cell_width, cx));
                    let mut rows = Vec::new();
                    loop {
                        let row = cells.by_ref().take(columns).collect::<Vec<_>>();
                        if row.is_empty() {
                            break;
                        }
                        rows.push(components::hierarchy_grid_row().children(row));
                    }
                    dashboard = dashboard.child(components::hierarchy_grid(theme).px(gpui::px(inset)).children(rows));
                }
                dashboard = dashboard.child(self.books.render_virtualized(grid_id, window));
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
                    dashboard = dashboard.child(results.render_virtualized(format!("{grid_id}-search-{}", self.search_generation), window));
                }
                Some(_) => dashboard = dashboard.child(components::browser_inline_note("No matching books", theme)),
                None => dashboard = dashboard.child(components::browser_loading_message(searching_label, theme)),
            }
        }

        let retry_page = page;
        let body = components::browser_error_banner(self.error.clone(), retry_label, theme, dashboard.into_any_element(), move |cx| {
            retry_page.update(cx, |page, cx| retry(page, cx));
        });
        components::library_section_content().child(header).child(body).into_any_element()
    }

    fn render_header<P, SetPath>(&self, root_label: &'static str, actions: gpui::Div, page: Entity<P>, set_path: SetPath, cx: &mut Context<P>) -> gpui::AnyElement
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
        components::browser_topbar(theme)
            .child(components::browser_topbar_left().child(breadcrumb))
            .child(components::browser_topbar_center().child(components::browser_search_box().child(components::browser_search_input(&self.filter_input))))
            .child(actions)
            .into_any_element()
    }

    fn render_child<P, SetPath>(
        &self, child: &C, button_prefix: &'static str, page: Entity<P>, set_path: SetPath, child_action: Option<Rc<dyn Fn(&C, Entity<P>, &mut Context<P>) -> AnyElement>>,
        child_menu: Option<Rc<dyn Fn(PopupMenu, C, Entity<P>, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>>, child_override: Option<Rc<dyn Fn(&C, Entity<P>, f32, &mut Context<P>) -> Option<AnyElement>>>, cell_width: f32,
        cx: &mut Context<P>,
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
        let end_action = child_action.map(|render| render(&child, page.clone(), cx));
        let mut path = self.path.clone();
        path.push(child.clone());
        let click_page = page.clone();
        let cell = components::hierarchy_cell(id, label, count, end_action, cell_width, theme).on_click(move |_, window, cx| {
            click_page.update(cx, |page, cx| set_path(page, path.clone(), window, cx));
        });
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

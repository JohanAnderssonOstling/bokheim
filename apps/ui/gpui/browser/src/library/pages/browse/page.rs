//! Shared browse-page state and layout for folders, subjects, and the store.

use std::rc::Rc;
use std::time::Duration;

use catalog_model::{CatalogResultRange, CatalogResultRangeResponse};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render, SharedString, Subscription, Task, Window, div, rems};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, IconName};
use library_model::BrowseChipSort;
use ui_components as components;

use super::adapter::{BrowseAdapter, BrowseCrumb, BrowseQuery, BrowseRequest};
use super::controls::{BrowseBookOrder, BrowseFormatFilter, BrowseOptionsEvent, BrowseOptionsSelection, SearchField, browse_options_control};
use super::folder_actions::{FolderActions, FolderActionsEvent};
use crate::DismissSettings;
use crate::library::services::{LatestRequest, LibraryContentsChanged, LibraryUpdateBridge};
use crate::library::widgets::{Paginator, PaginatorAxis, PaginatorGroup};

fn previous_result_window(range: CatalogResultRangeResponse, limit: u32) -> Option<u32> {
    (range.offset > 0).then(|| range.offset.saturating_sub(limit))
}

fn next_result_window(range: CatalogResultRangeResponse) -> Option<u32> {
    let next = range.offset.saturating_add(range.returned_items);
    (next < range.total_items).then_some(next)
}

type PersistBrowsePath = Rc<dyn Fn(&[BrowseCrumb])>;

#[derive(Clone)]
struct BrowseSelection {
    search: String,
    formats: Vec<BrowseFormatFilter>,
    chip_sort: Option<BrowseChipSort>,
    book_sort: Option<BrowseBookOrder>,
    languages: Vec<&'static str>,
}

impl BrowseSelection {
    fn new<A: BrowseAdapter>(adapter: &A) -> Self {
        Self { search: String::new(), formats: Vec::new(), chip_sort: adapter.controls().collection_sorts.first().copied(), book_sort: adapter.controls().book_sorts.first().copied(), languages: Vec::new() }
    }

    fn query(&self, location: String, range: CatalogResultRange) -> BrowseQuery {
        BrowseQuery { location, search: self.search.clone(), formats: self.formats.clone(), chip_sort: self.chip_sort, book_sort: self.book_sort, languages: self.languages.clone(), range }
    }

    fn options(&self) -> BrowseOptionsSelection {
        BrowseOptionsSelection::new(self.chip_sort, self.book_sort, self.formats.clone(), self.languages.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadIntent {
    Initial,
    Reload,
    Navigate,
    /// Keeps the current frame painted while a child request completes.
    NavigateInPlace,
    Refresh,
    PreviousWindow {
        offset: u32,
    },
    NextWindow {
        offset: u32,
    },
}

impl LoadIntent {
    fn offset(self) -> u32 {
        match self {
            Self::PreviousWindow { offset } | Self::NextWindow { offset } => offset,
            _ => 0,
        }
    }

    fn persists_path(self) -> bool {
        matches!(self, Self::Navigate | Self::NavigateInPlace)
    }

    fn reveals_end(self) -> bool {
        matches!(self, Self::PreviousWindow { .. })
    }

    fn invalidates_while_loading(self) -> bool {
        !matches!(self, Self::NavigateInPlace | Self::Refresh)
    }
}

#[derive(Clone)]
struct PendingLoad {
    location: String,
    path: Vec<BrowseCrumb>,
    intent: LoadIntent,
}

pub(super) struct BrowsePageConfig {
    root_location: String,
    root_label: SharedString,
    path: Vec<BrowseCrumb>,
    list_prefix: &'static str,
    changes: Option<Entity<LibraryUpdateBridge>>,
    persist_path: PersistBrowsePath,
    folder_actions: Option<Entity<FolderActions>>,
}

impl BrowsePageConfig {
    pub(super) fn new(root_location: impl Into<String>, root_label: impl Into<SharedString>, list_prefix: &'static str) -> Self {
        Self { root_location: root_location.into(), root_label: root_label.into(), path: Vec::new(), list_prefix, changes: None, persist_path: Rc::new(|_| {}), folder_actions: None }
    }

    pub(super) fn with_path(mut self, path: Vec<BrowseCrumb>) -> Self {
        self.path = path;
        self
    }

    pub(super) fn with_changes(mut self, changes: Entity<LibraryUpdateBridge>) -> Self {
        self.changes = Some(changes);
        self
    }

    pub(super) fn with_persist_path(mut self, persist_path: impl Fn(&[BrowseCrumb]) + 'static) -> Self {
        self.persist_path = Rc::new(persist_path);
        self
    }

    pub(super) fn with_folder_actions(mut self, folder_actions: Entity<FolderActions>) -> Self {
        self.folder_actions = Some(folder_actions);
        self
    }
}

pub(crate) struct BrowsePage<A: BrowseAdapter> {
    root_location: String,
    root_label: SharedString,
    location: String,
    path: Vec<BrowseCrumb>,
    list_prefix: &'static str,
    adapter: A,
    paginator: Entity<Paginator>,
    focus: FocusHandle,
    persist_path: PersistBrowsePath,
    search_field: Option<SearchField>,
    search_open: bool,
    selection: BrowseSelection,
    loading: bool,
    error: Option<SharedString>,
    request_generation: u64,
    request: Option<Task<()>>,
    search_debounce: Option<Task<()>>,
    folder_actions: Option<Entity<FolderActions>>,
    _changes: Option<Subscription>,
    _search_subscription: Option<Subscription>,
    _folder_actions_subscription: Option<Subscription>,
}

impl<A: BrowseAdapter> BrowsePage<A> {
    pub(super) fn new(adapter: A, config: BrowsePageConfig, cx: &mut Context<Self>) -> Self {
        let location = config.path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| config.root_location.clone());
        let selection = BrowseSelection::new(&adapter);
        let paginator = cx.new(|_| Paginator::new(format!("{}-browse-paginator", config.list_prefix), []).flow_axis(PaginatorAxis::Vertical));
        let mut page = Self {
            root_location: config.root_location,
            root_label: config.root_label,
            location,
            path: config.path,
            list_prefix: config.list_prefix,
            adapter,
            paginator,
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            persist_path: config.persist_path,
            search_field: None,
            search_open: false,
            selection,
            loading: false,
            error: None,
            request_generation: 0,
            request: None,
            search_debounce: None,
            folder_actions: config.folder_actions,
            _changes: None,
            _search_subscription: None,
            _folder_actions_subscription: None,
        };
        page.load(page.path.clone(), LoadIntent::Initial, cx);
        page._changes = config.changes.map(|changes| cx.subscribe(&changes, |page, _, _: &LibraryContentsChanged, cx| page.refresh_contents(cx)));
        page._folder_actions_subscription = page.folder_actions.clone().map(|actions| cx.subscribe(&actions, |page, _, event, cx| page.handle_folder_action(event.clone(), cx)));
        page
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(self.path.clone(), LoadIntent::Reload, cx);
    }

    /// Refreshes scanner-driven content without replacing the current page by
    /// a loading frame. Newly discovered covers can prepare in place while the
    /// already-visible book cards remain painted.
    fn refresh_contents(&mut self, cx: &mut Context<Self>) {
        self.load(self.path.clone(), LoadIntent::Refresh, cx);
    }

    fn handle_folder_action(&mut self, event: FolderActionsEvent, cx: &mut Context<Self>) {
        match event {
            FolderActionsEvent::Reload => self.reload(cx),
            FolderActionsEvent::Renamed(name) => {
                if let Some(crumb) = self.path.last_mut() {
                    crumb.label = name;
                }
                (self.persist_path)(&self.path);
                self.reload(cx);
            }
            FolderActionsEvent::OpenRoot => self.open_path(Vec::new(), cx),
        }
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        let Some(search_field) = &self.search_field else {
            return;
        };
        let search = search_field.query(cx);
        if self.selection.search == search {
            return;
        }
        self.selection.search = search;
        self.search_debounce.take();
        if !self.selection.search.is_empty() && self.selection.search.chars().count() < A::minimum_search_length() {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_millis(250));
        self.search_debounce = Some(cx.spawn(async move |page, cx| {
            timer.await;
            let _ = page.update(cx, |page, cx| {
                page.search_debounce.take();
                page.reload(cx);
            });
        }));
    }

    fn set_format(&mut self, format: BrowseFormatFilter, cx: &mut Context<Self>) {
        if !self.adapter.controls().formats.contains(&format) {
            return;
        }
        if format == BrowseFormatFilter::Any {
            if self.selection.formats.is_empty() {
                return;
            }
            self.selection.formats.clear();
        } else if let Some(index) = self.selection.formats.iter().position(|selected| *selected == format) {
            self.selection.formats.remove(index);
        } else {
            self.selection.formats.push(format);
        }
        self.reload(cx);
    }

    fn set_chip_sort(&mut self, chip_sort: BrowseChipSort, cx: &mut Context<Self>) {
        if !self.adapter.controls().collection_sorts.contains(&chip_sort) || self.selection.chip_sort == Some(chip_sort) {
            return;
        }
        self.selection.chip_sort = Some(chip_sort);
        self.reload(cx);
    }

    fn set_book_sort(&mut self, book_sort: BrowseBookOrder, cx: &mut Context<Self>) {
        if !self.adapter.controls().book_sorts.contains(&book_sort) || self.selection.book_sort == Some(book_sort) {
            return;
        }
        self.selection.book_sort = Some(book_sort);
        self.reload(cx);
    }

    fn set_language(&mut self, language: Option<&'static str>, cx: &mut Context<Self>) {
        if language.is_some_and(|language| !self.adapter.controls().languages.contains(&language)) {
            return;
        }
        match language {
            None => {
                if self.selection.languages.is_empty() {
                    return;
                }
                self.selection.languages.clear();
            }
            Some(language) => {
                if let Some(index) = self.selection.languages.iter().position(|selected| *selected == language) {
                    self.selection.languages.remove(index);
                } else {
                    self.selection.languages.push(language);
                }
            }
        }
        self.reload(cx);
    }

    fn select_option(&mut self, event: BrowseOptionsEvent, cx: &mut Context<Self>) {
        match event {
            BrowseOptionsEvent::SelectChipSort(sort) => self.set_chip_sort(sort, cx),
            BrowseOptionsEvent::SelectBookSort(sort) => self.set_book_sort(sort, cx),
            BrowseOptionsEvent::ToggleFormat(format) => self.set_format(format, cx),
            BrowseOptionsEvent::ToggleLanguage(language) => self.set_language(language, cx),
        }
    }

    fn ensure_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_field.is_some() {
            return;
        }
        let (search_field, subscription) = SearchField::new(Some("Search books or authors"), |page: &mut Self, cx| page.search_changed(cx), window, cx);
        self.search_field = Some(search_field);
        self._search_subscription = Some(subscription);
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_search_field(window, cx);
        self.search_open = true;
        if let Some(search) = &self.search_field {
            search.focus(window, cx);
        }
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    fn previous_page(&mut self, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        if self.paginator.update(cx, |paginator, cx| paginator.previous_with_axis(axis, cx)) {
            return true;
        }
        let Some(range) = self.adapter.result_range() else { return false };
        let Some(offset) = previous_result_window(range, A::result_window_limit().unwrap_or(CatalogResultRange::DEFAULT_LIMIT)) else { return false };
        if !self.paginator.read(cx).is_idle() {
            return false;
        }
        self.load(self.path.clone(), LoadIntent::PreviousWindow { offset }, cx);
        true
    }

    fn next_page(&mut self, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        if self.paginator.update(cx, |paginator, cx| paginator.next_with_axis(axis, cx)) {
            return true;
        }
        let Some(range) = self.adapter.result_range() else { return false };
        let Some(next) = next_result_window(range) else { return false };
        if !self.paginator.read(cx).is_idle() {
            return false;
        }
        self.load(self.path.clone(), LoadIntent::NextWindow { offset: next }, cx);
        true
    }

    fn navigate_paginator(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open && event.keystroke.key == "escape" {
            self.close_search(window, cx);
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }

        let search_focused = self.search_field.as_ref().is_some_and(|search| search.is_focused(window, cx));
        let handled = match event.keystroke.key.as_str() {
            "left" if !search_focused => self.previous_page(PaginatorAxis::Horizontal, cx),
            "right" if !search_focused => self.next_page(PaginatorAxis::Horizontal, cx),
            "pageup" => self.previous_page(PaginatorAxis::Vertical, cx),
            "pagedown" => self.next_page(PaginatorAxis::Vertical, cx),
            _ => false,
        };

        if handled {
            cx.stop_propagation();
        }
    }

    fn on_dismiss(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(actions) = &self.folder_actions {
            if actions.read(cx).has_modal() {
                actions.update(cx, |actions, cx| actions.dismiss_modal(cx));
                return;
            }
        }
        if self.search_open {
            self.close_search(window, cx);
            return;
        }
        if self.path.is_empty() {
            cx.propagate();
            return;
        }

        let mut parent = self.path.clone();
        parent.pop();
        self.open_path(parent, cx);
    }

    pub(super) fn open_child(&mut self, child_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(child) = self.adapter.children().iter().find(|child| child.id == child_id).cloned() else {
            log::warn!("ignored missing {} child {child_id}", self.list_prefix);
            return;
        };
        let direct_path = || {
            let mut path = self.path.clone();
            path.push(BrowseCrumb { location: child.id.clone(), label: child.name.clone() });
            path
        };
        let path = if self.selection.search.is_empty() {
            direct_path()
        } else {
            match self.adapter.path_to(&child) {
                Ok(Some(path)) => path,
                Ok(None) => direct_path(),
                Err(error) => {
                    log::warn!("failed to resolve full {} path for {}: {error}", self.list_prefix, child.id);
                    direct_path()
                }
            }
        };
        if !self.selection.search.is_empty() {
            self.selection.search.clear();
            if let Some(search) = &self.search_field {
                search.clear(window, cx);
            }
        }
        self.load(path, LoadIntent::NavigateInPlace, cx);
    }

    fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.load(path, LoadIntent::Navigate, cx);
    }

    fn load(&mut self, path: Vec<BrowseCrumb>, intent: LoadIntent, cx: &mut Context<Self>) {
        let location = path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| self.root_location.clone());
        if location != self.location {
            if let Some(actions) = &self.folder_actions {
                actions.update(cx, |actions, cx| actions.cancel_move_selection(cx));
            }
        }
        let limit = A::result_window_limit().unwrap_or(CatalogResultRange::DEFAULT_LIMIT);
        let query = self.selection.query(location.clone(), CatalogResultRange::new(intent.offset(), limit));
        let pending = PendingLoad { location, path, intent };
        let invalidates_while_loading = intent.invalidates_while_loading();
        self.request.take();
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        match self.adapter.request(&query) {
            BrowseRequest::Ready(response) => self.finish_request(generation, pending, Ok(response), cx),
            BrowseRequest::Async(operation) => {
                self.loading = invalidates_while_loading;
                self.error = None;
                self.request = Some(cx.spawn(async move |page, cx| {
                    let result = operation.await;
                    let _ = page.update(cx, |page, cx| page.finish_request(generation, pending, result, cx));
                }));
                if invalidates_while_loading {
                    cx.notify();
                }
            }
        }
    }

    fn finish_request(&mut self, generation: u64, pending: PendingLoad, result: Result<A::Response, String>, cx: &mut Context<Self>) {
        if generation != self.request_generation {
            return;
        }
        self.request.take();
        self.loading = false;
        match result {
            Ok(response) => {
                self.adapter.install(response, cx);
                self.location = pending.location;
                self.path = pending.path;
                self.error = None;
                if pending.intent.persists_path() {
                    (self.persist_path)(&self.path);
                }
                if let Some(actions) = &self.folder_actions {
                    let name = self.path.last().map(|crumb| crumb.label.clone());
                    actions.update(cx, |actions, cx| actions.set_location(&self.location, name, cx));
                }
                let groups = self.paginator_groups(pending.intent.invalidates_while_loading(), cx);
                self.paginator.update(cx, |paginator, cx| {
                    paginator.set_groups(groups, cx);
                    if pending.intent.reveals_end() {
                        paginator.show_end(cx);
                    }
                });
            }
            Err(error) => self.error = Some(error.into()),
        }
        cx.notify();
    }

    fn breadcrumb(&self, cx: &mut Context<Self>) -> AnyElement {
        let page = cx.entity();
        let mut breadcrumb = components::folder_breadcrumb();
        if self.path.is_empty() {
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(self.root_label.clone(), true));
        } else {
            let root_page = page.clone();
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(self.root_label.clone(), false).on_click(move |_, _, cx| {
                root_page.update(cx, |page, cx| page.open_path(Vec::new(), cx));
            }));
        }
        for (index, crumb) in self.path.iter().enumerate() {
            let current = index + 1 == self.path.len();
            let item = components::folder_breadcrumb_item(crumb.label.clone(), current);
            if current {
                breadcrumb = breadcrumb.child(item);
            } else {
                let path = self.path[..=index].to_vec();
                let path_page = page.clone();
                breadcrumb = breadcrumb.child(item.on_click(move |_, _, cx| {
                    path_page.update(cx, |page, cx| page.open_path(path.clone(), cx));
                }));
            }
        }
        div().w_full().min_w_0().overflow_x_scrollbar().flex().items_center().justify_start().child(breadcrumb.flex_none()).child(div().w(gpui::px(1.0)).h(gpui::px(1.0)).flex_none()).into_any_element()
    }

    fn paginator_groups(&self, wait_for_book_readiness: bool, cx: &mut Context<Self>) -> Vec<PaginatorGroup> {
        super::layout::paginator_groups::<A>(cx.entity().downgrade(), self.adapter.children(), self.adapter.books(), wait_for_book_readiness)
    }

    fn render_topbar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let search_page = page.clone();
        let search_button_label = if self.search_open { "Hide search" } else { "Search books or authors" };
        let paginator = self.paginator.read(cx);
        let remote_range = self.adapter.result_range();
        let idle = paginator.is_idle();
        let has_previous = paginator.has_previous() || remote_range.is_some_and(|range| range.offset > 0);
        let has_next = paginator.has_next() || remote_range.is_some_and(|range| range.offset.saturating_add(range.returned_items) < range.total_items);
        let can_previous = paginator.can_previous() || (idle && remote_range.is_some_and(|range| range.offset > 0));
        let can_next = paginator.can_next() || (idle && remote_range.is_some_and(|range| range.offset.saturating_add(range.returned_items) < range.total_items));
        let browse_controls = self.adapter.controls().clone();
        let has_options = !browse_controls.collection_sorts.is_empty() || !browse_controls.book_sorts.is_empty() || !browse_controls.formats.is_empty() || !browse_controls.languages.is_empty();
        let pagination = (!cx.supports_touch_input() && (has_previous || has_next)).then(|| {
            let previous_page = page.clone();
            let previous = has_previous.then(|| {
                components::topbar_icon_action_button(format!("{}-previous", self.list_prefix), "Previous", IconName::ChevronLeft, theme)
                    .disabled(!can_previous)
                    .on_click(move |_, _, cx| {
                        previous_page.update(cx, |page, cx| page.previous_page(PaginatorAxis::Horizontal, cx));
                    })
                    .into_any_element()
            });
            let next_page = page.clone();
            let next = has_next.then(|| {
                components::topbar_icon_action_button(format!("{}-next", self.list_prefix), "Next", IconName::ChevronRight, theme)
                    .disabled(!can_next)
                    .on_click(move |_, _, cx| {
                        next_page.update(cx, |page, cx| page.next_page(PaginatorAxis::Horizontal, cx));
                    })
                    .into_any_element()
            });
            components::paginator_controls(previous, next)
        });
        let controls = components::browser_topbar_right()
            .when(self.search_open, |controls| controls.flex_1())
            .children(self.search_open.then(|| div().flex_1().min_w(rems(8.0)).child(self.search_field.as_ref().expect("search field initialized above").render())))
            .children(self.folder_actions.as_ref().map(|actions| FolderActions::render_trigger(actions.clone(), cx)))
            .child(
                components::topbar_icon_action_button("toggle-browse-search", search_button_label, IconName::Search, theme)
                    .when(self.search_open || !self.selection.search.is_empty(), |button| button.bg(theme.accent_soft))
                    .on_click(move |_, window, cx| search_page.update(cx, |page, cx| page.toggle_search(window, cx))),
            )
            .when(has_options, |controls| {
                controls.child(browse_options_control(page.clone(), browse_controls, self.selection.options(), |page: &mut Self, event, cx| page.select_option(event, cx), window.viewport_size().height, window.rem_size(), theme))
            })
            .children(pagination);
        components::browser_topbar(theme).child(components::browser_topbar_left().child(self.breadcrumb(cx))).child(controls).into_any_element()
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let paginator = self.paginator.read(cx);
        let paginator_preparing = paginator.is_preparing();
        let scanning_empty_library = paginator.is_empty() && self.folder_actions.as_ref().is_some_and(|actions| actions.read(cx).scanner_running());
        let show_loading = (self.loading || paginator_preparing || scanning_empty_library) && self.error.is_none();
        let loading_label = if scanning_empty_library { "Scanning library…" } else { "Loading…" };
        let body = div()
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(self.paginator.clone())
            .when(show_loading, |body| body.child(components::absolute_full_size().bg(theme.surface).flex().items_center().justify_center().child(components::browser_loading_message(loading_label, theme))))
            .into_any_element();
        if let Some(error) = self.error.clone() {
            let retry_page = page.clone();
            let retry = components::topbar_action_button("retry-section", "Retry", IconName::Redo2, theme).on_click(move |_, _, cx| retry_page.update(cx, |page, cx| page.reload(cx)));
            components::browser_error_banner(error, retry, body).into_any_element()
        } else {
            body
        }
    }

    fn render_shell(&self, topbar: AnyElement, body: AnyElement, cx: &mut Context<Self>) -> gpui::Div {
        let folder_overlay = self.folder_actions.as_ref().and_then(|actions| FolderActions::render_overlay(actions.clone(), cx));
        components::full_size_column()
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_dismiss))
            .on_key_down(cx.listener(Self::navigate_paginator))
            .child(topbar)
            .child(components::remaining_space_column().child(body))
            .children(folder_overlay)
    }
}

impl<A: BrowseAdapter> Render for BrowsePage<A> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(body) = self.adapter.custom_body() {
            let theme = components::browser_theme(cx);
            let topbar = components::browser_topbar(theme).child(components::browser_topbar_left().child(self.breadcrumb(cx))).into_any_element();
            self.render_shell(topbar, body, cx)
        } else {
            let topbar = self.render_topbar(window, cx);
            let body = self.render_body(cx);
            self.render_shell(topbar, body, cx)
        }
    }
}

impl<A: BrowseAdapter> gpui::Focusable for BrowsePage<A> {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_windows_preserve_remote_navigation() {
        let range = CatalogResultRangeResponse { offset: 32, returned_items: 32, total_items: 1_000 };
        assert_eq!(previous_result_window(range, 32), Some(0));
        assert_eq!(next_result_window(range), Some(64));
    }

    #[test]
    fn final_result_window_has_no_remote_successor() {
        let range = CatalogResultRangeResponse { offset: 992, returned_items: 8, total_items: 1_000 };
        assert_eq!(next_result_window(range), None);
    }

    #[test]
    fn load_intents_make_navigation_behavior_explicit() {
        assert!(LoadIntent::Navigate.persists_path());
        assert!(LoadIntent::Navigate.invalidates_while_loading());
        assert!(LoadIntent::NavigateInPlace.persists_path());
        assert!(!LoadIntent::NavigateInPlace.invalidates_while_loading());
        assert!(!LoadIntent::Refresh.persists_path());
        assert!(!LoadIntent::Refresh.invalidates_while_loading());

        let previous = LoadIntent::PreviousWindow { offset: 32 };
        assert_eq!(previous.offset(), 32);
        assert!(previous.reveals_end());
        let next = LoadIntent::NextWindow { offset: 64 };
        assert_eq!(next.offset(), 64);
        assert!(!next.reveals_end());
    }
}

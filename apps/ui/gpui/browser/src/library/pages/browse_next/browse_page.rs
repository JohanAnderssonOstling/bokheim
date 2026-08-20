//! Shared browse-page state and layout for folders, subjects, and the store.

use std::rc::Rc;

use epub_svelte_lib::BrowseRow;
use epub_svelte_lib::app::executor::BackendTask;
use epub_svelte_lib::library::LibraryFileTypeFilter;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Render, SharedString, Subscription, Task, Window, div, px, rems};
use gpui_component::scroll::ScrollableElement;
use ui_components as components;

use super::super::browse::{SearchField, file_type_filter_control};
use super::super::browse_grid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseQuery {
    pub(crate) location: String,
    pub(crate) search: String,
    pub(crate) file_type: Option<LibraryFileTypeFilter>,
}

pub(crate) enum BrowseRequest<T> {
    Ready(Result<T, String>),
    Async(BackendTask<Result<T, String>>),
}

pub(crate) trait BrowseAdapter: Sized + 'static {
    type Response: 'static;
    type Book: Render + 'static;

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response>;
    fn install(&mut self, response: Self::Response, cx: &mut Context<BrowsePage<Self>>);
    fn children(&self) -> Rc<Vec<BrowseRow>>;
    fn books(&self) -> Rc<Vec<(Entity<Self::Book>, usize)>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseCrumb {
    pub(crate) location: String,
    pub(crate) label: String,
}

pub(crate) struct BrowsePage<A: BrowseAdapter> {
    root_location: String,
    root_label: SharedString,
    location: String,
    path: Vec<BrowseCrumb>,
    list_prefix: &'static str,
    adapter: A,
    persist_path: Rc<dyn Fn(&[BrowseCrumb])>,
    search_field: Option<SearchField>,
    search: String,
    file_type: Option<LibraryFileTypeFilter>,
    loading: bool,
    error: Option<SharedString>,
    request_generation: u64,
    request: Option<Task<()>>,
    _changes: Option<Task<()>>,
    _search_subscription: Option<Subscription>,
}

impl<A: BrowseAdapter> BrowsePage<A> {
    pub(super) fn new(
        adapter: A, root_location: String, root_label: impl Into<SharedString>, path: Vec<BrowseCrumb>, list_prefix: &'static str, file_type: Option<LibraryFileTypeFilter>, changes: Option<async_channel::Receiver<()>>,
        persist_path: impl Fn(&[BrowseCrumb]) + 'static, cx: &mut Context<Self>,
    ) -> Self {
        let location = path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| root_location.clone());
        let mut page = Self {
            root_location,
            root_label: root_label.into(),
            location,
            path,
            list_prefix,
            adapter,
            persist_path: Rc::new(persist_path),
            search_field: None,
            search: String::new(),
            file_type,
            loading: false,
            error: None,
            request_generation: 0,
            request: None,
            _changes: None,
            _search_subscription: None,
        };
        page.load_path(page.path.clone(), false, cx);
        page._changes = changes.map(|changes| {
            cx.spawn(async move |page, cx| {
                while changes.recv().await.is_ok() {
                    let _ = page.update(cx, |page, cx| page.reload(cx));
                }
            })
        });
        page
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.load_path(self.path.clone(), false, cx);
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        let Some(search_field) = &self.search_field else {
            return;
        };
        let search = search_field.query(cx);
        if self.search == search {
            return;
        }
        self.search = search;
        self.reload(cx);
    }

    fn set_file_type(&mut self, file_type: LibraryFileTypeFilter, cx: &mut Context<Self>) {
        if self.file_type == Some(file_type) {
            return;
        }
        self.file_type = Some(file_type);
        self.reload(cx);
    }

    fn ensure_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_field.is_some() {
            return;
        }
        let (search_field, subscription) = SearchField::new(Some("Search books or authors"), |page: &mut Self, cx| page.search_changed(cx), window, cx);
        self.search_field = Some(search_field);
        self._search_subscription = Some(subscription);
    }

    pub(super) fn open_child(&mut self, child_id: String, cx: &mut Context<Self>) {
        let Some(child) = self.adapter.children().iter().find(|child| child.id == child_id).cloned() else {
            log::warn!("ignored missing {} child {child_id}", self.list_prefix);
            return;
        };
        let mut path = self.path.clone();
        path.push(BrowseCrumb { location: child.id, label: child.name });
        self.load_path(path, true, cx);
    }

    fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.load_path(path, true, cx);
    }

    fn load_path(&mut self, path: Vec<BrowseCrumb>, persist: bool, cx: &mut Context<Self>) {
        let location = path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| self.root_location.clone());
        let query = BrowseQuery { location: location.clone(), search: self.search.clone(), file_type: self.file_type };
        self.request.take();
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        match self.adapter.request(&query) {
            BrowseRequest::Ready(result) => self.finish_request(generation, location, path, persist, result, cx),
            BrowseRequest::Async(operation) => {
                self.loading = true;
                self.error = None;
                self.request = Some(cx.spawn(async move |page, cx| {
                    let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
                    let _ = page.update(cx, |page, cx| page.finish_request(generation, location, path, persist, result, cx));
                }));
                cx.notify();
            }
        }
    }

    fn finish_request(&mut self, generation: u64, location: String, path: Vec<BrowseCrumb>, persist: bool, result: Result<A::Response, String>, cx: &mut Context<Self>) {
        if generation != self.request_generation {
            return;
        }
        self.request.take();
        self.loading = false;
        match result {
            Ok(response) => {
                self.adapter.install(response, cx);
                self.location = location;
                self.path = path;
                self.error = None;
                if persist {
                    (self.persist_path)(&self.path);
                }
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
}

impl<A: BrowseAdapter> Render for BrowsePage<A> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_search_field(window, cx);
        let theme = components::browser_theme(cx);
        let search = self.search_field.as_ref().expect("search field initialized above").render();
        let controls = div()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(components::TOPBAR_ACTION_GAP))
            .child(search)
            .when_some(self.file_type, |controls, file_type| controls.child(file_type_filter_control("browse-file-type", file_type, cx.entity(), |page: &mut Self, file_type, cx| page.set_file_type(file_type, cx), theme)));
        let topbar = components::browser_stacked_topbar(theme).child(self.breadcrumb(cx)).child(controls);
        let body = if self.loading {
            components::browser_loading_message("Loading…", theme).into_any_element()
        } else {
            let page = cx.entity();
            let open_page = page.clone();
            let open_location: browse_grid::OpenLocation = Rc::new(move |child_id, cx: &mut App| {
                open_page.update(cx, |page, cx| page.open_child(child_id, cx));
            });
            browse_grid::render(format!("{}-next-contents-{}-{}", self.list_prefix, self.location, self.request_generation).into(), self.adapter.children(), self.adapter.books(), cx.entity(), open_location)
        };
        let page = cx.entity();
        let body = components::browser_error_banner(self.error.clone(), "Retry".into(), theme, body, move |cx| {
            page.update(cx, |page, cx| page.reload(cx));
        });
        components::full_size_column().child(topbar).child(components::remaining_space_column().p(rems(1.0)).child(body))
    }
}

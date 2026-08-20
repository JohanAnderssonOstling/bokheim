//! State owner shared by synchronous hierarchy browse sources.

use std::rc::Rc;

use epub_svelte_lib::{BackendError, BrowseContents, BrowseRow};
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, IntoElement, Render, SharedString, Task, Window, div, rems};
use gpui_component::scroll::ScrollableElement;
use ui_components as components;

use super::browse_grid;
use crate::library::services::LibraryContext;
use crate::library::widgets::BookGrid;

type LoadContents = Rc<dyn Fn(&str) -> Result<BrowseContents, BackendError>>;
type PersistPath = Rc<dyn Fn(&[BrowseCrumb])>;

/// Stable navigation data for one successfully opened hierarchy level.
///
/// Browse rows also carry changing inventory counts. Those belong to the
/// current contents, not to the breadcrumb or persisted navigation state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseCrumb {
    pub(crate) location: String,
    pub(crate) label: String,
}

impl From<&BrowseRow> for BrowseCrumb {
    fn from(row: &BrowseRow) -> Self {
        Self { location: row.id.clone(), label: row.name.clone() }
    }
}

pub(crate) struct BrowsePage {
    root_location: String,
    root_label: SharedString,
    location: String,
    path: Vec<BrowseCrumb>,
    list_prefix: &'static str,
    children: Rc<Vec<BrowseRow>>,
    books: BookGrid,
    load_contents: LoadContents,
    persist_path: PersistPath,
    _changes: Task<()>,
}

impl BrowsePage {
    pub(crate) fn new(
        ui: LibraryContext, root_location: String, root_label: impl Into<SharedString>, path: Vec<BrowseCrumb>, list_prefix: &'static str, changes: async_channel::Receiver<()>,
        load_contents: impl Fn(&str) -> Result<BrowseContents, BackendError> + 'static, persist_path: impl Fn(&[BrowseCrumb]) + 'static, cx: &mut Context<Self>,
    ) -> Self {
        let location = path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| root_location.clone());
        let contents = match load_contents(&location) {
            Ok(contents) => contents,
            Err(error) => {
                log::error!("failed to load {list_prefix} contents at {location}: {error}");
                BrowseContents::default()
            }
        };
        let mut books = BookGrid::new(ui, true);
        books.set_books(contents.books, cx);
        let changes = cx.spawn(async move |page, cx| {
            while changes.recv().await.is_ok() {
                let _ = page.update(cx, |page, cx| page.reload_current(cx));
            }
        });

        Self { root_location, root_label: root_label.into(), location, path, list_prefix, children: Rc::new(contents.children), books, load_contents: Rc::new(load_contents), persist_path: Rc::new(persist_path), _changes: changes }
    }

    fn reload_current(&mut self, cx: &mut Context<Self>) {
        self.load_path(self.path.clone(), false, cx);
    }

    fn open_child(&mut self, child_id: String, cx: &mut Context<Self>) {
        let Some(child) = self.children.iter().find(|child| child.id == child_id).cloned() else {
            log::warn!("ignored missing {} child {child_id}", self.list_prefix);
            return;
        };
        let mut path = self.path.clone();
        path.push(BrowseCrumb::from(&child));
        self.load_path(path, true, cx);
    }

    fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.load_path(path, true, cx);
    }

    /// Loads before committing so a failed navigation cannot replace or
    /// persist the last usable path.
    fn load_path(&mut self, path: Vec<BrowseCrumb>, persist: bool, cx: &mut Context<Self>) {
        let location = path.last().map(|crumb| crumb.location.clone()).unwrap_or_else(|| self.root_location.clone());
        match (self.load_contents)(&location) {
            Ok(contents) => {
                self.location = location;
                self.path = path;
                self.children = Rc::new(contents.children);
                self.books.set_books(contents.books, cx);
                if persist {
                    (self.persist_path)(&self.path);
                }
                cx.notify();
            }
            Err(error) => log::error!("failed to load {} contents at {location}: {error}", self.list_prefix),
        }
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
                continue;
            }

            let path = self.path[..=index].to_vec();
            let path_page = page.clone();
            breadcrumb = breadcrumb.child(item.on_click(move |_, _, cx| {
                path_page.update(cx, |page, cx| page.open_path(path.clone(), cx));
            }));
        }

        div().w_full().min_w_0().overflow_x_scrollbar().flex().items_center().justify_start().child(breadcrumb.flex_none()).child(div().w(gpui::px(1.0)).h(gpui::px(1.0)).flex_none()).into_any_element()
    }
}

impl Render for BrowsePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        let open_location: browse_grid::OpenLocation = Rc::new(move |child_id, cx: &mut App| {
            page.update(cx, |page, cx| page.open_child(child_id, cx));
        });
        let theme = components::browser_theme(cx);
        let topbar = components::browser_topbar(theme).child(self.breadcrumb(cx));
        let contents = browse_grid::render(format!("{}-contents-{}", self.list_prefix, self.location).into(), self.children.clone(), self.books.virtual_spanned_views(), cx.entity(), open_location);

        components::full_size_column().child(topbar).child(components::remaining_space_column().p(rems(1.0)).child(contents))
    }
}

use super::adapter::BrowseAdapter;
use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div};
use std::rc::Rc;
use ui_components as components;

use super::library::LibraryBrowseAdapter;
use super::library::{folder, subject};
use super::page::{BrowsePage, BrowseSplitClose, BrowseSplitRequest, BrowseSplitSide};
use crate::library::services::LibraryContext;

#[cfg(test)]
#[path = "split_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowsePlaneKind {
    Folder,
    Subject,
}

/// Owns independent browse pages and gives each page an equal share of the
/// available width. Query and navigation state remain inside each child page.
pub(crate) struct BrowseSplitPlane<A: BrowseAdapter = LibraryBrowseAdapter> {
    factory: Rc<dyn Fn(&mut Context<BrowsePage<A>>) -> BrowsePage<A>>,
    panes: Vec<BrowsePane<A>>,
    focus: FocusHandle,
    restore_focus: bool,
}

struct BrowsePane<A: BrowseAdapter> {
    page: Entity<BrowsePage<A>>,
    _subscriptions: Vec<Subscription>,
}

impl BrowseSplitPlane {
    pub(crate) fn open_folder(&mut self, directory_id: sync_common::DirId, title: String, cx: &mut Context<Self>) {
        let path = if directory_id == sync_common::ROOT_DIR_ID { Vec::new() } else { vec![super::adapter::BrowseCrumb { location: directory_id.to_string(), label: title }] };
        self.panes[0].page.update(cx, |page, cx| page.open_split_path(path, cx));
    }

    pub(crate) fn open_book_detail(&mut self, book: library_model::BookCardRow, path: Vec<super::adapter::BrowseCrumb>, cx: &mut Context<Self>) {
        self.panes[0].page.update(cx, |page, cx| page.open_book_detail(book, path, cx));
    }

    pub(crate) fn clear_cached_book_detail(&mut self, cx: &mut Context<Self>) {
        for pane in &self.panes {
            pane.page.update(cx, |page, cx| page.clear_cached_book_detail(cx));
        }
    }

    pub(crate) fn new(kind: BrowsePlaneKind, library: LibraryContext, cx: &mut Context<Self>) -> Self {
        Self::from_factory(
            Rc::new(move |cx| match kind {
                BrowsePlaneKind::Folder => folder(library.clone(), cx),
                BrowsePlaneKind::Subject => subject(library.clone(), cx),
            }),
            cx,
        )
    }
}

impl<A: BrowseAdapter> BrowseSplitPlane<A> {
    pub(crate) fn from_factory(factory: Rc<dyn Fn(&mut Context<BrowsePage<A>>) -> BrowsePage<A>>, cx: &mut Context<Self>) -> Self {
        let mut plane = Self { factory, panes: Vec::new(), focus: cx.focus_handle(), restore_focus: false };
        plane.add_page_at(0, Vec::new(), cx);
        plane.focus = plane.panes[0].page.read(cx).focus_handle(cx);
        plane
    }

    fn create_page(&self, cx: &mut Context<Self>) -> Entity<BrowsePage<A>> {
        cx.new(|cx| (self.factory)(cx))
    }

    pub(crate) fn update_pages(&mut self, mut update: impl FnMut(&mut BrowsePage<A>, &mut Context<BrowsePage<A>>), cx: &mut Context<Self>) {
        for pane in &self.panes {
            pane.page.update(cx, |page, cx| update(page, cx));
        }
    }

    pub(crate) fn set_library_view(&mut self, detailed: bool, cx: &mut Context<Self>) {
        self.update_pages(|page, cx| page.set_library_view(detailed, cx), cx);
    }

    fn add_page_at(&mut self, index: usize, path: Vec<super::adapter::BrowseCrumb>, cx: &mut Context<Self>) {
        let page = self.create_page(cx);
        let subscription = cx.subscribe(&page, |plane, source, request: &BrowseSplitRequest, cx| {
            plane.add_page(source, request.side, request.path.clone(), cx);
        });
        let close_subscription = cx.subscribe(&page, |plane, source, _: &BrowseSplitClose, cx| {
            plane.close_page(source, cx);
        });
        if !path.is_empty() {
            page.update(cx, |page, cx| page.open_split_path(path, cx));
        }
        let insert_at = index.min(self.panes.len());
        self.panes.insert(insert_at, BrowsePane { page, _subscriptions: vec![subscription, close_subscription] });
        self.refresh_close_buttons(cx);
        cx.notify();
    }

    fn add_page(&mut self, source: Entity<BrowsePage<A>>, side: BrowseSplitSide, path: Vec<super::adapter::BrowseCrumb>, cx: &mut Context<Self>) {
        let Some(source_index) = self.panes.iter().position(|pane| pane.page == source) else { return };
        let index = match side {
            BrowseSplitSide::Left => source_index,
            BrowseSplitSide::Right => source_index + 1,
        };
        self.add_page_at(index, path, cx);
    }

    fn close_page(&mut self, page: Entity<BrowsePage<A>>, cx: &mut Context<Self>) {
        if self.panes.len() <= 1 {
            return;
        }
        let Some(index) = self.panes.iter().position(|candidate| candidate.page == page) else { return };
        self.panes.remove(index);
        self.refresh_close_buttons(cx);
        self.focus = self.panes[0].page.read(cx).focus_handle(cx);
        self.restore_focus = true;
        cx.notify();
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn refresh_close_buttons(&mut self, cx: &mut Context<Self>) {
        let enabled = self.panes.len() > 1;
        self.update_pages(|page, cx| page.set_split_close_enabled(enabled, cx), cx);
    }
}

impl<A: BrowseAdapter> Render for BrowseSplitPlane<A> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.restore_focus {
            self.restore_focus = false;
            self.focus.focus(window, cx);
        }
        if !crate::split_panes_available(window) {
            let hidden_pane_has_focus = self.panes.iter().skip(1).any(|pane| pane.page.read(cx).focus_handle(cx).contains_focused(window, cx));
            self.focus = self.panes[0].page.read(cx).focus_handle(cx);
            if hidden_pane_has_focus {
                self.focus.focus(window, cx);
            }
            return self.panes.first().expect("browse split plane always has a page").page.clone().into_any_element();
        }
        let theme = components::browser_theme(cx);
        div()
            .size_full()
            .flex()
            .flex_row()
            .items_stretch()
            .children(
                self.panes.iter().enumerate().map(|(index, pane)| {
                    div().debug_selector(move || format!("browse-pane-{index}")).flex_1().min_w_0().min_h_0().overflow_hidden().when(index > 0, |pane| pane.border_l_1().border_color(theme.rule)).child(pane.page.clone())
                }),
            )
            .into_any_element()
    }
}

impl<A: BrowseAdapter> gpui::Focusable for BrowseSplitPlane<A> {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

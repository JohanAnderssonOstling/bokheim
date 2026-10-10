use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div};
use ui_components as components;

use super::listing::{BrowseCrumb, BrowseKind};
use super::page::{BrowsePage, BrowseSplitClose, BrowseSplitRequest, BrowseSplitSide};
use crate::library::services::LibraryContext;

/// Owns independent browse pages and gives each page an equal share of the
/// available width. Query and navigation state remain inside each child page.
pub(crate) struct BrowseSplitPlane {
    kind: BrowseKind,
    library: LibraryContext,
    panes: Vec<BrowsePane>,
    focus: FocusHandle,
    restore_focus: bool,
}

struct BrowsePane {
    page: Entity<BrowsePage>,
    _subscriptions: Vec<Subscription>,
}

impl BrowseSplitPlane {
    pub(crate) fn open_folder(&mut self, directory_id: sync_common::DirId, title: String, cx: &mut Context<Self>) {
        let path = if directory_id == sync_common::ROOT_DIR_ID { Vec::new() } else { vec![BrowseCrumb { location: library_model::BrowseLocation::Folder(directory_id), label: title }] };
        self.panes[0].page.update(cx, |page, cx| page.open_path(path, cx));
    }

    /// Opens a subject or an author; the backend supplies its display trail.
    pub(crate) fn open_location(&mut self, location: library_model::BrowseLocation, label: String, cx: &mut Context<Self>) {
        self.panes[0].page.update(cx, |page, cx| page.open_path(vec![BrowseCrumb { location, label }], cx));
    }

    pub(crate) fn open_book_detail(&mut self, book: library_model::BookCardRow, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.panes[0].page.update(cx, |page, cx| page.open_book_detail(book, path, cx));
    }

    pub(crate) fn new(kind: BrowseKind, library: LibraryContext, cx: &mut Context<Self>) -> Self {
        let mut plane = Self { kind, library, panes: Vec::new(), focus: cx.focus_handle(), restore_focus: false };
        plane.add_page_at(0, Vec::new(), cx);
        plane.focus = plane.panes[0].page.read(cx).focus_handle(cx);
        plane
    }

    fn add_page_at(&mut self, index: usize, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        let (kind, library) = (self.kind, self.library.clone());
        let page = cx.new(|cx| BrowsePage::new(kind, library, cx));
        let subscription = cx.subscribe(&page, |plane, source, request: &BrowseSplitRequest, cx| {
            plane.add_page(source, request.side, request.path.clone(), cx);
        });
        let close_subscription = cx.subscribe(&page, |plane, source, _: &BrowseSplitClose, cx| {
            plane.close_page(source, cx);
        });
        if !path.is_empty() {
            page.update(cx, |page, cx| page.open_path(path, cx));
        }
        let insert_at = index.min(self.panes.len());
        self.panes.insert(insert_at, BrowsePane { page, _subscriptions: vec![subscription, close_subscription] });
        self.refresh_close_buttons(cx);
        cx.notify();
    }

    fn add_page(&mut self, source: Entity<BrowsePage>, side: BrowseSplitSide, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        let Some(source_index) = self.panes.iter().position(|pane| pane.page == source) else { return };
        // Where a new pane on that side would go. The adjacent pane there, if
        // one is already open, is navigated in place — "open left/right"
        // means "show it over there", not "add yet another pane every time
        // it's asked for".
        let index = match side {
            BrowseSplitSide::Left => source_index,
            BrowseSplitSide::Right => source_index + 1,
        };
        let adjacent = match side {
            BrowseSplitSide::Left => index.checked_sub(1),
            BrowseSplitSide::Right => Some(index),
        };
        match adjacent.and_then(|adjacent| self.panes.get(adjacent)) {
            Some(pane) => pane.page.update(cx, |page, cx| page.open_path(path, cx)),
            None => self.add_page_at(index, path, cx),
        }
    }

    fn close_page(&mut self, page: Entity<BrowsePage>, cx: &mut Context<Self>) {
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
        for pane in &self.panes {
            pane.page.update(cx, |page, cx| page.set_split_close_enabled(enabled, cx));
        }
    }
}

impl Render for BrowseSplitPlane {
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

impl gpui::Focusable for BrowseSplitPlane {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

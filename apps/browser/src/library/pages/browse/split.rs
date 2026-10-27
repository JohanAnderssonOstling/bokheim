use gpui::{App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Subscription, Window};

use super::listing::{BrowseCrumb, BrowseKind};
use super::page::{BrowsePage, BrowseSplitClose, BrowseSplitRequest};
use crate::library::services::LibraryContext;
use crate::{BrowsePageSession, LibraryRoute, NavigationSession, PaneRequest, Route};

/// Hosts one browse page. The application owns splits, including each pane's
/// library selection, sidebar, reader and playback controller.
pub(crate) struct BrowsePageHost {
    kind: BrowseKind,
    library: LibraryContext,
    page: Entity<BrowsePage>,
    close_enabled: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PaneRequest> for BrowsePageHost {}

impl BrowsePageHost {
    pub(crate) fn new(kind: BrowseKind, library: LibraryContext, cx: &mut Context<Self>) -> Self {
        let page = cx.new(|cx| BrowsePage::new(kind, library.clone(), cx));
        let mut host = Self { kind, library, page, close_enabled: false, _subscriptions: Vec::new() };
        host.subscribe(cx);
        host
    }

    fn subscribe(&mut self, cx: &mut Context<Self>) {
        let mut saved = self.page.read(cx).saved_session(cx);
        self._subscriptions = vec![
            cx.observe(&self.page, move |_, page, cx| {
                let next = page.read(cx).saved_session(cx);
                if saved != next {
                    saved = next;
                    cx.notify();
                }
            }),
            cx.subscribe(&self.page, |host, _, request: &BrowseSplitRequest, cx| {
                let route = match host.kind {
                    BrowseKind::Home => LibraryRoute::Home,
                    BrowseKind::Books => LibraryRoute::Books,
                    BrowseKind::Folder => LibraryRoute::Folders,
                    BrowseKind::Subject => LibraryRoute::Subjects,
                    BrowseKind::Author => LibraryRoute::Authors,
                    BrowseKind::Catalog => LibraryRoute::Catalog,
                };
                let session = NavigationSession {
                    library_id: Some(*host.library.backend().id()),
                    route: Route::Library(route),
                    pages: vec![BrowsePageSession { path: request.path.iter().map(|crumb| (crumb.location, crumb.label.clone())).collect(), detail: None }],
                    ..Default::default()
                };
                cx.emit(PaneRequest::Open { session, side: request.side });
            }),
            cx.subscribe(&self.page, |_, _, _: &BrowseSplitClose, cx| cx.emit(PaneRequest::Close)),
        ];
    }

    pub(crate) fn set_split_close_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.close_enabled != enabled {
            self.close_enabled = enabled;
            self.page.update(cx, |page, cx| page.set_split_close_enabled(enabled, cx));
        }
    }

    pub(crate) fn open_root(&mut self, cx: &mut Context<Self>) {
        self.page = cx.new(|cx| BrowsePage::new(self.kind, self.library.clone(), cx));
        self.page.update(cx, |page, cx| page.set_split_close_enabled(self.close_enabled, cx));
        self.subscribe(cx);
        cx.notify();
    }

    pub(crate) fn open_folder(&mut self, directory_id: sync_common::DirId, title: String, cx: &mut Context<Self>) {
        let path = if directory_id == sync_common::ROOT_DIR_ID { Vec::new() } else { vec![BrowseCrumb { location: library_model::BrowseLocation::Folder(directory_id), label: title }] };
        self.open_path(path, cx);
    }

    pub(crate) fn open_location(&mut self, location: library_model::BrowseLocation, label: String, cx: &mut Context<Self>) {
        self.open_path(vec![BrowseCrumb { location, label }], cx);
    }

    pub(crate) fn open_path(&mut self, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.page.update(cx, |page, cx| page.open_path(path, cx));
    }

    pub(crate) fn open_book_detail(&mut self, book: library_model::BookCardRow, path: Vec<BrowseCrumb>, cx: &mut Context<Self>) {
        self.page.update(cx, |page, cx| page.open_book_detail(book, path, cx));
    }

    pub(crate) fn shown_book_detail(&self, cx: &App) -> Option<library_model::BookLocator> {
        self.page.read(cx).shown_book_detail(cx)
    }

    pub(crate) fn saved_pages(&self, cx: &App) -> Vec<BrowsePageSession> {
        vec![self.page.read(cx).saved_session(cx)]
    }

    pub(crate) fn restore(&mut self, pages: Vec<BrowsePageSession>, cx: &mut Context<Self>) {
        if let Some(saved) = pages.into_iter().next() {
            self.page.update(cx, |page, cx| page.restore_session(saved, cx));
        }
    }

    pub(crate) fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.page.read(cx).focus_handle(cx)
    }

    pub(crate) fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        self.page.update(cx, |page, cx| page.dismiss(cx))
    }
}

impl Render for BrowsePageHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.page.clone()
    }
}

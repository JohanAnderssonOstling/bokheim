//! Root and lifetime boundary for the selected library.
//!
//! Pages are created on first visit and retained for the lifetime of the
//! selected library, so returning to a route restores the scroll position,
//! filter and selection the user left there. Dropping them all at once still
//! happens — but at the library boundary, where the state genuinely stops being
//! meaningful, not on every route change.
//!
//! Retention means a background page can fall behind the database. Library
//! events refresh the visible page and mark the rest stale; a stale page is
//! refreshed when it is next shown.

use std::collections::{HashMap, HashSet};

use epub_svelte_lib::LibraryBackend;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use gpui::{AnyElement, AppContext, Context, Entity, EventEmitter, IntoElement, Render, Subscription, Window};

mod detail;
mod host;
mod pages;
mod services;
mod setup;
mod title_filter;
mod widgets;

use self::detail::BookDetailPage;
use self::pages::authors::AuthorsPage;
use self::pages::folders::{FoldersPage, FoldersPageEvent};
use self::pages::home::HomePage;
use self::pages::subjects::{SubjectsPage, SubjectsPageEvent};
use self::pages::trash::TrashPage;
use self::services::LibraryContext;
use self::widgets::CoverInvalidationRegistry;
pub(crate) use self::widgets::{LoadedCover, loaded_cover_from_jpeg};
use crate::model::{FolderCrumb, SubjectCrumb};
use crate::route::LibraryRoute;

pub(crate) use host::{LibraryHost, LibraryHostEvent, LibraryState, LibraryStateChanged};
pub(crate) use setup::{LibrarySetup, LibrarySetupEvent};

#[derive(Clone)]
pub(crate) enum LibraryRootEvent {
    FolderPathChanged,
    SubjectPathChanged,
}

/// A retained page, holding the subscription that carries its events up to the
/// root. The subscription lives beside the entity so that a page kept in the
/// background keeps reporting path changes.
enum LibraryPage {
    Home(Entity<HomePage>),
    Authors(Entity<AuthorsPage>),
    Folders { page: Entity<FoldersPage>, _subscription: Subscription },
    Subjects { page: Entity<SubjectsPage>, _subscription: Subscription },
    Trash(Entity<TrashPage>),
}

impl LibraryPage {
    fn element(&self) -> AnyElement {
        match self {
            Self::Home(page) => page.clone().into_any_element(),
            Self::Authors(page) => page.clone().into_any_element(),
            Self::Folders { page, .. } => page.clone().into_any_element(),
            Self::Subjects { page, .. } => page.clone().into_any_element(),
            Self::Trash(page) => page.clone().into_any_element(),
        }
    }
}

pub(crate) struct LibraryRoot {
    context: LibraryContext,
    folder_path: Vec<FolderCrumb>,
    subject_path: Vec<SubjectCrumb>,
    route: LibraryRoute,
    pages: HashMap<LibraryRoute, LibraryPage>,
    /// Routes whose retained page has not seen a library change since it was
    /// last refreshed. Drained on display, never for the visible page.
    stale: HashSet<LibraryRoute>,
    detail: Option<Entity<BookDetailPage>>,
}

impl EventEmitter<LibraryRootEvent> for LibraryRoot {}

impl LibraryRoot {
    pub(crate) fn new(mut context: LibraryContext, route: LibraryRoute, folder_path: Vec<FolderCrumb>, subject_path: Vec<SubjectCrumb>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let target = cx.entity().downgrade();
        context.set_open_detail(std::rc::Rc::new(move |content_hash, cx| {
            let _ = target.update(cx, |root, cx| root.open_detail(content_hash, cx));
        }));
        let mut root = Self { context, folder_path, subject_path, route, pages: HashMap::new(), stale: HashSet::new(), detail: None };
        root.show(route, window, cx);
        root
    }

    fn open_detail(&mut self, content_hash: epub_svelte_lib::ContentHash, cx: &mut Context<Self>) {
        let context = self.context.clone();
        let target = cx.entity().downgrade();
        self.detail = Some(cx.new(|cx| {
            BookDetailPage::new(
                context,
                content_hash,
                std::rc::Rc::new(move |cx| {
                    let _ = target.update(cx, |root, cx| {
                        root.detail = None;
                        cx.notify();
                    });
                }),
                cx,
            )
        }));
        cx.notify();
    }

    /// Drops the transient book-detail surface and everything it owns.
    ///
    /// Retained browse pages survive ordinary navigation, but details do not:
    /// their HTML renderer and background tasks belong only to the currently
    /// visible book. Sidebar navigation must therefore destroy this entity
    /// even when the selected sidebar route is already `self.route`.
    pub(crate) fn dismiss_detail(&mut self, cx: &mut Context<Self>) -> bool {
        if self.detail.take().is_none() {
            return false;
        }
        cx.notify();
        true
    }

    pub(crate) fn folder_path(&self) -> Vec<FolderCrumb> {
        self.folder_path.clone()
    }

    pub(crate) fn subject_path(&self) -> Vec<SubjectCrumb> {
        self.subject_path.clone()
    }

    pub(crate) fn route(&self) -> LibraryRoute {
        self.route
    }

    pub(crate) fn backend(&self) -> LibraryBackend {
        self.context.library.clone()
    }

    /// Focuses the search box of the page in front, which is the only search
    /// box there is now: it searches everything beneath the folder or subject
    /// being browsed.
    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.pages.get(&self.route) {
            Some(LibraryPage::Folders { page, .. }) => page.update(cx, |page, cx| page.focus_search(window, cx)),
            Some(LibraryPage::Subjects { page, .. }) => page.update(cx, |page, cx| page.focus_search(window, cx)),
            _ => {}
        }
    }

    pub(crate) fn set_route(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == route {
            return;
        }
        self.show(route, window, cx);
        cx.notify();
    }

    /// Makes `route` the visible page, creating it on first visit and bringing
    /// it up to date if library changes landed while it was in the background.
    fn show(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        self.route = route;
        if !self.pages.contains_key(&route) {
            let page = self.create_page(route, window, cx);
            self.pages.insert(route, page);
            self.stale.insert(route);
        }
        if self.stale.remove(&route) {
            self.refresh(route, cx);
        }
    }

    fn create_page(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) -> LibraryPage {
        let context = self.context.clone();
        match route {
            LibraryRoute::Home => LibraryPage::Home(cx.new(|cx| HomePage::new(context, cx))),
            LibraryRoute::Authors => LibraryPage::Authors(cx.new(|cx| AuthorsPage::new(context, window, cx))),
            LibraryRoute::Folders => {
                let folder_path = self.folder_path.clone();
                let page = cx.new(|cx| FoldersPage::new(context, folder_path, window, cx));
                let subscription = cx.subscribe(&page, |root: &mut Self, _, event: &FoldersPageEvent, cx| match event {
                    FoldersPageEvent::PathChanged(path) => {
                        root.folder_path = path.clone();
                        cx.emit(LibraryRootEvent::FolderPathChanged);
                    }
                    FoldersPageEvent::ContentsChanged => {}
                });
                LibraryPage::Folders { page, _subscription: subscription }
            }
            LibraryRoute::Subjects => {
                let subject_path = self.subject_path.clone();
                let page = cx.new(|cx| SubjectsPage::new(context, subject_path, window, cx));
                let subscription = cx.subscribe(&page, |root: &mut Self, _, event: &SubjectsPageEvent, cx| match event {
                    SubjectsPageEvent::PathChanged(path) => {
                        root.subject_path = path.clone();
                        cx.emit(LibraryRootEvent::SubjectPathChanged);
                    }
                });
                LibraryPage::Subjects { page, _subscription: subscription }
            }
            LibraryRoute::Trash => LibraryPage::Trash(cx.new(|_| TrashPage::new(context))),
        }
    }

    /// Switches every retained page's grids between covers and detailed rows.
    /// Applied to background pages too rather than being deferred like a data
    /// refresh: this costs nothing until a page is drawn, and a page that is
    /// shown later must not still be in the previous presentation.
    pub(crate) fn set_library_view(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<Self>) {
        for page in self.pages.values() {
            match page {
                LibraryPage::Home(_) => {}
                LibraryPage::Authors(page) => page.update(cx, |page, cx| page.set_library_view(library_view, cx)),
                LibraryPage::Folders { page, .. } => page.update(cx, |page, cx| page.set_library_view(library_view, cx)),
                LibraryPage::Subjects { page, .. } => page.update(cx, |page, cx| page.set_library_view(library_view, cx)),
                LibraryPage::Trash(_) => {}
            }
        }
        cx.notify();
    }

    /// Refreshes the visible page now and defers the retained ones until they
    /// are shown, so a background page never queries for a view nobody is
    /// looking at.
    pub(crate) fn refresh_active(&mut self, cx: &mut Context<Self>) {
        self.refresh(self.route, cx);
        for route in self.pages.keys().copied().filter(|route| *route != self.route).collect::<Vec<_>>() {
            self.stale.insert(route);
        }
    }

    fn refresh(&mut self, route: LibraryRoute, cx: &mut Context<Self>) {
        match self.pages.get(&route) {
            Some(LibraryPage::Home(page)) => page.update(cx, |page, cx| page.refresh(cx)),
            Some(LibraryPage::Authors(page)) => page.update(cx, |page, cx| page.refresh(cx)),
            Some(LibraryPage::Folders { page, .. }) => page.update(cx, |page, cx| page.refresh(cx)),
            Some(LibraryPage::Subjects { page, .. }) => page.update(cx, |page, cx| page.refresh(cx)),
            Some(LibraryPage::Trash(page)) => page.update(cx, |page, cx| page.refresh(cx)),
            None => {}
        }
    }

    pub(crate) fn handle_library_event(&mut self, event: &LibraryEvent, cx: &mut Context<Self>) {
        match event {
            LibraryEvent::ThumbnailGenerated { content_hash } => self.patch_thumbnails(std::slice::from_ref(content_hash), cx),
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, .. } => self.patch_thumbnails(content_hashes, cx),
            LibraryEvent::FolderDownloadStatusChanged { .. } => self.refresh_active(cx),
            LibraryEvent::DownloadStatusChanged { .. } | LibraryEvent::TransferStatusChanged => {}
            LibraryEvent::CatalogueChanged => self.refresh_active(cx),
            LibraryEvent::SubjectEnrichmentBatchProcessed { .. } => self.refresh_active(cx),
            LibraryEvent::BookAdded { .. } | LibraryEvent::BookRemoved { .. } | LibraryEvent::DirectoryAdded { .. } | LibraryEvent::DirectoryRemoved { .. } | LibraryEvent::ScanComplete => self.refresh_active(cx),
        }
    }

    fn patch_thumbnails(&self, content_hashes: &[epub_svelte_lib::ContentHash], cx: &mut Context<Self>) {
        CoverInvalidationRegistry::invalidate(self.context.library.id().clone(), content_hashes, cx);
    }
}

impl Render for LibraryRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.detail.clone().map(|detail| detail.into_any_element()).unwrap_or_else(|| self.pages.get(&self.route).map(LibraryPage::element).unwrap_or_else(|| gpui::div().into_any_element()))
    }
}

//! Root and lifetime boundary for the selected library.
//!
//! Only the active route owns a page entity. Changing routes drops that page,
//! its retained cards, and any in-flight page work as one operation.

use epub_svelte_lib::LibraryBackend;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use gpui::{AnyElement, AppContext, Context, Entity, EventEmitter, IntoElement, Render, Subscription, Window};

mod host;
mod pages;
mod services;
mod setup;
mod widgets;

use self::pages::authors::AuthorsPage;
use self::pages::folders::{FoldersPage, FoldersPageEvent};
use self::pages::home::HomePage;
use self::pages::search::SearchPage;
use self::services::LibraryContext;
use crate::model::FolderCrumb;
use crate::route::LibraryRoute;

pub(crate) use host::{LibraryHost, LibraryHostEvent, LibraryState, LibraryStateChanged};
pub(crate) use setup::{LibrarySetup, LibrarySetupEvent};

#[derive(Clone)]
pub(crate) enum LibraryRootEvent {
    FolderPathChanged,
}

enum LibraryPage {
    Home(Entity<HomePage>),
    Authors(Entity<AuthorsPage>),
    Folders(Entity<FoldersPage>),
    Search(Entity<SearchPage>),
}

impl LibraryPage {
    fn route(&self) -> LibraryRoute {
        match self {
            Self::Home(_) => LibraryRoute::Home,
            Self::Authors(_) => LibraryRoute::Authors,
            Self::Folders(_) => LibraryRoute::Folders,
            Self::Search(_) => LibraryRoute::Search,
        }
    }

    fn element(&self) -> AnyElement {
        match self {
            Self::Home(page) => page.clone().into_any_element(),
            Self::Authors(page) => page.clone().into_any_element(),
            Self::Folders(page) => page.clone().into_any_element(),
            Self::Search(page) => page.clone().into_any_element(),
        }
    }
}

pub(crate) struct LibraryRoot {
    context: LibraryContext,
    folder_path: Vec<FolderCrumb>,
    page: LibraryPage,
    page_subscription: Option<Subscription>,
}

impl EventEmitter<LibraryRootEvent> for LibraryRoot {}

impl LibraryRoot {
    pub(crate) fn new(context: LibraryContext, route: LibraryRoute, folder_path: Vec<FolderCrumb>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (page, page_subscription) = Self::create_page(context.clone(), route, &folder_path, window, cx);
        let mut root = Self { context, folder_path, page, page_subscription };
        root.refresh_active(cx);
        root
    }

    pub(crate) fn folder_path(&self) -> Vec<FolderCrumb> {
        self.folder_path.clone()
    }

    pub(crate) fn route(&self) -> LibraryRoute {
        self.page.route()
    }

    pub(crate) fn backend(&self) -> LibraryBackend {
        self.context.library.clone()
    }

    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let LibraryPage::Search(page) = &self.page {
            page.update(cx, |page, cx| page.focus_input(window, cx));
        }
    }

    pub(crate) fn set_route(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.route() == route {
            return;
        }
        let (page, subscription) = Self::create_page(self.context.clone(), route, &self.folder_path, window, cx);
        self.page = page;
        self.page_subscription = subscription;
        self.refresh_active(cx);
        cx.notify();
    }

    fn create_page(context: LibraryContext, route: LibraryRoute, folder_path: &[FolderCrumb], window: &mut Window, cx: &mut Context<Self>) -> (LibraryPage, Option<Subscription>) {
        let mut subscription = None;
        let page = match route {
            LibraryRoute::Home => LibraryPage::Home(cx.new(|cx| HomePage::new(context, cx))),
            LibraryRoute::Authors => LibraryPage::Authors(cx.new(|cx| AuthorsPage::new(context, window, cx))),
            LibraryRoute::Folders => {
                let folder_path = folder_path.to_vec();
                let page = cx.new(|cx| FoldersPage::new(context, folder_path, cx));
                subscription = Some(cx.subscribe(&page, |root: &mut Self, _, event: &FoldersPageEvent, cx| match event {
                    FoldersPageEvent::PathChanged(path) => {
                        root.folder_path = path.clone();
                        cx.emit(LibraryRootEvent::FolderPathChanged);
                    }
                }));
                LibraryPage::Folders(page)
            }
            LibraryRoute::Search => LibraryPage::Search(cx.new(|cx| SearchPage::new(context, window, cx))),
        };
        (page, subscription)
    }

    pub(crate) fn refresh_active(&mut self, cx: &mut Context<Self>) {
        match &self.page {
            LibraryPage::Home(page) => page.update(cx, |page, cx| page.refresh(cx)),
            LibraryPage::Authors(page) => page.update(cx, |page, cx| page.refresh(cx)),
            LibraryPage::Folders(page) => page.update(cx, |page, cx| page.refresh(cx)),
            LibraryPage::Search(page) => page.update(cx, |page, cx| page.refresh(cx)),
        }
    }

    pub(crate) fn handle_library_event(&mut self, event: &LibraryEvent, cx: &mut Context<Self>) {
        match event {
            LibraryEvent::DownloadStatusChanged { .. } | LibraryEvent::FolderDownloadStatusChanged { .. } | LibraryEvent::ThumbnailGenerated { .. } | LibraryEvent::TransferStatusChanged => {}
            LibraryEvent::BookAdded { .. } | LibraryEvent::BookRemoved { .. } | LibraryEvent::DirectoryAdded { .. } | LibraryEvent::DirectoryRemoved { .. } | LibraryEvent::ScanComplete => self.refresh_active(cx),
        }
    }
}

impl Render for LibraryRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.page.element()
    }
}

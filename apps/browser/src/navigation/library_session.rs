//! Persistent pages owned by one library session.

use std::collections::HashMap;
use std::rc::Rc;

use app::AppClient;
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div, px};
use sync_common::{LibraryId, ROOT_DIR_ID};

use super::LibraryRoute;
use crate::BrowsePageSession;
use crate::library::BrowseCrumb;
use crate::library::{BrowseKind, BrowseSplitPlane, LibraryContext, LibraryUpdateBridge, OpenBook, TrashPage, UndoEntry, UndoStack};

struct CachedPage {
    view: AnyView,
    focus: FocusHandle,
}

pub(crate) struct LibrarySession {
    undo: UndoStack,
    library: LibraryContext,
    route: LibraryRoute,
    pages: HashMap<LibraryRoute, CachedPage>,
    browse_planes: HashMap<LibraryRoute, Entity<BrowseSplitPlane>>,
    _plane_subscriptions: Vec<Subscription>,
    _library_updates_subscription: Subscription,
}

fn create_page<V: Focusable + Render>(build: impl FnOnce(&mut Context<V>) -> V, cx: &mut Context<LibrarySession>) -> CachedPage {
    let page = cx.new(build);
    let focus = page.read(cx).focus_handle(cx);
    CachedPage { view: page.into(), focus }
}

impl LibrarySession {
    pub(super) fn new(app_backend: Rc<AppClient>, open_book: OpenBook, library_id: LibraryId, cx: &mut Context<Self>) -> Self {
        let library_backend = Rc::new(app_backend.library(library_id));
        let updates = LibraryUpdateBridge::create(app_backend.clone(), library_id, cx);
        let library = LibraryContext::new(app_backend, library_backend, open_book, updates, cx.entity().downgrade());
        let library_updates = library.updates();
        let library_updates_subscription = cx.observe(&library_updates, |_, _, cx| cx.notify());
        let mut session =
            Self { undo: Default::default(), library, route: LibraryRoute::Home, pages: HashMap::new(), browse_planes: HashMap::new(), _plane_subscriptions: Vec::new(), _library_updates_subscription: library_updates_subscription };
        session.browse_plane(LibraryRoute::Home, cx);
        session
    }

    pub(crate) fn push_undo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
    }

    pub(crate) fn take_undo(&mut self) -> Option<UndoEntry> {
        self.undo.take()
    }

    pub(super) fn set_importing(&mut self, importing: bool, cx: &mut Context<Self>) {
        self.library.updates().update(cx, |updates, cx| updates.set_importing(importing, cx));
    }

    fn create_route_page(&self, route: LibraryRoute, cx: &mut Context<Self>) -> CachedPage {
        let library = self.library.clone();
        match route {
            LibraryRoute::Home | LibraryRoute::Books | LibraryRoute::Folders | LibraryRoute::Subjects | LibraryRoute::Authors | LibraryRoute::Catalog => unreachable!("browse routes use split planes"),
            LibraryRoute::Trash => create_page(|cx| TrashPage::new(library, cx), cx),
        }
    }

    fn page(&self) -> &CachedPage {
        self.pages.get(&self.route).expect("the active library page must be cached")
    }

    fn is_browse_route(route: LibraryRoute) -> bool {
        matches!(route, LibraryRoute::Home | LibraryRoute::Books | LibraryRoute::Folders | LibraryRoute::Subjects | LibraryRoute::Authors | LibraryRoute::Catalog)
    }

    fn browse_plane(&mut self, route: LibraryRoute, cx: &mut Context<Self>) -> Entity<BrowseSplitPlane> {
        if let Some(plane) = self.browse_planes.get(&route) {
            return plane.clone();
        }
        let kind = match route {
            LibraryRoute::Home => BrowseKind::Home,
            LibraryRoute::Books => BrowseKind::Books,
            LibraryRoute::Folders => BrowseKind::Folder,
            LibraryRoute::Subjects => BrowseKind::Subject,
            LibraryRoute::Authors => BrowseKind::Author,
            LibraryRoute::Catalog => BrowseKind::Catalog,
            _ => unreachable!("only browse routes have browse planes"),
        };
        let library = self.library.clone();
        let plane = cx.new(|cx| BrowseSplitPlane::new(kind, library, cx));
        self._plane_subscriptions.push(cx.observe(&plane, |_, _, cx| cx.notify()));
        self.browse_planes.insert(route, plane.clone());
        plane
    }

    pub(super) fn saved_pages(&self, cx: &App) -> Vec<BrowsePageSession> {
        self.browse_planes.get(&self.route).map(|plane| plane.read(cx).saved_pages(cx)).unwrap_or_default()
    }

    pub(super) fn restore(&mut self, route: LibraryRoute, pages: Vec<BrowsePageSession>, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(route, window, cx);
        if Self::is_browse_route(route) {
            self.browse_plane(route, cx).update(cx, |plane, cx| plane.restore(pages, cx));
        }
    }

    pub(super) fn route(&self) -> LibraryRoute {
        self.route
    }

    pub(super) fn dismiss_page(&mut self, cx: &mut Context<Self>) -> bool {
        self.browse_planes.get(&self.route).cloned().is_some_and(|plane| plane.update(cx, |plane, cx| plane.dismiss(cx)))
    }

    pub(super) fn open_folder(&mut self, directory_id: sync_common::DirId, title: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Folders, window, cx);
        self.browse_plane(LibraryRoute::Folders, cx).update(cx, |plane, cx| plane.open_folder(directory_id, title, cx));
    }

    pub(super) fn open_subject(&mut self, location: library_model::BrowseLocation, label: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Subjects, window, cx);
        self.browse_plane(LibraryRoute::Subjects, cx).update(cx, |plane, cx| plane.open_location(location, label, cx));
    }

    fn open_author(&mut self, author: book_model::AuthorId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Authors, window, cx);
        self.browse_plane(LibraryRoute::Authors, cx).update(cx, |plane, cx| plane.open_location(library_model::BrowseLocation::Author(Some(author)), name, cx));
    }

    pub(super) fn shown_book_detail(&self, cx: &App) -> Option<library_model::BookLocator> {
        if !Self::is_browse_route(self.route) {
            return None;
        }
        self.browse_planes.get(&self.route)?.read(cx).shown_book_detail(cx)
    }

    pub(super) fn open_book_detail(&mut self, content_hash: sync_common::ContentHash, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Folders, window, cx);
        let backend = self.library.backend().clone();
        let plane = self.browse_plane(LibraryRoute::Folders, cx);
        cx.spawn(async move |session, cx| {
            let result = backend.book_detail(content_hash).await;
            cx.defer_update(session, move |_session, cx| {
                if let Ok(detail) = result {
                    let path = detail.path.into_iter().map(|segment| BrowseCrumb { location: segment.id, label: segment.name }).collect();
                    plane.update(cx, |plane, cx| plane.open_book_detail(detail.book, path, cx));
                }
            });
        })
        .detach();
    }

    pub(super) fn focus_handle(&self, cx: &App) -> FocusHandle {
        if Self::is_browse_route(self.route) { self.browse_planes.get(&self.route).expect("browse plane must exist before it is focused").read(cx).focus_handle() } else { self.page().focus.clone() }
    }

    pub(super) fn navigate(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        if Self::is_browse_route(route) {
            self.browse_plane(route, cx);
        } else if !self.pages.contains_key(&route) {
            let page = self.create_route_page(route, cx);
            self.pages.insert(route, page);
        }
        self.route = route;
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn choose_book(&mut self, cx: &mut Context<Self>) {
        self.library.choose_and_import_book(ROOT_DIR_ID, cx);
    }

    #[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
    pub(crate) async fn test_context(cx: &mut gpui::TestAppContext) -> (tempfile::TempDir, Entity<Self>, LibraryContext) {
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, _) = AppClient::start_native(launch).unwrap();
        let library = backend.create_library_with_asset_storage("UI regression fixture".into(), true).await.unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::init_page_scrolling(cx);
        });
        let session = cx.new(|cx| Self::new(Rc::new(backend), Rc::new(|_, _, _, _, _| {}), *library.library_id(), cx));
        let context = cx.read_entity(&session, |session, _| session.library.clone());
        (directory, session, context)
    }
}

impl Render for LibrarySession {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let updates = self.library.updates();
        let updates = updates.read(cx);
        let uses_contents = matches!(self.route, LibraryRoute::Home | LibraryRoute::Books | LibraryRoute::Authors | LibraryRoute::Folders | LibraryRoute::Subjects | LibraryRoute::Catalog);
        // Keep import destinations and the catalog mounted even when the local
        // library is empty: catalog books do not belong to it yet.
        let content = if !(uses_contents && !matches!(self.route, LibraryRoute::Folders | LibraryRoute::Books | LibraryRoute::Catalog) && !updates.loading() && updates.book_count() == Some(0) && !updates.scanning()) {
            let view = if Self::is_browse_route(self.route) { self.browse_planes.get(&self.route).expect("browse plane must exist before render").clone().into_any_element() } else { self.page().view.clone().into_any_element() };
            ui_components::library_section_content().debug_selector(|| "library-session-page".into()).child(view).into_any_element()
        } else {
            let theme = ui_components::browser_theme(cx);
            let session = cx.entity();
            let actions = div().flex().items_center().gap(px(ui_components::SPACE_SM)).child(ui_components::primary_action_button("empty-library-add-book", "Add book", gpui_component::IconName::Plus, false).on_click(move |_, _, cx| {
                session.update(cx, |session, cx| session.choose_book(cx));
            }));
            // The browse page's own heading, which an empty view still needs to
            // reach the others, which may not be empty.
            let heading = (ui_components::uses_mobile_navigation(window) && Self::is_browse_route(self.route)).then(|| ui_components::topbar(theme).child(super::library_root_heading(self.route, theme)));
            ui_components::library_section_content()
                .debug_selector(|| "library-session-empty".into())
                .children(heading)
                .child(ui_components::remaining_space_column().child(ui_components::browser_actionable_empty("No books in this library", "Add a book to get started.", actions, theme).size_full()))
                .into_any_element()
        };
        ui_components::library_section_content()
            .on_action(cx.listener(|session, _: &crate::OpenTrash, window, cx| session.navigate(LibraryRoute::Trash, window, cx)))
            .on_action(cx.listener(|session, open: &crate::OpenAuthor, window, cx| session.open_author(open.author, open.name.clone(), window, cx)))
            .child(content)
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests {
    use super::*;
    use crate::library::{UndoEntry, UndoStep};
    use gpui::{TestAppContext, VisualTestContext};

    #[gpui::test]
    async fn empty_library_keeps_catalog_and_import_pages_visible(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, _) = AppClient::start_native(launch).unwrap();
        let library = backend.create_library_with_asset_storage("Empty".into(), false).await.unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::init_page_scrolling(cx);
        });
        let window = cx.add_window(|_, cx| LibrarySession::new(Rc::new(backend), Rc::new(|_, _, _, _, _| {}), *library.library_id(), cx));
        let updates = window.read_with(cx, |session, _| session.library.updates().clone()).unwrap();
        cx.condition(&updates, |updates, _| !updates.loading() && !updates.scanning() && updates.book_count() == Some(0)).await;
        let mut visual = VisualTestContext::from_window(window.into(), cx);

        for (route, empty_library) in [
            (LibraryRoute::Home, true),
            (LibraryRoute::Catalog, false),
            (LibraryRoute::Authors, true),
            (LibraryRoute::Books, false),
            (LibraryRoute::Folders, false),
            (LibraryRoute::Subjects, true),
        ] {
            window.update(&mut visual, |session, window, cx| session.navigate(route, window, cx)).unwrap();
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
            assert_eq!(visual.debug_bounds("library-session-empty").is_some(), empty_library, "wrong empty state for {route:?}");
            assert_eq!(visual.debug_bounds("library-session-page").is_some(), !empty_library, "page was hidden for {route:?}");
        }
    }

    #[gpui::test]
    async fn async_completion_records_undo_in_the_session_after_navigation(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, _) = AppClient::start_native(launch).unwrap();
        let library = backend.create_library_with_asset_storage("Undo".into(), true).await.unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::init_page_scrolling(cx);
        });
        let window = cx.add_window(|_, cx| LibrarySession::new(Rc::new(backend), Rc::new(|_, _, _, _, _| {}), *library.library_id(), cx));
        let context = window.update(cx, |session, _, _| session.library.clone()).unwrap();
        let owner = context.session();
        let (finish, ready) = async_channel::bounded(1);
        // The operation owns its work. Its target is the session, independently
        // of the page that launched it or the currently displayed route.
        let task = cx.spawn(async move |mut cx| {
            ready.recv().await.unwrap();
            owner.update(&mut cx, |session, _| session.push_undo(UndoEntry::new("Completed move").with(UndoStep::TrashDirectory { directory: ROOT_DIR_ID }))).unwrap();
        });
        window.update(cx, |session, window, cx| session.navigate(LibraryRoute::Authors, window, cx)).unwrap();
        finish.send(()).await.unwrap();
        task.await;
        assert_eq!(window.update(cx, |session, _, _| session.take_undo().unwrap().label).unwrap(), "Completed move");
        assert!(window.update(cx, |session, _, _| session.take_undo()).unwrap().is_none());
        let owner = context.session();
        let (finish, ready) = async_channel::bounded(1);
        let task = cx.spawn(async move |mut cx| {
            ready.recv().await.unwrap();
            owner.update(&mut cx, |session, _| session.push_undo(UndoEntry::new("Finished after close").with(UndoStep::TrashDirectory { directory: ROOT_DIR_ID })))
        });
        window.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        finish.send(()).await.unwrap();
        assert!(task.await.is_err(), "background work must not keep a closed session alive");
        drop(context);
    }
}

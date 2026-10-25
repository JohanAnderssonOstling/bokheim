//! Persistent pages owned by one library session.

use std::collections::HashMap;
use std::rc::Rc;

use app::AppClient;
use gpui::prelude::*;
use gpui::{App, AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div, px};
use gpui_component::IconName;
use sync_common::{LibraryId, ROOT_DIR_ID};

use super::LibraryRoute;
use crate::BrowsePageSession;
use crate::library::BrowseCrumb;
use crate::library::{BrowseKind, BrowseSplitPlane, LibraryContext, LibraryUpdateBridge, OpenBook, TrashPage, UndoEntry, UndoStack};

#[derive(Clone, Copy)]
enum SessionPage {
    Browse(BrowseKind),
    Trash,
}

impl From<LibraryRoute> for SessionPage {
    fn from(route: LibraryRoute) -> Self {
        match route {
            LibraryRoute::Home => Self::Browse(BrowseKind::Home),
            LibraryRoute::Books => Self::Browse(BrowseKind::Books),
            LibraryRoute::Folders => Self::Browse(BrowseKind::Folder),
            LibraryRoute::Subjects => Self::Browse(BrowseKind::Subject),
            LibraryRoute::Authors => Self::Browse(BrowseKind::Author),
            LibraryRoute::Catalog => Self::Browse(BrowseKind::Catalog),
            LibraryRoute::Trash => Self::Trash,
        }
    }
}

pub(crate) struct LibrarySession {
    undo: UndoStack,
    library: LibraryContext,
    route: LibraryRoute,
    focus: FocusHandle,
    trash: Option<Entity<TrashPage>>,
    browse_planes: HashMap<BrowseKind, Entity<BrowseSplitPlane>>,
    _plane_subscriptions: Vec<Subscription>,
    _library_updates_subscription: Subscription,
}

impl LibrarySession {
    pub(super) fn new(app_backend: Rc<AppClient>, open_book: OpenBook, library_id: LibraryId, cx: &mut Context<Self>) -> Self {
        let library_backend = Rc::new(app_backend.library(library_id));
        let updates = LibraryUpdateBridge::create(app_backend.clone(), library_id, cx);
        let library = LibraryContext::new(app_backend, library_backend, open_book, updates, cx.entity().downgrade());
        let library_updates = library.updates();
        let library_updates_subscription = cx.observe(&library_updates, |_, _, cx| cx.notify());
        let mut session = Self {
            undo: Default::default(),
            library,
            route: LibraryRoute::Home,
            focus: cx.focus_handle(),
            trash: None,
            browse_planes: HashMap::new(),
            _plane_subscriptions: Vec::new(),
            _library_updates_subscription: library_updates_subscription,
        };
        session.browse_plane(BrowseKind::Home, cx);
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

    fn trash_page(&mut self, cx: &mut Context<Self>) -> Entity<TrashPage> {
        let library = self.library.clone();
        self.trash.get_or_insert_with(|| cx.new(|cx| TrashPage::new(library, cx))).clone()
    }

    fn browse_plane(&mut self, kind: BrowseKind, cx: &mut Context<Self>) -> Entity<BrowseSplitPlane> {
        if let Some(plane) = self.browse_planes.get(&kind) {
            return plane.clone();
        }
        let library = self.library.clone();
        let plane = cx.new(|cx| BrowseSplitPlane::new(kind, library, cx));
        self._plane_subscriptions.push(cx.observe(&plane, |_, _, cx| cx.notify()));
        self.browse_planes.insert(kind, plane.clone());
        plane
    }

    fn current_browse_plane(&self) -> Option<&Entity<BrowseSplitPlane>> {
        match SessionPage::from(self.route) {
            SessionPage::Browse(kind) => self.browse_planes.get(&kind),
            SessionPage::Trash => None,
        }
    }

    pub(super) fn saved_pages(&self, cx: &App) -> Vec<BrowsePageSession> {
        self.current_browse_plane().map(|plane| plane.read(cx).saved_pages(cx)).unwrap_or_default()
    }

    pub(super) fn restore(&mut self, route: LibraryRoute, pages: Vec<BrowsePageSession>, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(route, window, cx);
        if let SessionPage::Browse(kind) = SessionPage::from(route) {
            self.browse_plane(kind, cx).update(cx, |plane, cx| plane.restore(pages, cx));
        }
    }

    pub(super) fn route(&self) -> LibraryRoute {
        self.route
    }

    pub(super) fn dismiss_page(&mut self, cx: &mut Context<Self>) -> bool {
        self.current_browse_plane().cloned().is_some_and(|plane| plane.update(cx, |plane, cx| plane.dismiss(cx)))
    }

    pub(super) fn open_folder(&mut self, directory_id: sync_common::DirId, title: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Folders, window, cx);
        self.browse_plane(BrowseKind::Folder, cx).update(cx, |plane, cx| plane.open_folder(directory_id, title, cx));
    }

    pub(super) fn open_subject(&mut self, location: library_model::BrowseLocation, label: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Subjects, window, cx);
        self.browse_plane(BrowseKind::Subject, cx).update(cx, |plane, cx| plane.open_location(location, label, cx));
    }

    fn open_author(&mut self, author: book_model::AuthorId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Authors, window, cx);
        self.browse_plane(BrowseKind::Author, cx).update(cx, |plane, cx| plane.open_location(library_model::BrowseLocation::Author(Some(author)), name, cx));
    }

    pub(super) fn shown_book_detail(&self, cx: &App) -> Option<library_model::BookLocator> {
        self.current_browse_plane()?.read(cx).shown_book_detail(cx)
    }

    pub(super) fn open_book_detail(&mut self, content_hash: sync_common::BookId, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Folders, window, cx);
        let backend = self.library.backend().clone();
        let plane = self.browse_plane(BrowseKind::Folder, cx);
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
        let focus = match SessionPage::from(self.route) {
            SessionPage::Browse(kind) => self.browse_planes.get(&kind).map(|plane| plane.read(cx).focus_handle()),
            SessionPage::Trash => self.trash.as_ref().map(|page| page.read(cx).focus_handle(cx)),
        };
        focus.unwrap_or_else(|| self.focus.clone())
    }

    pub(super) fn open_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match SessionPage::from(self.route) {
            SessionPage::Browse(kind) => self.browse_plane(kind, cx).update(cx, |plane, cx| plane.open_root(cx)),
            SessionPage::Trash => self.trash = None,
        }
        self.navigate(self.route, window, cx);
    }

    pub(super) fn navigate(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        match SessionPage::from(route) {
            SessionPage::Browse(kind) => {
                self.browse_plane(kind, cx);
            }
            SessionPage::Trash => {
                self.trash_page(cx);
            }
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
        let page = SessionPage::from(self.route);
        let uses_contents = matches!(page, SessionPage::Browse(_));
        // Keep import destinations and the catalog mounted even when the local
        // library is empty: catalog books do not belong to it yet.
        let content = if !(uses_contents && !matches!(self.route, LibraryRoute::Folders | LibraryRoute::Books | LibraryRoute::Catalog) && !updates.loading() && updates.book_count() == Some(0) && !updates.scanning()) {
            let view = match page {
                SessionPage::Browse(kind) => self.browse_plane(kind, cx).into_any_element(),
                SessionPage::Trash => self.trash_page(cx).into_any_element(),
            };
            ui_components::library_section_content().debug_selector(|| "library-session-page".into()).child(view).into_any_element()
        } else {
            let theme = ui_components::browser_theme(cx);
            let session = cx.entity();
            let actions = div()
                .flex()
                .items_center()
                .justify_center()
                .when(ui_components::uses_mobile_navigation(window), |actions| actions.flex_col())
                .gap(px(ui_components::SPACE_SM))
                .child(ui_components::primary_action_button("empty-library-add-book", "Add book from device", IconName::Plus, false).on_click(move |_, _, cx| {
                    session.update(cx, |session, cx| session.choose_book(cx));
                }))
                .child(ui_components::secondary_action_button("empty-library-catalog", "Go to catalog", IconName::BookOpen, false).on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::OpenLibraryView(LibraryRoute::Catalog)), cx);
                }));
            // The browse page's own heading, which an empty view still needs to
            // reach the others, which may not be empty.
            let heading = (ui_components::uses_mobile_navigation(window) && uses_contents).then(|| ui_components::topbar(theme).child(super::library_root_heading(self.route, theme)));
            ui_components::library_section_content()
                .debug_selector(|| "library-session-empty".into())
                .children(heading)
                .child(ui_components::remaining_space_column().child(ui_components::browser_actionable_empty_without_description("No books in this library", actions, theme).size_full()))
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

        for (route, empty_library) in [(LibraryRoute::Home, true), (LibraryRoute::Catalog, false), (LibraryRoute::Authors, true), (LibraryRoute::Books, false), (LibraryRoute::Folders, false), (LibraryRoute::Subjects, true)] {
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

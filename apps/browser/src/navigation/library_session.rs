//! Persistent pages owned by one library session.

use ui_text::text;

use std::collections::HashMap;
use std::rc::Rc;

#[cfg(test)]
use app::AppClient;
use gpui::prelude::*;
use gpui::{AnyView, App, AppContext, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div, px};
use gpui_component::IconName;
use library_backend::LibraryClient;
use library_management::management::ManagementClient;
use library_model::{BrowseLocation, BrowsePathSegment, LibraryBusyReason};
#[cfg(test)]
use sync_common::LibraryId;
use sync_common::{DirId, ROOT_DIR_ID};

use super::LibraryRoute;
use crate::library::{BrowseKind, BrowsePage, LibraryContext, LibraryUpdateBridge, NotebookPage, OpenBook, TrashPage, UndoEntry, UndoStack};
use crate::{BrowsePageSession, PaneRequest};

#[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod reader_import;

pub(crate) struct LibrarySession {
    operation_dialog: Option<LibraryDialog>,
    #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    reader_import: Option<(Entity<reader_import::ReaderImportDialog>, Subscription)>,
    busy_focus: FocusHandle,
    undo: UndoStack,
    library: LibraryContext,
    route: LibraryRoute,
    trash: Option<Entity<TrashPage>>,
    notebook: Option<Entity<NotebookPage>>,
    text_search: Option<Entity<NotebookPage>>,
    browse_pages: HashMap<BrowseKind, (Entity<BrowsePage>, [Subscription; 2])>,
    _library_updates_subscription: Subscription,
}

struct LibraryDialog {
    view: AnyView,
    width: f32,
    focus: FocusHandle,
    dismiss: Box<dyn Fn(&mut Window, &mut App)>,
    _dismiss: Subscription,
}

/// Library dialogs decide when pending writes allow them to close.
pub(crate) trait LibraryOperationDialog: Render + Focusable + EventEmitter<DismissEvent> {
    fn dismiss(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl gpui::EventEmitter<PaneRequest> for LibrarySession {}

impl LibrarySession {
    #[cfg(test)]
    pub(super) fn new(management: ManagementClient, open_book: OpenBook, library_id: LibraryId, cx: &mut Context<Self>) -> Self {
        let now_playing = cx.new(|_| crate::NowPlaying::default());
        let library = management.library(library_id).unwrap();
        Self::with_now_playing(management, open_book, library, now_playing, cx)
    }

    pub(super) fn with_now_playing(management: ManagementClient, open_book: OpenBook, library: LibraryClient, now_playing: Entity<crate::NowPlaying>, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<crate::stores::preferences::TextSearchEnabled>(|session: &mut Self, cx| {
            if !crate::stores::preferences::text_search_enabled(cx) {
                session.text_search = None;
                if session.route == LibraryRoute::Search {
                    session.route = LibraryRoute::Home;
                }
                cx.notify();
            }
        })
        .detach();
        let library_backend = Rc::new(library);
        let updates = LibraryUpdateBridge::create(library_backend.as_ref().clone(), cx);
        let library = LibraryContext::new(management, library_backend, open_book, updates, cx.entity().downgrade(), now_playing);
        let library_updates = library.updates();
        let library_updates_subscription = cx.observe(&library_updates, |_, _, cx| cx.notify());
        let mut session = Self {
            operation_dialog: None,
            #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            reader_import: None,
            busy_focus: cx.focus_handle(),
            undo: Default::default(),
            library,
            route: LibraryRoute::Home,
            trash: None,
            notebook: None,
            text_search: None,
            browse_pages: HashMap::new(),
            _library_updates_subscription: library_updates_subscription,
        };
        session.browse_page(BrowseKind::Home, cx);
        session
    }

    /// Reservations apply only to pages in this library, never its switcher.
    pub(super) fn blocks_navigation(&self, cx: &App) -> bool {
        if self.operation_dialog.is_some() || self.library.updates().read(cx).busy_reason().is_some() {
            return true;
        }
        #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        if self.reader_import.as_ref().is_some_and(|(dialog, _)| dialog.read(cx).importing) {
            return true;
        }
        false
    }

    pub(crate) fn show_operation_dialog<V: LibraryOperationDialog>(&mut self, view: Entity<V>, width: f32, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Preserve an existing draft when another command opens a dialog.
        if self.operation_dialog.is_some() {
            if activate {
                self.focus_page(window, cx);
            }
            return;
        }
        let focus = view.read(cx).focus_handle(cx);
        let dismiss = cx.subscribe_in(&view, window, |session, view, _: &DismissEvent, window, cx| {
            if session.operation_dialog.as_ref().is_none_or(|dialog| dialog.view.entity_id() != view.entity_id()) {
                return;
            }
            let restore_focus = session.operation_dialog.as_ref().is_some_and(|dialog| dialog.focus.contains_focused(window, cx));
            session.operation_dialog = None;
            if restore_focus {
                session.focus_page(window, cx);
            }
            cx.notify();
        });
        let target = view.downgrade();
        let request_dismiss = Box::new(move |window: &mut Window, cx: &mut App| {
            let _ = target.update(cx, |view, cx| view.dismiss(window, cx));
        });
        self.operation_dialog = Some(LibraryDialog { view: view.into(), width, focus: focus.clone(), dismiss: request_dismiss, _dismiss: dismiss });
        if activate {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn dismiss_operation_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.library.updates().read(cx).busy_reason().is_none()
            && let Some(dialog) = &self.operation_dialog
        {
            (dialog.dismiss)(window, cx);
        }
    }

    pub(crate) fn push_undo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
    }

    pub(crate) fn take_undo(&mut self) -> Option<UndoEntry> {
        self.undo.take()
    }

    fn trash_page(&mut self, cx: &mut Context<Self>) -> Entity<TrashPage> {
        let library = self.library.clone();
        self.trash.get_or_insert_with(|| cx.new(|cx| TrashPage::new(library, cx))).clone()
    }

    fn notebook_page(&mut self, cx: &mut Context<Self>) -> Entity<NotebookPage> {
        let library = self.library.clone();
        self.notebook.get_or_insert_with(|| cx.new(|cx| NotebookPage::new(library, cx))).clone()
    }

    fn text_search_page(&mut self, cx: &mut Context<Self>) -> Entity<NotebookPage> {
        let library = self.library.clone();
        self.text_search.get_or_insert_with(|| cx.new(|cx| NotebookPage::text_search(library, cx))).clone()
    }

    fn browse_page(&mut self, kind: BrowseKind, cx: &mut Context<Self>) -> Entity<BrowsePage> {
        if let Some((page, _)) = self.browse_pages.get(&kind) {
            return page.clone();
        }
        let library = self.library.clone();
        let page = cx.new(|cx| BrowsePage::new(kind, library, cx));
        let mut saved = page.read(cx).saved_session(cx);
        let subscriptions = [
            cx.observe(&page, move |_, page, cx| {
                let next = page.read(cx).saved_session(cx);
                if saved != next {
                    saved = next;
                    cx.notify();
                }
            }),
            cx.subscribe(&page, |_, _, request: &PaneRequest, cx| cx.emit(request.clone())),
        ];
        self.browse_pages.insert(kind, (page.clone(), subscriptions));
        page
    }

    fn current_browse_page(&self) -> Option<&Entity<BrowsePage>> {
        self.route.browse_kind().and_then(|kind| self.browse_pages.get(&kind).map(|(page, _)| page))
    }

    pub(super) fn saved_pages(&self, cx: &App) -> Vec<BrowsePageSession> {
        self.current_browse_page().map(|page| vec![page.read(cx).saved_session(cx)]).unwrap_or_default()
    }

    pub(super) fn restore(&mut self, route: LibraryRoute, pages: Vec<BrowsePageSession>, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(route, window, cx);
        if let Some(kind) = route.browse_kind()
            && let Some(saved) = pages.into_iter().next()
        {
            self.browse_page(kind, cx).update(cx, |page, cx| page.restore_session(saved, cx));
        }
    }

    pub(super) fn route(&self) -> LibraryRoute {
        self.route
    }

    pub(super) fn dismiss_page(&mut self, cx: &mut Context<Self>) -> bool {
        if self.library.updates().read(cx).busy_reason().is_some() {
            return true;
        }
        #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        if self.dismiss_reader_import(cx) {
            return true;
        }
        if self.route == LibraryRoute::Search {
            return self.text_search.as_ref().is_some_and(|page| page.update(cx, |page, cx| page.dismiss(cx)));
        }
        if self.route == LibraryRoute::Notebook {
            return self.notebook.as_ref().is_some_and(|page| page.update(cx, |page, cx| page.dismiss(cx)));
        }
        self.current_browse_page().cloned().is_some_and(|page| page.update(cx, |page, cx| page.dismiss(cx)))
    }

    pub(super) fn open_folder(&mut self, directory_id: DirId, title: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Folders, window, cx);
        let path = if directory_id == ROOT_DIR_ID { Vec::new() } else { vec![BrowsePathSegment { id: BrowseLocation::Folder(directory_id), name: title }] };
        self.browse_page(BrowseKind::Folder, cx).update(cx, |page, cx| page.open_path(path, cx));
    }

    pub(super) fn open_subject(&mut self, location: BrowseLocation, label: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Subjects, window, cx);
        self.browse_page(BrowseKind::Subject, cx).update(cx, |page, cx| page.open_path(vec![BrowsePathSegment { id: location, name: label }], cx));
    }

    fn open_author(&mut self, author: book_model::AuthorId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Authors, window, cx);
        self.browse_page(BrowseKind::Author, cx).update(cx, |page, cx| page.open_path(vec![BrowsePathSegment { id: BrowseLocation::Author(Some(author)), name: name }], cx));
    }

    /// A catalog subject opens on the Catalog page; every other on Subjects.
    fn open_subject_path(&mut self, path: Vec<BrowsePathSegment>, window: &mut Window, cx: &mut Context<Self>) {
        let (route, kind) = match path.first().map(|segment| segment.id) {
            Some(BrowseLocation::Catalog(_)) => (LibraryRoute::Catalog, BrowseKind::Catalog),
            _ => (LibraryRoute::Subjects, BrowseKind::Subject),
        };
        self.navigate(route, window, cx);
        self.browse_page(kind, cx).update(cx, |page, cx| page.open_path(path, cx));
    }

    pub(super) fn shown_book_detail(&self, cx: &App) -> Option<library_model::BookLocator> {
        self.current_browse_page()?.read(cx).shown_book_detail(cx)
    }

    pub(super) fn open_book_detail(&mut self, book_id: sync_common::BookId, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(LibraryRoute::Folders, window, cx);
        let backend = self.library.backend().clone();
        let page = self.browse_page(BrowseKind::Folder, cx);
        cx.spawn(async move |session, cx| {
            let result = backend.book_detail(book_id).await;
            cx.defer_update(session, move |_session, cx| {
                if let Ok(detail) = result {
                    page.update(cx, |page, cx| page.open_book_detail(detail.book, detail.path, cx));
                }
            });
        })
        .detach();
    }

    fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        if self.library.updates().read(cx).busy_reason().is_some() {
            return Some(self.busy_focus.clone());
        }
        if let Some(dialog) = &self.operation_dialog {
            return Some(dialog.focus.clone());
        }
        #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        if let Some((dialog, _)) = &self.reader_import {
            return Some(dialog.read(cx).focus_handle(cx));
        }
        match self.route.browse_kind() {
            Some(kind) => self.browse_pages.get(&kind).map(|(page, _)| page.read(cx).focus_handle(cx)),
            None if self.route == LibraryRoute::Notebook => self.notebook.as_ref().map(|page| page.read(cx).focus_handle(cx)),
            None if self.route == LibraryRoute::Search => self.text_search.as_ref().map(|page| page.read(cx).focus_handle(cx)),
            None => self.trash.as_ref().map(|page| page.read(cx).focus_handle(cx)),
        }
    }

    pub(super) fn focus_page(&self, window: &mut Window, cx: &mut App) {
        if self.blocks_navigation(cx) {
            if let Some(handle) = self.focus_handle(cx) {
                handle.focus(window, cx);
            }
            return;
        }
        if self.route == LibraryRoute::Search {
            if let Some(page) = &self.text_search {
                page.update(cx, |page, cx| page.refresh(cx));
            }
        }
        if self.route == LibraryRoute::Notebook {
            self.refresh_notebook(cx);
        }
        if let Some(handle) = self.focus_handle(cx) {
            handle.focus(window, cx);
        }
    }

    pub(super) fn open_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.route.browse_kind() {
            Some(kind) => {
                self.browse_pages.remove(&kind);
            }
            None if matches!(self.route, LibraryRoute::Notebook | LibraryRoute::Search) => {}
            None => self.trash = None,
        }
        self.navigate(self.route, window, cx);
    }

    pub(super) fn navigate(&mut self, route: LibraryRoute, window: &mut Window, cx: &mut Context<Self>) {
        let route = if route == LibraryRoute::Search && !crate::stores::preferences::text_search_enabled(cx) { LibraryRoute::Home } else { route };
        match route.browse_kind() {
            Some(kind) => {
                self.browse_page(kind, cx);
            }
            None if route == LibraryRoute::Notebook => {
                self.notebook_page(cx);
            }
            None if route == LibraryRoute::Search => {
                self.text_search_page(cx);
            }
            None => {
                self.trash_page(cx);
            }
        }
        self.route = route;
        self.focus_page(window, cx);
        cx.notify();
    }

    pub(crate) fn refresh_notebook(&self, cx: &mut App) {
        if let Some(page) = &self.notebook {
            page.update(cx, |page, cx| page.refresh(cx));
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
    pub(crate) async fn test_context(cx: &mut gpui::TestAppContext) -> (tempfile::TempDir, Entity<Self>, LibraryContext, Rc<AppClient>) {
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, startup) = AppClient::start_native(launch).unwrap();
        let library = backend.management.create_library_with_asset_storage("UI regression fixture".into(), true).await.unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::init_page_scrolling(cx);
        });
        cx.update(|cx| {
            crate::AppServices::new(
                backend.clone(),
                crate::AppStartup {
                    default_library_root: startup.default_library_root,
                    dictionary_storage_root: startup.dictionary_storage_root,
                    preference_storage_root: startup.preference_storage_root,
                    libraries: startup.libraries,
                    browsing_preferences: startup.browsing_preferences,
                    account_status: startup.account_status,
                },
            )
            .configure(cx);
        });
        let backend = Rc::new(backend);
        let session = cx.new(|cx| Self::new(backend.management.clone(), Rc::new(|_, _, _, _, _| {}), *library.library_id(), cx));
        let context = cx.read_entity(&session, |session, _| session.library.clone());
        (directory, session, context, backend)
    }
}

impl Render for LibrarySession {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let updates = self.library.updates();
        let updates = updates.read(cx);
        let busy_label = match updates.busy_reason() {
            Some(LibraryBusyReason::MovingLibrary) => Some(text!(browse::moving_library)),
            Some(LibraryBusyReason::RecoveringLibrary) => Some(text!(common::library_folder_unavailable_reconnect_its_drive_or_choose_another_library)),
            _ => None,
        };
        if let Some(label) = busy_label {
            return ui_components::library_section_content()
                .debug_selector(|| "library-operation".into())
                .track_focus(&self.busy_focus)
                .items_center()
                .justify_center()
                .occlude()
                .child(ui_components::browser_loading_message(label, ui_components::browser_theme(cx)))
                .into_any_element();
        }
        if let Some(dialog) = &self.operation_dialog {
            let theme = ui_components::browser_theme(cx);
            return ui_components::library_section_content()
                .bg(theme.overlay_scrim)
                .p(px(ui_components::SPACE_MD))
                .items_center()
                .justify_center()
                .occlude()
                .child(ui_components::modal_surface(theme).id("library-operation-dialog").role(gpui::Role::Dialog).w(px(dialog.width)).max_w_full().max_h_full().flex().flex_col().child(dialog.view.clone()))
                .into_any_element();
        }
        #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        if let Some((dialog, _)) = &self.reader_import {
            let theme = ui_components::browser_theme(cx);
            return ui_components::library_section_content()
                .debug_selector(|| "library-reader-import".into())
                .bg(theme.overlay_scrim)
                .p(px(ui_components::SPACE_MD))
                .items_center()
                .justify_center()
                .occlude()
                .child(ui_components::modal_surface(theme).w(px(420.0)).max_w_full().max_h_full().flex().flex_col().child(dialog.clone()))
                .into_any_element();
        }
        let kind = self.route.browse_kind();
        // Keep import destinations and the catalog mounted even when the local
        // library is empty: catalog books do not belong to it yet.
        let content = if !(matches!(self.route, LibraryRoute::Home | LibraryRoute::Subjects | LibraryRoute::Authors) && !updates.loading() && updates.book_count() == Some(0) && !updates.scanning()) {
            let view = match kind {
                Some(kind) => self.browse_page(kind, cx).into_any_element(),
                None if self.route == LibraryRoute::Notebook => self.notebook_page(cx).into_any_element(),
                None if self.route == LibraryRoute::Search => self.text_search_page(cx).into_any_element(),
                None => self.trash_page(cx).into_any_element(),
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
                .child(ui_components::primary_action_button("empty-library-add-book", text!(browse::add_book_from_device), IconName::Plus, false).on_click(move |_, _, cx| {
                    session.update(cx, |session, cx| session.library.choose_and_import_book(ROOT_DIR_ID, cx));
                }))
                .child(ui_components::secondary_action_button("empty-library-catalog", text!(browse::go_to_catalog), IconName::BookOpen, false).on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::OpenLibraryView(LibraryRoute::Catalog)), cx);
                }));
            // The browse page's own heading, which an empty view still needs to
            // reach the others, which may not be empty.
            let heading = ui_components::uses_mobile_navigation(window).then(|| ui_components::topbar(theme).child(super::library_root_heading(self.route, theme)));
            ui_components::library_section_content()
                .debug_selector(|| "library-session-empty".into())
                .children(heading)
                .child(ui_components::remaining_space_column().child(ui_components::browser_actionable_empty(text!(browse::no_books_in_this_library), actions, theme).size_full()))
                .into_any_element()
        };
        ui_components::library_section_content()
            .on_action(cx.listener(|session, _: &crate::OpenTrash, window, cx| session.navigate(LibraryRoute::Trash, window, cx)))
            .on_action(cx.listener(|session, open: &crate::OpenAuthor, window, cx| session.open_author(open.author, open.name.clone(), window, cx)))
            .on_action(cx.listener(|session, open: &crate::OpenSubject, window, cx| session.open_subject_path(open.path.clone(), window, cx)))
            .child(content)
            .into_any_element()
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests {
    use super::*;
    use crate::library::{UndoEntry, UndoStep};
    use gpui::{TestAppContext, VisualTestContext};
    use std::cell::Cell;

    #[gpui::test]
    async fn navigation_and_root_reset_focus_the_selected_page_for_every_route(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, session, _library, _app_client) = LibrarySession::test_context(cx).await;
        let window = cx.add_window(|_, _| gpui::Empty);
        for route in [LibraryRoute::Home, LibraryRoute::Books, LibraryRoute::Folders, LibraryRoute::Subjects, LibraryRoute::Authors, LibraryRoute::Catalog, LibraryRoute::Trash, LibraryRoute::Notebook] {
            window
                .update(cx, |_, window, cx| {
                    session.update(cx, |session, cx| {
                        session.navigate(route, window, cx);
                        let previous = session.focus_handle(cx).expect("the selected page has a focus handle");
                        assert!(previous.is_focused(window), "navigation to {route:?} must focus its page");
                        session.open_root(window, cx);
                        assert_eq!(previous.is_focused(window), route == LibraryRoute::Notebook, "Notebook refreshes its existing page; other routes reset theirs");
                        assert!(session.focus_handle(cx).is_some_and(|handle| handle.is_focused(window)));
                        assert_eq!(session.route(), route);
                        assert_eq!(session.saved_pages(cx).len(), usize::from(route.browse_kind().is_some()));
                    })
                })
                .unwrap();
        }
    }

    #[gpui::test]
    async fn cached_pages_keep_navigation_and_root_reset_disconnects_the_old_page(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, session, library, _app_client) = LibrarySession::test_context(cx).await;
        library.backend().create_directory(ROOT_DIR_ID, "Nested".into()).await.unwrap();
        let folder = library.backend().folder_destinations().await.unwrap().into_iter().find(|folder| folder.label == "Nested").unwrap().id;
        let window = cx.add_window(|_, _| gpui::Empty);
        window.update(cx, |_, window, cx| session.update(cx, |session, cx| session.open_folder(folder, "Nested".into(), window, cx))).unwrap();
        let original = session.read_with(cx, |session, _| session.browse_pages[&BrowseKind::Folder].0.clone());
        window
            .update(cx, |_, window, cx| {
                session.update(cx, |session, cx| {
                    session.navigate(LibraryRoute::Authors, window, cx);
                    session.navigate(LibraryRoute::Folders, window, cx);
                    assert_eq!(session.browse_pages[&BrowseKind::Folder].0, original);
                    assert_eq!(session.saved_pages(cx)[0].path, [(BrowseLocation::Folder(folder), "Nested".into())]);
                })
            })
            .unwrap();
        let received = Rc::new(Cell::new(0));
        let target = received.clone();
        let _subscription = cx.update(|cx| cx.subscribe(&session, move |_, _: &PaneRequest, _| target.set(target.get() + 1)));
        original.update(cx, |_, cx| cx.emit(PaneRequest::Close));
        assert_eq!(received.get(), 1);
        window.update(cx, |_, window, cx| session.update(cx, |session, cx| session.open_root(window, cx))).unwrap();
        let replacement = session.read_with(cx, |session, cx| {
            assert!(session.saved_pages(cx)[0].path.is_empty());
            session.browse_pages[&BrowseKind::Folder].0.clone()
        });
        assert_ne!(replacement, original);
        // Keep the original entity alive: removal must disconnect its events,
        // rather than relying on the old page being destroyed immediately.
        original.update(cx, |_, cx| cx.emit(PaneRequest::Close));
        replacement.update(cx, |_, cx| cx.emit(PaneRequest::Close));
        assert_eq!(received.get(), 2);
    }

    #[gpui::test]
    async fn empty_library_keeps_catalog_and_import_pages_visible(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cx.executor().allow_parking();
        let directory = tempfile::tempdir().unwrap();
        let launch = app::host::BackendLaunch::initialize(app::AppDataLocation::native_path(directory.path())).unwrap();
        let (backend, startup) = AppClient::start_native(launch).unwrap();
        let library = backend.management.create_library_with_asset_storage("Empty".into(), false).await.unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::init_page_scrolling(cx);
        });
        cx.update(|cx| {
            crate::AppServices::new(
                backend.clone(),
                crate::AppStartup {
                    default_library_root: startup.default_library_root,
                    dictionary_storage_root: startup.dictionary_storage_root,
                    preference_storage_root: startup.preference_storage_root,
                    libraries: startup.libraries,
                    browsing_preferences: startup.browsing_preferences,
                    account_status: startup.account_status,
                },
            )
            .configure(cx);
        });
        let window = cx.add_window(|_, cx| LibrarySession::new(backend.management.clone(), Rc::new(|_, _, _, _, _| {}), *library.library_id(), cx));
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
            (LibraryRoute::Trash, false),
            (LibraryRoute::Notebook, false),
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
        let (backend, startup) = AppClient::start_native(launch).unwrap();
        let library = backend.management.create_library_with_asset_storage("Undo".into(), true).await.unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            ui_components::init_page_scrolling(cx);
        });
        cx.update(|cx| {
            crate::AppServices::new(
                backend.clone(),
                crate::AppStartup {
                    default_library_root: startup.default_library_root,
                    dictionary_storage_root: startup.dictionary_storage_root,
                    preference_storage_root: startup.preference_storage_root,
                    libraries: startup.libraries,
                    browsing_preferences: startup.browsing_preferences,
                    account_status: startup.account_status,
                },
            )
            .configure(cx);
        });
        let window = cx.add_window(|_, cx| LibrarySession::new(backend.management.clone(), Rc::new(|_, _, _, _, _| {}), *library.library_id(), cx));
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

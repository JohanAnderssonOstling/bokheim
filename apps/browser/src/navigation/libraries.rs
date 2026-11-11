//! Cached page sessions and coarse lifecycle operations for every library.

use ui_text::text;

use crate::services::{import_failure_message, notify_warning};
use library_backend::{ImportFailure, ROOT_DIR_ID};
use library_management::management::ManagementClient;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use app::{AppClient, LibraryEntry, LibraryFolderError, LibraryFolderStep};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Task, Window};
use gpui_component::menu::PopupMenu;
use sync_common::LibraryId;
use ui_components::user_facing_error;

use super::library_errors::folder_add_message;
use super::{LibraryOperationDialog, LibrarySession};
use crate::library::OpenBook;

pub(crate) struct Libraries {
    backend: Rc<AppClient>,
    open_book: OpenBook,
    sessions: HashMap<LibraryId, Entity<LibrarySession>>,
    now_playing: Entity<crate::NowPlaying>,
    entries: Vec<LibraryEntry>,
    selected_id: Option<LibraryId>,
    pub(super) selected_error: Option<app::BackendError>,
    creating_libraries: usize,
    selection_epoch: Rc<Cell<u64>>,
    _list_updates: Task<()>,
}

pub(super) struct LibraryDialogOpened {
    pub library_id: LibraryId,
    pub selection_epoch: u64,
}
impl EventEmitter<LibraryDialogOpened> for Libraries {}

fn selected_library_id(libraries: &[LibraryEntry], current: Option<LibraryId>, preferred: Option<LibraryId>) -> Option<LibraryId> {
    preferred
        .filter(|id| libraries.iter().any(|library| library.library_id() == id))
        .or_else(|| current.filter(|id| libraries.iter().any(|library| library.library_id() == id)))
        .or_else(|| libraries.first().map(|library| *library.library_id()))
}

impl Libraries {
    pub(super) fn new(backend: Rc<AppClient>, open_book: OpenBook, entries: Vec<LibraryEntry>, library_id: Option<LibraryId>, cx: &mut Context<Self>) -> Self {
        let now_playing = cx.new(|_| crate::NowPlaying::default());
        let selection_epoch = Rc::new(Cell::new(0));
        let list_epoch = selection_epoch.clone();
        let list_backend = backend.management.clone();
        let list_updates = cx.spawn(async move |sessions, cx| {
            let mut changes = list_backend.library_list_changes();
            // Read after subscribing so discovery between startup and this
            // subscription cannot leave the tab with an outdated initial list.
            loop {
                let epoch = list_epoch.get();
                let result = list_backend.libraries().await;
                // If selection changed while reading, obtain a fresh snapshot
                // rather than losing the notification that triggered this read.
                if epoch != list_epoch.get() {
                    continue;
                }
                if sessions.update_in(cx, |sessions, window, cx| sessions.apply_library_list_result(epoch, result, window, cx)).is_err() {
                    return;
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        // The shared backend scheduler owns startup reconciliation. Each tab
        // uses the initial local list and subscribes to discovery updates.
        let mut libraries = Self { backend, open_book, sessions: HashMap::new(), now_playing, entries, selected_id: library_id, selected_error: None, creating_libraries: 0, selection_epoch, _list_updates: list_updates };
        if let Some(id) = library_id {
            libraries.set_selected_library(id, cx);
        }
        libraries
    }

    pub(crate) fn set_now_playing(&self, book: Option<crate::NowPlayingBook>, cx: &mut App) {
        self.now_playing.update(cx, |playing, cx| {
            playing.book = book;
            cx.notify();
        });
    }

    pub(crate) fn creating_libraries(&self) -> bool {
        self.creating_libraries > 0
    }

    pub(crate) fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub(crate) fn annotation_export_available(&self) -> bool {
        cfg!(not(target_arch = "wasm32"))
    }

    pub(crate) fn annotation_export_menu(libraries: Entity<Self>, library_id: LibraryId, menu: PopupMenu, cx: &App) -> PopupMenu {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let host = libraries.read(cx);
            let Some(entry) = host.entries.iter().find(|entry| *entry.library_id() == library_id) else { return menu };
            let backend = host.backend.clone();
            let name = entry.library_name().to_owned();
            menu.item(ui_components::menu_item(text!(browse::export_annotations), move |_, window, cx| crate::universal::open_annotation_export(libraries.clone(), backend.management.clone(), library_id, name.clone(), window, cx)))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (libraries, library_id, cx);
            menu
        }
    }
    pub(crate) fn backend_client(&self) -> AppClient {
        self.backend.as_ref().clone()
    }
    pub(crate) fn selected_id(&self) -> Option<LibraryId> {
        self.selected_id
    }
    pub(crate) fn selection_epoch(&self) -> u64 {
        self.selection_epoch.get()
    }
    pub(crate) fn contains(&self, library_id: &LibraryId) -> bool {
        self.entries.iter().any(|entry| entry.library_id() == library_id)
    }
    pub(super) fn selected_session(&self) -> Option<Entity<LibrarySession>> {
        self.selected_id.and_then(|id| self.sessions.get(&id).cloned())
    }

    pub(super) fn selected_library_blocks_navigation(&self, cx: &App) -> bool {
        self.selected_session().is_some_and(|session| session.read(cx).blocks_navigation(cx))
    }

    pub(crate) fn show_operation_dialog<V: LibraryOperationDialog>(&mut self, library_id: LibraryId, view: Entity<V>, width: f32, selection_epoch: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.contains(&library_id) {
            return;
        }
        let session = match self.ensure_session(library_id, cx) {
            Ok(session) => session,
            Err(error) => {
                self.notify_error(error, window, cx);
                return;
            }
        };
        // A deferred menu action must not undo a more recent library selection.
        let activate = self.selection_epoch.get() == selection_epoch;
        if activate {
            self.select_library(library_id, cx);
        }
        session.update(cx, |session, cx| session.show_operation_dialog(view, width, activate, window, cx));
        if activate {
            cx.emit(LibraryDialogOpened { library_id, selection_epoch: self.selection_epoch.get() });
        }
    }

    /// Selects a library, refreshing the authoritative list first when needed.
    pub(crate) fn select_library(&mut self, library_id: LibraryId, cx: &mut Context<Self>) {
        // A list read started before this selection may not contain a newly
        // added folder. It must not switch us back to the default library.
        let epoch = self.selection_epoch.get().wrapping_add(1);
        self.selection_epoch.set(epoch);
        if self.selected_id == Some(library_id) && self.sessions.contains_key(&library_id) {
            return;
        }
        if self.contains(&library_id) {
            self.set_selected_library(library_id, cx);
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |sessions, cx| {
            let result = backend.management.libraries().await;
            let _ = sessions.update_in(cx, |sessions, window, cx| {
                if sessions.selection_epoch.get() != epoch {
                    return;
                }
                match result {
                    Ok(libraries) => {
                        sessions.reconcile(libraries, Some(library_id), cx);
                    }
                    Err(error) => sessions.notify_error(error, window, cx),
                }
            });
        })
        .detach();
    }

    pub(crate) fn save_library_name(&self, library_id: Option<LibraryId>, name: String, asset_storage_enabled: bool) -> impl std::future::Future<Output = Result<Option<LibraryId>, app::BackendError>> + use<> {
        let backend = self.backend.clone();
        async move {
            if let Some(id) = library_id {
                backend.management.rename_library(id, name).await?;
                Ok(None)
            } else {
                // No locator means the configured default library location.
                backend.management.create_library_with_asset_storage(name, asset_storage_enabled).await.map(|entry| Some(*entry.library_id()))
            }
        }
    }

    #[cfg(all(feature = "filesystem-libraries", not(target_arch = "wasm32")))]
    pub(crate) fn move_library(&mut self, library_id: LibraryId, destination: String, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(guard) = crate::services::library_removal::begin_library_removal(library_id, cx) else { return };
        let preparation = crate::services::library_removal::prepare_library_removal(library_id, cx);
        let backend = self.backend.clone();
        crate::services::notify_info("library-move", text!(browse::moving_library), window, cx);
        cx.spawn(async move |sessions, cx| {
            let _guard = guard;
            let result = crate::services::library_removal::prepared_removal(preparation, backend.management.move_library(library_id, destination)).await;
            let _ = sessions.update_in(cx, |sessions, window, cx| match result {
                Ok(entry) => {
                    sessions.selection_epoch.set(sessions.selection_epoch.get().wrapping_add(1));
                    if let Some(old) = sessions.entries.iter_mut().find(|old| *old.library_id() == library_id) {
                        *old = entry;
                    }
                    crate::services::notify_info("library-move", text!(browse::library_moved), window, cx);
                    cx.notify();
                }
                Err(error) => sessions.notify_error(error, window, cx),
            });
        })
        .detach();
    }

    pub(crate) fn delete_library(&mut self, library_id: LibraryId, cx: &mut Context<Self>) {
        let Ok(removal_guard) = crate::services::library_removal::begin_library_removal(library_id, cx) else {
            return;
        };
        let backend = self.backend.clone();
        let preparation = crate::services::library_removal::prepare_library_removal(library_id, cx);
        cx.spawn(async move |sessions, cx| {
            let _removal_guard = removal_guard;
            let result = crate::services::library_removal::prepared_removal(preparation, backend.management.delete_library(library_id)).await;
            let _ = sessions.update_in(cx, |sessions, window, cx| match result {
                Ok(()) => {
                    // Discard list reads started before this confirmed deletion.
                    sessions.selection_epoch.set(sessions.selection_epoch.get().wrapping_add(1));
                    let entries = sessions.entries.iter().filter(|entry| *entry.library_id() != library_id).cloned().collect();
                    sessions.reconcile(entries, None, cx);
                }
                Err(error) => sessions.notify_error(error, window, cx),
            });
        })
        .detach();
    }

    pub(crate) fn folder_management(&self) -> ManagementClient {
        self.backend.management.clone()
    }

    pub(crate) fn create_selected_libraries(&mut self, directories: Vec<gpui::SelectedDirectory>, asset_storage_enabled: bool, cx: &mut Context<Self>) {
        self.creating_libraries += 1;
        let selection_epoch = self.selection_epoch.get();
        cx.notify();
        let backend = self.backend.clone();
        cx.spawn(async move |sessions, cx| {
            let mut created = Vec::new();
            let mut errors = Vec::new();
            let mut selected_local = None;
            let locators = directories.iter().filter_map(|directory| directory.local_path.as_ref().map(|path| path.to_string_lossy().into_owned())).collect::<Vec<_>>();
            if !locators.is_empty() {
                match backend.management.create_libraries_with_asset_storage(locators, asset_storage_enabled).await {
                    Ok((ids, failures)) => {
                        selected_local = ids.last().copied();
                        errors.extend(failures);
                    }
                    Err(error) => errors.push(LibraryFolderError { folder_name: None, step: LibraryFolderStep::OpenLibrary, error }),
                }
            }
            for directory in directories.into_iter().filter(|directory| directory.local_path.is_none()) {
                let directory = crate::directory_import(directory);
                let name = directory.name.clone();
                match backend.management.create_library_with_asset_storage(name.clone(), asset_storage_enabled).await {
                    Ok(entry) => {
                        created.push((entry, directory));
                    }
                    Err(error) => errors.push(LibraryFolderError { folder_name: Some(name), step: LibraryFolderStep::OpenLibrary, error }),
                }
            }
            let selected = created.last().map(|(entry, _)| *entry.library_id()).or(selected_local);
            let _ = sessions.update_in(cx, |sessions, window, cx| {
                sessions.creating_libraries -= 1;
                for (entry, _) in &created {
                    let id = *entry.library_id();
                    // Creation already returned the authoritative identity.
                    // Do not wait for another list query before selecting it.
                    if !sessions.contains(&id) {
                        sessions.entries.push(entry.clone());
                    }
                }
                cx.notify();
                if !errors.is_empty() {
                    notify_warning("library-add-warning", folder_add_message(&errors), window, cx);
                }
                if let Some(library_id) = selected.filter(|_| sessions.selection_epoch.get() == selection_epoch) {
                    sessions.select_library(library_id, cx);
                }
            });

            for (entry, directory) in created {
                let library_id = *entry.library_id();
                let name = entry.library_name();
                let result = async { backend.management.library(library_id)?.import_directory(ROOT_DIR_ID, directory, false).await }.await;
                #[cfg(not(target_arch = "wasm32"))]
                match result {
                    Ok(failures) if !failures.is_empty() => {
                        let _ = sessions.update_in(cx, |_, window, cx| notify_warning("library-import-warning", import_failure_summary(&failures), window, cx));
                    }
                    Err(error) => {
                        let message = format!(text!(browse::couldn_t_import_books_from), user_facing_error(&error, ""), name = name);
                        let _ = sessions.update_in(cx, |_, window, cx| notify_warning("library-import-warning", message, window, cx));
                    }
                    _ => {}
                }
                #[cfg(target_arch = "wasm32")]
                let _ = (result, name); // Coordinator notifications own the outcome.
            }
        })
        .detach();
    }

    fn apply_library_list_result(&mut self, epoch: u64, result: Result<Vec<LibraryEntry>, app::BackendError>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_epoch.get() != epoch {
            return;
        }
        match result {
            Ok(libraries) => {
                self.reconcile(libraries, None, cx);
            }
            Err(error) => self.notify_error(error, window, cx),
        }
    }

    fn reconcile(&mut self, libraries: Vec<LibraryEntry>, preferred_library_id: Option<LibraryId>, cx: &mut Context<Self>) {
        let library_ids = libraries.iter().map(|entry| *entry.library_id()).collect::<Vec<_>>();
        self.sessions.retain(|library_id, _| library_ids.contains(library_id));
        let library_id = selected_library_id(&libraries, self.selected_id, preferred_library_id);
        let changed = library_id != self.selected_id;
        self.entries = libraries;
        if changed || library_id.is_some_and(|id| !self.sessions.contains_key(&id) && library_ids.contains(&id)) {
            match library_id {
                Some(id) => self.set_selected_library(id, cx),
                None => {
                    self.selected_id = None;
                    self.selected_error = None;
                }
            }
        }
        cx.notify();
    }

    fn set_selected_library(&mut self, library_id: LibraryId, cx: &mut Context<Self>) {
        self.selected_error = None;
        if self.entries.iter().any(|entry| *entry.library_id() == library_id) && !self.sessions.contains_key(&library_id) {
            if let Err(error) = self.ensure_session(library_id, cx) {
                self.selected_error = Some(error);
            }
        }
        self.selected_id = Some(library_id);
        cx.notify();
    }

    fn ensure_session(&mut self, library_id: LibraryId, cx: &mut Context<Self>) -> Result<Entity<LibrarySession>, app::BackendError> {
        if let Some(session) = self.sessions.get(&library_id) {
            return Ok(session.clone());
        }
        let library = self.backend.management.library(library_id)?;
        let session = cx.new(|cx| LibrarySession::with_now_playing(self.backend.management.clone(), self.open_book.clone(), library, self.now_playing.clone(), cx));
        self.sessions.insert(library_id, session.clone());
        Ok(session)
    }

    fn notify_error(&self, error: app::BackendError, window: &mut Window, cx: &mut App) {
        crate::services::notify_error("library-operation-error", user_facing_error(&error, text!(browse::could_not_finish_the_library_action_please_try_again)), window, cx);
    }
}

fn import_failure_summary(failures: &[ImportFailure]) -> String {
    let mut summary = failures.iter().take(3).map(import_failure_message).collect::<Vec<_>>().join(". ");
    if failures.len() > 3 {
        summary.push_str(&format!(text!(browse::more_imports_failed), failures.len() - 3));
    }
    summary
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests {
    use super::*;
    use gpui::{SelectedDirectory, TestAppContext};

    #[gpui::test]
    async fn library_records_survive_creation_rename_and_removal_without_losing_selection(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library, app_client) = LibrarySession::test_context(cx).await;
        let backend = app_client;
        let initial = backend.management.libraries().await.unwrap();
        let original = *library.backend().id();
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(view, window, cx)
        });
        let libraries = window.update(cx, |_, _, cx| cx.new(|cx| Libraries::new(backend.clone(), Rc::new(|_, _, _, _, _| {}), initial.clone(), Some(original), cx))).unwrap();
        libraries.read_with(cx, |libraries, _| {
            assert_eq!(libraries.entries()[0].storage_locator(), initial[0].storage_locator());
            assert_eq!(libraries.selected_id(), Some(original));
        });
        libraries.update(cx, |libraries, cx| {
            libraries.create_selected_libraries(vec![SelectedDirectory { local_path: None, name: "Provider library".into(), directories: Vec::new(), files: Vec::new() }], true, cx);
        });
        cx.condition(&libraries, |libraries, _| !libraries.creating_libraries() && libraries.entries().iter().any(|entry| entry.library_name() == "Provider library")).await;
        let created = backend.management.libraries().await.unwrap().into_iter().find(|entry| entry.library_name() == "Provider library").unwrap();
        let id = *created.library_id();
        libraries.read_with(cx, |libraries, _| {
            let entry = libraries.entries().iter().find(|entry| *entry.library_id() == id).unwrap();
            assert_eq!(entry.storage_locator(), created.storage_locator());
            assert_eq!(libraries.selected_id(), Some(id));
        });
        backend.management.rename_library(id, "Renamed library".into()).await.unwrap();
        cx.condition(&libraries, |libraries, _| libraries.entries().iter().any(|entry| *entry.library_id() == id && entry.library_name() == "Renamed library")).await;
        libraries.read_with(cx, |libraries, _| {
            let entry = libraries.entries().iter().find(|entry| *entry.library_id() == id).unwrap();
            assert_eq!(entry.storage_locator(), created.storage_locator());
            assert_eq!(libraries.selected_id(), Some(id));
        });
        libraries.update(cx, |libraries, cx| libraries.delete_library(id, cx));
        cx.condition(&libraries, |libraries, _| !libraries.contains(&id)).await;
        libraries.read_with(cx, |libraries, _| assert_eq!(libraries.selected_id(), Some(original)));
    }
}

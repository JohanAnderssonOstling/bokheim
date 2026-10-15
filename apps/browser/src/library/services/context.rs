use crate::BOOK_IMPORT_EXTENSIONS;
use crate::services::import_failure_message;
use gpui::{App, Context, Entity, FileDialogFilter, PathPromptOptions, WeakEntity, Window};
use library_backend::{DownloadState, LibraryClient};
use library_model::BookLocator;
use std::rc::Rc;
use sync_common::{ContentHash, DirId};

use super::{DownloadSubscription, LibraryUpdateBridge, UndoEntry};
use crate::navigation::LibrarySession;
pub(crate) type OpenBook = Rc<dyn Fn(BookLocator, String, Option<String>, &mut Window, &mut App)>;

/// UI state associated with one selected library.
///
/// Keeps the selected concrete backend together with GPUI-specific callbacks
/// and events.
#[derive(Clone)]
pub(crate) struct LibraryContext {
    /// The app, for what is the account's rather than this library's, such
    /// as its OPDS feeds.
    app: Rc<app::AppClient>,
    backend: Rc<LibraryClient>,
    open_book: OpenBook,
    updates: Entity<LibraryUpdateBridge>,
    session: WeakEntity<LibrarySession>,
}

impl LibraryContext {
    pub(crate) fn new(app: Rc<app::AppClient>, backend: Rc<LibraryClient>, open_book: OpenBook, updates: Entity<LibraryUpdateBridge>, session: WeakEntity<LibrarySession>) -> Self {
        Self { app, backend, open_book, updates, session }
    }

    pub(crate) fn app(&self) -> &app::AppClient {
        self.app.as_ref()
    }

    pub(crate) fn backend(&self) -> &LibraryClient {
        self.backend.as_ref()
    }

    /// Pages hold a weak command target; the session owns their undo history.
    pub(crate) fn session(&self) -> WeakEntity<LibrarySession> {
        self.session.clone()
    }

    pub(crate) fn take_undo(&self, cx: &mut App) -> Option<UndoEntry> {
        self.session.update(cx, |session, _| session.take_undo()).ok().flatten()
    }

    pub(crate) fn updates(&self) -> Entity<LibraryUpdateBridge> {
        self.updates.clone()
    }

    pub(crate) fn subscribe_download<T: 'static>(&self, content_hash: ContentHash, cx: &mut Context<T>, callback: impl FnMut(&mut T, &DownloadState, &mut Context<T>) + 'static) -> DownloadSubscription {
        let source = self.updates.update(cx, |updates, cx| updates.download_source(content_hash, cx));
        LibraryUpdateBridge::subscribe_download(source, cx, callback)
    }

    /// Prompts for one book and imports it into the requested folder.
    ///
    /// Successful imports are observed through [`LibraryUpdateBridge`], so
    /// callers do not need to reload their page explicitly.
    pub(crate) fn choose_and_import_book<C: 'static>(&self, parent_id: DirId, cx: &mut Context<C>) {
        let extensions = BOOK_IMPORT_EXTENSIONS.into_iter().map(str::to_owned).collect();
        let selection = cx.prompt_for_files_with_filters(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add book".into()) }, vec![FileDialogFilter { name: "Books".to_owned(), extensions }]);
        let library = self.backend.clone();
        cx.spawn(async move |view, cx| {
            let result = match selection.await {
                Ok(Ok(Some(files))) => match files.into_iter().next() {
                    Some(file) => {
                        let _progress = view.update_in(cx, |_, window, cx| crate::services::ImportProgressToast::start(window, cx)).ok();
                        let result = {
                            let mut source = file.source;
                            source.path = vec![file.name.clone()];
                            let directory = crate::directory_import(gpui::SelectedDirectory { local_path: None, name: file.name, directories: Vec::new(), files: vec![source] });
                            library.import_directory(parent_id, directory, false).await.and_then(|failures| if failures.is_empty() { Ok(()) } else { Err(failures.iter().map(import_failure_message).collect::<Vec<_>>().join("\n")) })
                        };
                        result.map(|_| ())
                    }
                    None => return,
                },
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = result {
                let _ = view.update_in(cx, |_, window, cx| {
                    crate::services::notify_error("add-book-error", format!("Could not add book: {error}"), window, cx);
                });
            }
        })
        .detach();
    }

    /// Imports a folder from the picker through the same copy flow as a drop.
    pub(crate) fn choose_and_import_folder<C: 'static>(&self, parent_id: DirId, cx: &mut Context<C>) {
        let extensions = BOOK_IMPORT_EXTENSIONS.into_iter();
        // Native document providers filter the files they enumerate. Include
        // audiobook members so the backend can classify the complete folder.
        #[cfg(not(target_arch = "wasm32"))]
        let extensions = extensions.chain(["mp3folder", "mp3", "nfo", "cue", "jpg", "jpeg", "png", "webp"]);
        let extensions = extensions.map(str::to_owned).collect();
        let selection = cx.prompt_for_directories_with_filters(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Add folder".into()) }, vec![FileDialogFilter { name: "Books".to_owned(), extensions }]);
        let library = self.backend.clone();
        cx.spawn(async move |view, cx| {
            let result = match selection.await {
                Ok(Ok(Some(directories))) => match directories.into_iter().next() {
                    Some(directory) => {
                        let _progress = view.update_in(cx, |_, window, cx| crate::services::ImportProgressToast::start(window, cx)).ok();
                        #[cfg(not(target_arch = "wasm32"))]
                        let result = match directory.local_path.as_ref() {
                            Some(path) => library.import_paths(parent_id, vec![path.clone()], false).await,
                            None => library.import_directory(parent_id, crate::directory_import(directory), true).await,
                        };
                        #[cfg(target_arch = "wasm32")]
                        let result = library.import_directory(parent_id, crate::directory_import(directory), true).await;
                        result
                    }
                    None => return,
                },
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = view.update_in(cx, |_, window, cx| match result {
                Ok(failures) if !failures.is_empty() => crate::services::notify_warning("add-folder-warning", failures.iter().map(import_failure_message).collect::<Vec<_>>().join("\n"), window, cx),
                Err(error) => crate::services::notify_error("add-folder-error", format!("Could not add folder: {error}"), window, cx),
                _ => {}
            });
        })
        .detach();
    }

    /// Copies or moves a desktop drop into the library, then imports it in place.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_dropped_paths<C: 'static>(&self, parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool, cx: &mut Context<C>) {
        let library = self.backend.clone();
        cx.spawn(async move |view, cx| {
            let _progress = view.update_in(cx, |_, window, cx| crate::services::ImportProgressToast::start(window, cx)).ok();
            let message = match library.import_paths(parent_id, paths, moving).await {
                Ok(failures) if failures.is_empty() => None,
                Ok(failures) => Some(failures.iter().map(import_failure_message).collect::<Vec<_>>().join("\n")),
                Err(error) => Some(error),
            };
            if let Some(message) = message {
                let _ = view.update_in(cx, |_, window, cx| crate::services::notify_warning("drop-import-warning", message, window, cx));
            }
        })
        .detach();
    }

    /// Opens the book where the reader asked for it — a contents entry rather
    /// than wherever they left off. The target is the book's own, so the
    /// reader that receives it is the one that knows how to resolve it.
    pub(crate) fn open_book_at(&self, content_hash: ContentHash, title: String, target: Option<String>, window: &mut Window, cx: &mut App) {
        let locator = BookLocator::new(*self.backend.id(), content_hash);
        (self.open_book)(locator, title, target, window, cx);
    }
}

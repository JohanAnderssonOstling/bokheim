use library_management::management::ManagementClient;
use ui_text::text;

use ui_components::user_facing_error;

use crate::BOOK_IMPORT_EXTENSIONS;
use crate::services::{import_failure_message, notify_error, notify_warning};
use gpui::{App, Context, Entity, FileDialogFilter, PathPromptOptions, WeakEntity, Window};
use library_backend::{BackendError, DownloadState, LibraryClient, ThumbnailResolution};
use library_model::{BookCardRow, BookDetail, BookLocator, BookSource};
use std::rc::Rc;
use sync_common::{BookId, DirId};

use super::{DownloadSubscription, LibraryUpdateBridge, UndoEntry};
use crate::navigation::LibrarySession;
pub(crate) type OpenBook = Rc<dyn Fn(BookLocator, String, Option<String>, &mut Window, &mut App)>;

/// UI state associated with one selected library.
///
/// Keeps the selected concrete backend together with GPUI-specific callbacks
/// and events.
#[derive(Clone)]
pub(crate) struct LibraryContext {
    management: ManagementClient,
    backend: Rc<LibraryClient>,
    open_book: OpenBook,
    updates: Entity<LibraryUpdateBridge>,
    session: WeakEntity<LibrarySession>,
    now_playing: Entity<crate::NowPlaying>,
}

#[cfg(test)]
pub(crate) fn empty_detail(book: BookCardRow, description: String) -> BookDetail {
    BookDetail {
        book,
        external_url: None,
        audiobook_chapter_count: None,
        description,
        subjects: Vec::new(),
        year: None,
        source_directory_id: None,
        path: Vec::new(),
        authors: Vec::new(),
        credits: Vec::new(),
        current_entry: None,
        current_target: None,
        chapter_read_status: Default::default(),
        toc: Vec::new(),
    }
}

impl LibraryContext {
    /// Resolves details identically for book pages and card overlays.
    pub(crate) async fn book_detail(self, book: BookCardRow) -> Result<BookDetail, BackendError> {
        match book.source {
            BookSource::Library { book_id } | BookSource::Catalog { book_id } => self.backend.book_detail(book_id).await,
            BookSource::ExternalCatalog { entry, .. } => self.backend.external_catalog_detail(entry).await,
        }
    }

    /// Resolution selects cropped card art or the unmodified detail cover.
    pub(crate) async fn book_cover(self, source: BookSource, resolution: ThumbnailResolution) -> Result<Option<Vec<u8>>, BackendError> {
        match source {
            BookSource::ExternalCatalog { entry, .. } => self.management.external_catalog_cover(entry, resolution).await,
            BookSource::Library { book_id } | BookSource::Catalog { book_id } => self.backend.thumbnail(book_id, resolution).await,
        }
    }

    pub(crate) fn new(management: ManagementClient, backend: Rc<LibraryClient>, open_book: OpenBook, updates: Entity<LibraryUpdateBridge>, session: WeakEntity<LibrarySession>, now_playing: Entity<crate::NowPlaying>) -> Self {
        Self { management, backend, open_book, updates, session, now_playing }
    }

    pub(crate) fn now_playing(&self) -> &Entity<crate::NowPlaying> {
        &self.now_playing
    }

    pub(crate) fn management(&self) -> &ManagementClient {
        &self.management
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

    pub(crate) fn files_busy(&self, cx: &App) -> bool {
        self.updates.read(cx).files_busy()
    }

    pub(crate) fn importing(&self, cx: &App) -> bool {
        self.updates.read(cx).importing()
    }

    pub(crate) fn subscribe_download<T: 'static>(&self, book_id: BookId, cx: &mut Context<T>, callback: impl FnMut(&mut T, &DownloadState, &mut Context<T>) + 'static) -> DownloadSubscription {
        let source = self.updates.update(cx, |updates, cx| updates.download_source(book_id, cx));
        LibraryUpdateBridge::subscribe_download(source, cx, callback)
    }

    /// Prompts for one book and imports it into the requested folder.
    ///
    /// Successful imports are observed through [`LibraryUpdateBridge`], so
    /// callers do not need to reload their page explicitly.
    pub(crate) fn choose_and_import_book<C: 'static>(&self, parent_id: DirId, cx: &mut Context<C>) {
        if self.files_busy(cx) || self.importing(cx) {
            return;
        }
        let extensions = BOOK_IMPORT_EXTENSIONS.into_iter().map(str::to_owned).collect();
        let selection = cx
            .prompt_for_files_with_filters(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(text!(browse::add_book).into()) }, vec![FileDialogFilter { name: text!(browse::books).to_owned(), extensions }]);
        let library = self.backend.clone();
        let updates = self.updates.downgrade();
        let task = cx.spawn(async move |_, cx| {
            let selected = selection.await.unwrap_or_else(|error| Err(error.into())).map(|files| files.and_then(|files| files.into_iter().next()));
            let result = match selected {
                Ok(Some(file)) => {
                    let mut source = file.source;
                    source.path = vec![file.name.clone()];
                    let directory = crate::directory_import(gpui::SelectedDirectory { local_path: None, name: file.name, directories: Vec::new(), files: vec![source] });
                    library.import_directory(parent_id, directory, false).await.map(|failures| failures.iter().map(import_failure_message).collect::<Vec<_>>().join("\n"))
                }
                Ok(None) => Ok(String::new()),
                Err(error) => Err(BackendError::message(error.to_string())),
            };
            let _ = updates.update_in(cx, |updates, window, cx| {
                updates.set_import_task(None, cx);
                match result {
                    // Import failures already carry their own wording.
                    Ok(failures) if !failures.is_empty() => notify_error("add-book-error", failures, window, cx),
                    Ok(_) => {}
                    Err(error) => notify_error("add-book-error", format!(text!(browse::could_not_add_book_generic), user_facing_error(&error, "")), window, cx),
                }
            });
        });
        self.updates.update(cx, |updates, cx| updates.set_import_task(Some(task), cx));
    }

    /// Imports a folder from the picker through the same copy flow as a drop.
    pub(crate) fn choose_and_import_folder<C: 'static>(&self, parent_id: DirId, cx: &mut Context<C>) {
        if self.files_busy(cx) || self.importing(cx) {
            return;
        }
        let extensions = BOOK_IMPORT_EXTENSIONS.into_iter();
        // Native document providers filter the files they enumerate. Include
        // audiobook members so the backend can classify the complete folder.
        #[cfg(not(target_arch = "wasm32"))]
        let extensions = extensions.chain(["mp3folder", "mp3", "nfo", "cue", "jpg", "jpeg", "png", "webp"]);
        let extensions = extensions.map(str::to_owned).collect();
        let selection = cx.prompt_for_directories_with_filters(
            PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(text!(browse::add_folder).into()) },
            vec![FileDialogFilter { name: text!(browse::books).to_owned(), extensions }],
        );
        let library = self.backend.clone();
        let updates = self.updates.downgrade();
        let task = cx.spawn(async move |_, cx| {
            let selected = selection.await.unwrap_or_else(|error| Err(error.into())).map(|directories| directories.and_then(|directories| directories.into_iter().next()));
            let result = match selected {
                Ok(Some(directory)) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    let result = match directory.local_path.as_ref() {
                        Some(path) => library.import_paths(parent_id, vec![path.clone()], false).await,
                        None => library.import_directory(parent_id, crate::directory_import(directory), true).await,
                    };
                    #[cfg(target_arch = "wasm32")]
                    let result = library.import_directory(parent_id, crate::directory_import(directory), true).await;
                    result
                }
                Ok(None) => Ok(Vec::new()),
                Err(error) => Err(BackendError::message(error.to_string())),
            };
            let _ = updates.update_in(cx, |updates, window, cx| {
                updates.set_import_task(None, cx);
                match result {
                    Ok(failures) if !failures.is_empty() => notify_warning("add-folder-warning", failures.iter().map(import_failure_message).collect::<Vec<_>>().join("\n"), window, cx),
                    Err(error) => notify_error("add-folder-error", format!(text!(browse::could_not_add_folder), user_facing_error(&error, "")), window, cx),
                    _ => {}
                }
            });
        });
        self.updates.update(cx, |updates, cx| updates.set_import_task(Some(task), cx));
    }

    /// Copies or moves a desktop drop into the library, then imports it in place.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_dropped_paths<C: 'static>(&self, parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool, cx: &mut Context<C>) {
        if self.files_busy(cx) || self.importing(cx) {
            return;
        }
        let library = self.backend.clone();
        let updates = self.updates.downgrade();
        let task = cx.spawn(async move |_, cx| {
            let message = match library.import_paths(parent_id, paths, moving).await {
                Ok(failures) if failures.is_empty() => None,
                Ok(failures) => Some(failures.iter().map(import_failure_message).collect::<Vec<_>>().join("\n")),
                Err(error) => Some(user_facing_error(&error, text!(browse::could_not_import_the_dropped_files_check_that_they_are_available_and)).to_owned()),
            };
            let _ = updates.update_in(cx, |updates, window, cx| {
                updates.set_import_task(None, cx);
                if let Some(message) = message {
                    notify_warning("drop-import-warning", message, window, cx);
                }
            });
        });
        self.updates.update(cx, |updates, cx| updates.set_import_task(Some(task), cx));
    }

    /// Opens the book where the reader asked for it — a contents entry rather
    /// than wherever they left off. The target is the book's own, so the
    /// reader that receives it is the one that knows how to resolve it.
    pub(crate) fn open_book_at(&self, book_id: BookId, title: String, target: Option<String>, window: &mut Window, cx: &mut App) {
        let locator = BookLocator::new(*self.backend.id(), book_id);
        (self.open_book)(locator, title, target, window, cx);
    }
}

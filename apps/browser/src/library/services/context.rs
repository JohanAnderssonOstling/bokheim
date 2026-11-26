use library_management::management::ManagementClient;
use ui_text::text;

use crate::BOOK_IMPORT_EXTENSIONS;
use crate::services::{book_transfer_error_message, notify_error};
use gpui::{App, Context, Entity, FileDialogFilter, PathPromptOptions, WeakEntity, Window};
use library_backend::{BackendError, DownloadState, ImportFailure, LibraryClient, ThumbnailResolution};
use library_model::{BookCardRow, BookDetail, BookLocator, BookSource};
use std::future::Future;
use std::rc::Rc;
use sync_common::{BookId, DirId};

use super::{CoverSubscription, DownloadSubscription, LibraryUpdateBridge, UndoEntry};
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

    pub(crate) fn subscribe_cover<T: 'static>(&self, book_id: BookId, cx: &mut Context<T>, callback: impl FnMut(&mut T, &mut Context<T>) + 'static) -> CoverSubscription {
        let source = self.updates.update(cx, |updates, cx| updates.cover_source(book_id, cx));
        LibraryUpdateBridge::subscribe_cover(source, cx, callback)
    }

    pub(crate) fn subscribe_download<T: 'static>(&self, book_id: BookId, cx: &mut Context<T>, callback: impl FnMut(&mut T, &DownloadState, &mut Context<T>) + 'static) -> DownloadSubscription {
        let source = self.updates.update(cx, |updates, cx| updates.download_source(book_id, cx));
        LibraryUpdateBridge::subscribe_download(source, cx, callback)
    }

    /// Request ownership and immediate queued state belong to the library,
    /// so replacing a card or detail page cannot cancel a download request.
    pub(crate) fn download_book(&self, book: BookId, cx: &mut App) {
        let updates = self.updates.read(cx);
        if updates.downloads_blocked() || !matches!(updates.download_state(book), Some(DownloadState::NotDownloaded)) {
            return;
        }
        self.updates.update(cx, |updates, cx| updates.set_download_state(book, DownloadState::Queued, cx));
        let library = self.backend.clone();
        let updates = self.updates.downgrade();
        cx.spawn(async move |cx| {
            if let Err(error) = library.download_book(book).await {
                let _ = updates.update(cx, |updates, cx| {
                    updates.set_download_state(book, DownloadState::NotDownloaded, cx);
                    let owner = cx.entity_id();
                    cx.with_window(owner, |window, cx| notify_error("book-download-error", book_transfer_error_message(error.failure()), window, cx));
                });
            }
        })
        .detach();
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
        let selection = cx.prompt_for_files_with_filters(
            PathPromptOptions { directory: None, files: true, directories: false, multiple: false, prompt: Some(text!(browse::add_book).into()) },
            vec![FileDialogFilter { name: text!(browse::books).to_owned(), extensions }],
        );
        let library = self.backend.clone();
        self.run_import(
            async move {
                let selected = selection.await.unwrap_or_else(|error| Err(error.into())).map_err(|error| BackendError::message(error.to_string()))?;
                let Some(file) = selected.and_then(|files| files.into_iter().next()) else { return Ok(Vec::new()) };
                let mut source = file.source;
                source.path = vec![file.name.clone()];
                let directory = crate::directory_import(gpui::SelectedDirectory { local_path: None, name: file.name, directories: Vec::new(), files: vec![source] });
                library.import_directory(parent_id, directory, false).await
            },
            cx,
        );
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn import_browser_files<C: 'static>(&self, parent_id: DirId, files: Vec<gpui::SelectedFile>, cx: &mut Context<C>) {
        if files.is_empty() {
            return;
        }
        let files = files
            .into_iter()
            .map(|file| {
                let mut source = file.source;
                source.path = vec![file.name];
                source
            })
            .collect();
        let directory = crate::directory_import(gpui::SelectedDirectory { local_path: None, name: String::new(), directories: vec![], files });
        let library = self.backend.clone();
        self.run_import(async move { library.import_directory(parent_id, directory, false).await }, cx);
    }

    /// Imports a folder from the picker through the same copy flow as a drop.
    pub(crate) fn choose_and_import_folder<C: 'static>(&self, parent_id: DirId, cx: &mut Context<C>) {
        if self.files_busy(cx) || self.importing(cx) {
            return;
        }
        let extensions = BOOK_IMPORT_EXTENSIONS.into_iter();
        #[cfg(not(target_arch = "wasm32"))]
        let extensions = extensions.chain(["mp3folder", "mp3", "nfo", "cue", "jpg", "jpeg", "png", "webp"]);
        let selection = cx.prompt_for_directories_with_filters(
            PathPromptOptions { directory: None, files: false, directories: true, multiple: false, prompt: Some(text!(browse::add_folder).into()) },
            vec![FileDialogFilter { name: text!(browse::books).to_owned(), extensions: extensions.map(str::to_owned).collect() }],
        );
        let library = self.backend.clone();
        self.run_import(
            async move {
                let selected = selection.await.unwrap_or_else(|error| Err(error.into())).map_err(|error| BackendError::message(error.to_string()))?;
                let Some(directory) = selected.and_then(|directories| directories.into_iter().next()) else { return Ok(Vec::new()) };
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = directory.local_path.as_ref() {
                    return library.import_paths(parent_id, vec![path.clone()], false).await;
                }
                library.import_directory(parent_id, crate::directory_import(directory), true).await
            },
            cx,
        );
    }

    /// Copies or moves a desktop drop into the library, then imports it in place.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_dropped_paths<C: 'static>(&self, parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool, cx: &mut Context<C>) {
        let library = self.backend.clone();
        self.run_import(async move { library.import_paths(parent_id, paths, moving).await }, cx);
    }

    /// One lifetime and feedback policy for imports from pickers and drops.
    fn run_import<C: 'static>(&self, operation: impl Future<Output = Result<Vec<ImportFailure>, BackendError>> + 'static, cx: &mut Context<C>) {
        if self.files_busy(cx) {
            return;
        }
        self.updates.update(cx, |updates, cx| updates.enqueue_import(Box::pin(operation), cx));
    }

    /// Opens the book where the reader asked for it — a contents entry rather
    /// than wherever they left off. The target is the book's own, so the
    /// reader that receives it is the one that knows how to resolve it.
    pub(crate) fn open_book_at(&self, book_id: BookId, title: String, target: Option<String>, window: &mut Window, cx: &mut App) {
        let locator = BookLocator::new(*self.backend.id(), book_id);
        (self.open_book)(locator, title, target, window, cx);
    }
}

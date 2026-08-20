use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(not(feature = "kobo"))]
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::AuthorId;
use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::CatalogAuthorDiscoveryView;
use epub_svelte_lib::app::executor::BackendTask;
use epub_svelte_lib::catalog::CatalogImportRequest;
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::{BookLocator, ContentHash, DirId, ROOT_DIR_ID};
use gpui::{App, Context, FileDialogFilter, PathPromptOptions, Window};

use crate::catalog::CatalogCardServices;
use crate::services::AppServices;

pub(crate) use epub_svelte_lib::library::{
    LibraryAuthorsView as AuthorBrowseRows, LibraryFolderView as FolderBrowseRows, LibraryHomeView as HomeBrowseRows, LibrarySearchResult as SearchBrowseRows, LibrarySubjectView as SubjectBrowseRows, LibraryTrashView as TrashBrowseRows,
};

/// Capabilities scoped to one selected library.
///
/// This is a plain value, not a GPUI entity. `BrowserRoot` creates one for the
/// current page from the selected backend. Pages use its typed operations
/// instead of assembling backend calls from several service wrappers.
#[derive(Clone)]
pub(crate) struct LibraryContext {
    app: AppServices,
    pub(crate) library: LibraryBackend,
    open_detail: Option<Rc<dyn Fn(ContentHash, &mut App)>>,
    toc_cache: Arc<std::sync::Mutex<HashMap<String, Vec<app_contract::library::BookTocEntry>>>>,
}

impl LibraryContext {
    pub(crate) fn new(app: AppServices, library: LibraryBackend) -> Self {
        Self { app, library, open_detail: None, toc_cache: Arc::new(std::sync::Mutex::new(HashMap::new())) }
    }

    pub(crate) fn set_open_detail(&mut self, open_detail: Rc<dyn Fn(ContentHash, &mut App)>) {
        self.open_detail = Some(open_detail);
    }

    pub(crate) fn open_detail(&self, content_hash: ContentHash, cx: &mut App) {
        if let Some(open) = &self.open_detail {
            open(content_hash, cx);
        }
    }

    pub(crate) fn load_book_detail(&self, content_hash: ContentHash) -> impl Future<Output = Result<epub_svelte_lib::library::BookDetail, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.book_detail_summary(content_hash));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_book_toc(&self, content_hash: ContentHash) -> impl Future<Output = Result<Vec<app_contract::library::BookTocEntry>, String>> + 'static {
        let library = self.library.clone();
        let cache = self.toc_cache.clone();
        async move {
            let key = content_hash.to_string();
            if let Some(toc) = cache.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
                return Ok(toc);
            }
            let operation = library.spawn_blocking(move |library| library.book_toc(content_hash));
            let toc = operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string())?;
            if let Ok(mut cache) = cache.lock() {
                cache.insert(key, toc.clone());
            }
            Ok(toc)
        }
    }

    pub(crate) fn contracts(&self) -> &Arc<FrontendContract> {
        &self.app.contracts
    }

    /// The reader's book-display preference, for grids created before any
    /// change event has been pushed down. Later changes arrive as
    /// `LibraryDisplayChanged` rather than being re-read here.
    pub(crate) fn library_view(&self) -> app_contract::preferences::LibraryView {
        self.app.backend.browsing_preferences().map(|preferences| preferences.library_view()).unwrap_or_default()
    }

    pub(crate) fn scanner_running(&self) -> bool {
        self.library.scanner_running()
    }

    pub(crate) fn persist_folder_path(&self, path: Vec<crate::model::FolderCrumb>) {
        if let Err(error) = self.app.backend.save_library_folder_path(self.library.id().clone(), path) {
            log::warn!("failed to persist folder path: {error}");
        }
    }

    pub(crate) fn persist_subject_path(&self, path: Vec<crate::model::SubjectCrumb>) {
        if let Err(error) = self.app.backend.save_library_subject_path(self.library.id().clone(), path) {
            log::warn!("failed to persist subject path: {error}");
        }
    }

    pub(crate) fn sync_server_connected(&self) -> bool {
        let network = self.app.backend.network_status();
        self.app.backend.account_status().map(|status| status.signed_in()).unwrap_or(false) && network.connected && network.validated != Some(false)
    }

    pub(crate) fn catalog_cards(&self) -> CatalogCardServices {
        let context = self.clone();
        let import = Rc::new(move |request| context.import_catalog_book(request));
        CatalogCardServices::new(&self.app, import)
    }

    pub(crate) fn with_window<R>(&self, cx: &mut App, body: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
        self.app.with_window(cx, body)
    }

    pub(crate) fn open_reader(&self, content_hash: ContentHash, title: String, cx: &mut App) {
        let locator = BookLocator::new(self.library.id().clone(), content_hash);
        (self.app.open_reader)(locator, title, None, cx);
    }

    pub(crate) fn open_reader_at(&self, content_hash: ContentHash, title: String, target: String, cx: &mut App) {
        let locator = BookLocator::new(self.library.id().clone(), content_hash);
        (self.app.open_reader)(locator, title, Some(target), cx);
    }

    pub(crate) fn load_cover(&self, content_hash: ContentHash) -> impl Future<Output = Result<Option<Vec<u8>>, String>> + 'static {
        let operation = self.library.load_thumbnail(content_hash);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    #[cfg(not(feature = "kobo"))]
    pub(crate) fn reveal_local_book(&self, content_hash: ContentHash) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let operation = self.library.spawn_blocking(move |library| library.local_book_path(content_hash));
        async move {
            let path = operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string())?.ok_or_else(|| "This book is not stored on this device".to_owned())?;
            reveal_in_file_manager(&path)
        }
    }

    pub(crate) fn load_home(&self, limit: i32) -> impl Future<Output = Result<HomeBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.home(limit));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_authors(&self, selected: Option<AuthorId>, sort: epub_svelte_lib::library::AuthorSort) -> impl Future<Output = Result<AuthorBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.authors(selected, sort));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_author_books(&self, author_id: AuthorId) -> impl Future<Output = Result<Vec<epub_svelte_lib::library::BookCardRow>, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.author_books(author_id));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_folder(&self, folder_id: DirId, file_type: epub_svelte_lib::library::LibraryFileTypeFilter) -> impl Future<Output = Result<FolderBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.folder_filtered(&folder_id, file_type));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_subjects(&self, subject_path: Option<String>, file_type: epub_svelte_lib::library::LibraryFileTypeFilter) -> impl Future<Output = Result<SubjectBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.subjects_filtered(subject_path.as_deref(), file_type));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn reload_unified_taxonomy(&self) -> impl Future<Output = Result<String, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.reload_unified_taxonomy());
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_edition_identity_reviews(&self) -> impl Future<Output = Result<Vec<epub_svelte_lib::library::EditionIdentityReview>, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.edition_identity_reviews());
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn refresh_edition_identity_candidate_classifications(&self) -> impl Future<Output = Result<usize, String>> + 'static {
        let operation = self.library.spawn(move |library| async move { library.refresh_edition_identity_candidate_classifications().await.map_err(|error| error.to_string()) });
        async move { operation.await.map_err(|error| error.to_string())? }
    }

    pub(crate) fn select_edition_identity_candidate(&self, content_hash: ContentHash, edition_id: String, isbn13: String) -> impl Future<Output = Result<bool, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.select_edition_identity_candidate(content_hash, &edition_id, &isbn13));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn apply_manual_edition_identity(&self, content_hash: ContentHash, isbn: String) -> impl Future<Output = Result<bool, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.apply_manual_edition_identity(content_hash, &isbn));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn reject_edition_identity_candidates(&self, content_hash: ContentHash) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.reject_edition_identity_candidates(content_hash));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn retry_failed_edition_identities(&self) -> impl Future<Output = Result<usize, String>> + 'static {
        let operation = self.library.spawn(move |library| async move { library.retry_failed_edition_identities().await.map_err(|error| error.to_string()) });
        async move { operation.await.map_err(|error| error.to_string())? }
    }

    /// Every book beneath `folder_id`, not just the ones placed directly in it.
    pub(crate) fn search_folder(&self, folder_id: DirId, query: String, file_type: epub_svelte_lib::library::LibraryFileTypeFilter) -> impl Future<Output = Result<SearchBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.search_folder_filtered(&folder_id, &query, file_type));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    /// Every book under `subject_path` and its narrower subjects. `None` is the
    /// whole library, which is what the root of the Subjects page shows.
    pub(crate) fn search_subject(&self, subject_path: Option<String>, query: String, file_type: epub_svelte_lib::library::LibraryFileTypeFilter) -> impl Future<Output = Result<SearchBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.search_subject_filtered(subject_path.as_deref(), &query, file_type));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn import_book(&self, folder_id: DirId, path: PathBuf, file_name: String) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.import_book_task(folder_id, path, file_name);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn choose_and_import_book<P>(&self, folder_id: DirId, cx: &mut Context<P>, finished: impl FnOnce(&mut P, Result<(), String>, &mut Context<P>) + 'static)
    where
        P: 'static,
    {
        let prompt = cx.prompt_for_paths_with_filters(
            PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add book".into()) },
            vec![FileDialogFilter { name: "Books and web publications".to_owned(), extensions: self.app.contracts.browsing_content.import_extensions.clone() }],
        );
        let context = self.clone();
        cx.spawn(async move |page, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else { return };
            let result = match paths.into_iter().next() {
                Some(path) => match path.file_name().and_then(|name| name.to_str()).map(str::to_owned) {
                    Some(file_name) => context.import_book(folder_id, path, file_name).await,
                    None => Err("Selected book has no valid file name".to_owned()),
                },
                None => return,
            };
            let _ = page.update(cx, |page, cx| finished(page, result, cx));
        })
        .detach();
    }

    pub(crate) fn create_folder(&self, parent_id: DirId, name: String) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.create_directory_task(parent_id, name);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn rename_folder(&self, folder_id: DirId, name: String) -> impl Future<Output = Result<String, String>> + 'static {
        let operation = self.library.rename_directory_task(folder_id, name);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn move_folder(&self, folder_id: DirId, parent_id: DirId) -> impl Future<Output = Result<String, String>> + 'static {
        let operation = self.library.move_directory_task(folder_id, parent_id);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn move_folder_to_trash(&self, folder_id: DirId) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.move_directory_to_trash(&folder_id));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn restore_folder(&self, folder_id: DirId, parent_id: Option<DirId>) -> impl Future<Output = Result<String, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.restore_directory(&folder_id, parent_id.as_ref()));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn copy_book_to_folder(&self, content_hash: epub_svelte_lib::ContentHash, source_id: DirId, destination_id: DirId) -> impl Future<Output = Result<String, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.copy_book_to_directory(&content_hash, &source_id, &destination_id));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn move_book_to_folder(&self, content_hash: epub_svelte_lib::ContentHash, source_id: DirId, destination_id: DirId) -> impl Future<Output = Result<String, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.move_book_to_directory(&content_hash, &source_id, &destination_id));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn remove_book_from_folder(&self, content_hash: epub_svelte_lib::ContentHash, source_id: DirId) -> impl Future<Output = Result<bool, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.remove_book_from_directory(&content_hash, &source_id));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn move_book_to_trash(&self, content_hash: epub_svelte_lib::ContentHash) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.move_book_to_trash(&content_hash));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn download_book(&self, content_hash: epub_svelte_lib::ContentHash) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn(move |library| async move { library.download_book(content_hash).await });
        async move { operation.await.map_err(|error| error.to_string())?.map(|_| ()).map_err(|error| error.to_string()) }
    }

    pub(crate) fn remove_book_download(&self, content_hash: epub_svelte_lib::ContentHash) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.remove_local_book_copy(content_hash));
        async move { operation.await.map_err(|error| error.to_string())?.map(|_| ()).map_err(|error| error.to_string()) }
    }

    pub(crate) fn load_trash(&self) -> impl Future<Output = Result<TrashBrowseRows, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.trash());
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn restore_book(&self, content_hash: epub_svelte_lib::ContentHash) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.restore_book(&content_hash));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn purge_book(&self, content_hash: epub_svelte_lib::ContentHash) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.purge_book(&content_hash));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn purge_folder(&self, folder_id: DirId) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.purge_directory(&folder_id));
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn empty_trash(&self) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.empty_trash());
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn folder_destinations(&self) -> impl Future<Output = Result<Vec<epub_svelte_lib::library::LibraryFolderDestination>, String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.folder_destinations());
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn download_folder(&self, folder_id: DirId) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn(move |library| async move { library.download_folder(folder_id).await });
        async move { operation.await.map_err(|error| error.to_string())?.map(|_| ()).map_err(|error| error.to_string()) }
    }

    pub(crate) fn remove_folder_downloads(&self, folder_id: DirId) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.spawn_blocking(move |library| library.remove_local_folder_copies(&folder_id));
        async move { operation.await.map_err(|error| error.to_string())?.map(|_| ()).map_err(|error| error.to_string()) }
    }

    pub(crate) fn discover_author_books(&self, author_id: AuthorId) -> BackendTask<Result<CatalogAuthorDiscoveryView, String>> {
        self.library.spawn(move |library| async move { library.discover_author_books(author_id).await.map_err(|error| error.to_string()) })
    }

    pub(crate) fn import_catalog_book(&self, request: CatalogImportRequest) -> Result<BackendTask<Result<(), String>>, String> {
        Ok(self.library.spawn(move |library| async move { library.import_catalog_book(ROOT_DIR_ID, &request).await.map(|_| ()).map_err(|error| error.to_string()) }))
    }
}

#[cfg(all(not(feature = "kobo"), target_os = "macos"))]
fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    Command::new("open").arg("-R").arg(path).spawn().map(|_| ()).map_err(|error| format!("Could not show the book in Finder: {error}"))
}

#[cfg(all(not(feature = "kobo"), target_os = "windows"))]
fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    let selection = format!("/select,{}", path.display());
    Command::new("explorer.exe").arg(selection).spawn().map(|_| ()).map_err(|error| format!("Could not show the book in File Explorer: {error}"))
}

#[cfg(all(not(feature = "kobo"), not(any(target_os = "macos", target_os = "windows"))))]
fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    let directory = path.parent().ok_or_else(|| "The book has no containing folder".to_owned())?;
    Command::new("xdg-open").arg(directory).spawn().map(|_| ()).map_err(|error| format!("Could not open the book's folder: {error}"))
}

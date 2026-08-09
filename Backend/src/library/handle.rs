//! Public handle for operations scoped to one concrete library.

use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Weak};

use crate::app::executor::BackendTask;
use crate::catalog::{CatalogAuthorDiscoveryResult, CatalogImportRequest, CatalogSearch, CatalogSearchQuery, CatalogSourceId};
use crate::library::database as db;
use crate::library::{AuthorSummary, BookCardRow, Dir, ResolvedBook};
use crate::platform::scanner::{LibraryEvent, LibraryScanner};
use crate::sync::SyncManager;
use crate::LibraryRoot;
use crate::{AuthorId, ContentHash, DirId, DownloadState, FolderDownloadStatus, LibraryId};

use crate::app::{AppBackend, AppState, BackendContext, BackendError, CatalogAuthorDiscoveryView, CatalogPageView, CatalogWideSearchView};
use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

/// All operations whose meaning is scoped to one concrete library.
///
/// The handle retains the global backend only for shared policy, networking,
/// and synchronization. It never relies on a globally selected library.
#[derive(Clone)]
pub struct LibraryBackend {
    app: Weak<AppState>,
    pub(super) context: BackendContext,
    pub(super) id: LibraryId,
    pub(super) library_root: LibraryRoot,
    pub(super) db: Pool<SqliteConnectionManager>,
    pub(super) sync_manager: Arc<SyncManager>,
    pub(super) event_tx: Sender<LibraryEvent>,
    // Its Drop implementation stops and joins the scanner threads.
    pub(super) _scanner: Arc<LibraryScanner>,
}

impl LibraryBackend {
    pub(super) fn from_parts(
        app: Weak<AppState>, context: BackendContext, id: LibraryId, library_root: LibraryRoot, db: Pool<SqliteConnectionManager>, sync_manager: Arc<SyncManager>, event_tx: Sender<LibraryEvent>, scanner: LibraryScanner,
    ) -> Self {
        Self { app, context, id, library_root, db, sync_manager, event_tx, _scanner: Arc::new(scanner) }
    }

    pub(super) fn app(&self) -> Result<AppBackend, BackendError> {
        AppBackend::from_weak(&self.app)
    }

    pub fn id(&self) -> &LibraryId {
        &self.id
    }

    pub fn spawn<T, F, Fut>(&self, operation: F) -> BackendTask<T>
    where
        T: Send + 'static,
        F: FnOnce(Self) -> Fut,
        Fut: Future<Output = T> + Send + 'static,
    {
        let library = self.clone();
        self.app().expect("a library operation requires its application").spawn(move |_| operation(library))
    }

    pub fn thumbnail(&self, content_hash: ContentHash) -> Result<Option<Vec<u8>>, BackendError> {
        self.load_thumbnail_bytes(content_hash).map_err(BackendError::operation)
    }

    pub fn load_thumbnail(&self, content_hash: ContentHash) -> BackendTask<Result<Option<Vec<u8>>, BackendError>> {
        let library = self.clone();
        self.app().expect("a library task requires its application").spawn(move |_| async move { library.thumbnail(content_hash) })
    }

    pub fn create_directory(&self, parent_id: &str, name: &str) -> Result<Dir, BackendError> {
        self.create_directory_on_disk(parent_id, name).map_err(BackendError::operation)
    }

    pub fn create_directory_task(&self, parent_id: String, name: String) -> BackendTask<Result<(), BackendError>> {
        let library = self.clone();
        self.app().expect("a library task requires its application").spawn(move |_| async move { library.create_directory(&parent_id, &name).map(|_| ()) })
    }

    pub fn import_book(&self, parent_id: &str, source_path: &Path, file_name: &str) -> Result<ContentHash, BackendError> {
        self.import_local_book(parent_id, source_path, file_name).map_err(BackendError::operation)
    }

    pub fn import_book_task(&self, parent_id: String, source_path: std::path::PathBuf, file_name: String) -> BackendTask<Result<(), BackendError>> {
        let library = self.clone();
        self.app().expect("a library task requires its application").spawn(move |_| async move { library.import_book(&parent_id, &source_path, &file_name).map(|_| ()) })
    }

    pub async fn search_catalogs(&self, query: &str) -> Result<CatalogWideSearchView, BackendError> {
        self.app()?.search_catalogs_in(self.id().as_str(), query).await
    }

    pub async fn catalog_page(&self, source_id: &CatalogSourceId, locator: Option<&str>) -> Result<CatalogPageView, BackendError> {
        self.app()?.catalog_page_in(self.id().as_str(), source_id, locator).await
    }

    pub async fn search_catalog(&self, source_id: &CatalogSourceId, search: &CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPageView, BackendError> {
        self.app()?.search_catalog_in(self.id().as_str(), source_id, search, query).await
    }

    pub async fn discover_author_books(&self, author_id: AuthorId) -> Result<CatalogAuthorDiscoveryView, BackendError> {
        self.app()?.refresh_catalog_author_view_in(self.id().as_str(), &author_id.to_string()).await
    }

    pub async fn discover_author_catalog(&self, author_id: AuthorId) -> Result<CatalogAuthorDiscoveryResult, BackendError> {
        self.app()?.populate_catalog_author_books_in(self.id().as_str(), &author_id.to_string()).await
    }

    pub async fn import_catalog_book(&self, directory_id: &str, request: &CatalogImportRequest) -> Result<ContentHash, BackendError> {
        let destination = crate::library::BookImportDestination::parse(self.id().as_str(), directory_id).map_err(BackendError::operation)?;
        self.app()?.import_catalog_book_in(&destination, request).await
    }

    pub fn resolve_book_for_reader(&self, content_hash: ContentHash) -> Result<ResolvedBook, BackendError> {
        self.resolve_book(content_hash).map_err(BackendError::operation)
    }

    pub fn is_audiobook(&self, content_hash: ContentHash) -> Result<bool, BackendError> {
        Ok(self.book_path(content_hash)?.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("m4b")))
    }

    pub fn read_book_bytes(&self, content_hash: ContentHash) -> Result<Vec<u8>, BackendError> {
        let mut book = self.resolve_book_for_reader(content_hash)?;
        let mut bytes = Vec::new();
        book.reader.read_to_end(&mut bytes).map_err(BackendError::operation)?;
        Ok(bytes)
    }

    pub fn book_file_name(&self, content_hash: ContentHash) -> Result<String, BackendError> {
        let path = self.book_path(content_hash)?;
        path.file_name().and_then(|name| name.to_str()).map(str::to_owned).ok_or_else(|| BackendError::operation("book file name is not valid UTF-8"))
    }

    fn book_path(&self, content_hash: ContentHash) -> Result<std::path::PathBuf, BackendError> {
        let relative = db::get_book_file_path(&*self.db.get().map_err(BackendError::operation)?, content_hash.clone()).map_err(BackendError::operation)?.ok_or_else(|| BackendError::operation(format!("book file is unavailable: {content_hash}")))?;
        Ok(self.library_root.resolve_book(&relative))
    }

    pub async fn download_book(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        self.app()?.download_book_in(self.id().as_str(), content_hash).await
    }

    pub(crate) fn download_state(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let downloaded = db::is_book_downloaded(&conn, content_hash.clone()).map_err(BackendError::operation)?;
        let requested = db::is_book_download_requested(&conn, content_hash).map_err(BackendError::operation)?;
        if downloaded {
            Ok(DownloadState::Downloaded)
        } else if requested {
            Ok(DownloadState::Queued)
        } else {
            Ok(DownloadState::NotDownloaded)
        }
    }

    pub(crate) fn folder_download_status(&self, directory_id: &DirId) -> Result<FolderDownloadStatus, BackendError> {
        let (downloaded, total) = db::get_folder_download_counts(&*self.db.get().map_err(BackendError::operation)?, directory_id.as_str()).map_err(BackendError::operation)?;
        Ok(FolderDownloadStatus::new(directory_id.as_str(), downloaded, total))
    }

    pub fn remove_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        self.evict_local_book_copy(content_hash).map_err(BackendError::operation)
    }

    pub async fn download_folder(&self, directory_id: DirId) -> Result<FolderDownloadStatus, BackendError> {
        self.app()?.download_folder_in(self.id().as_str(), directory_id).await
    }
}

#[derive(Clone, serde::Serialize, specta::Type)]
pub struct LibraryHomeView {
    pub recently_read: Vec<BookCardRow>,
    pub recently_added: Vec<BookCardRow>,
}

#[derive(Clone, serde::Serialize, specta::Type)]
pub struct LibraryAuthorsView {
    pub authors: Vec<AuthorSummary>,
    pub books: Option<Vec<BookCardRow>>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LibraryFolderEntry {
    pub id: String,
    pub name: String,
    #[specta(type = specta_typescript::Number)]
    pub book_count: usize,
    #[specta(type = specta_typescript::Number)]
    pub downloaded_book_count: usize,
}

#[derive(Clone, serde::Serialize, specta::Type)]
pub struct LibraryFolderView {
    pub directories: Vec<LibraryFolderEntry>,
    pub books: Vec<BookCardRow>,
}

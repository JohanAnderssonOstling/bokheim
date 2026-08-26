//! Public handle for operations scoped to one concrete library.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use crate::app::executor::BackendTask;
use crate::catalog::{CatalogImportRequest, CatalogPublicationIdentity, CatalogSearch, CatalogSearchQuery, CatalogSourceId, CatalogSubjectBrowseRequest};
use crate::library::database as db;
use crate::library::ResolvedBook;
use crate::platform::scanner::{LibraryEvent, LibraryScanner};
use crate::sync::SyncManager;
use crate::{ContentHash, DirId, DownloadState, FolderDownloadStatus, LibraryId};
use app_contract::catalog::CatalogSubjectBrowseView;
use app_contract::library::{BookImportDestination, Dir, DirectoryImport, LibraryFolderDestination, LibraryTrashView};

use crate::app::{AppBackend, AppState, BackendContext, BackendError, CatalogPageView};
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
    pub(super) library_root: PathBuf,
    pub(super) db: Pool<SqliteConnectionManager>,
    pub(super) sync_manager: Arc<SyncManager>,
    pub(super) event_tx: Sender<LibraryEvent>,
    import_activity: Arc<AtomicUsize>,
    // Its Drop implementation stops and joins the scanner threads.
    pub(super) _scanner: Arc<LibraryScanner>,
}

struct ImportActivity {
    active: Arc<AtomicUsize>,
    events: Sender<LibraryEvent>,
}

impl Drop for ImportActivity {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
        let _ = self.events.send(LibraryEvent::LibraryChanged);
    }
}

impl LibraryBackend {
    fn notify_library_changed<T>(&self, result: Result<T, BackendError>) -> Result<T, BackendError> {
        if result.is_ok() {
            let _ = self.event_tx.send(LibraryEvent::LibraryChanged);
        }
        result
    }

    pub(super) fn from_parts(
        app: Weak<AppState>, context: BackendContext, id: LibraryId, library_root: PathBuf, db: Pool<SqliteConnectionManager>, sync_manager: Arc<SyncManager>, event_tx: Sender<LibraryEvent>, scanner: LibraryScanner,
    ) -> Self {
        Self { app, context, id, library_root, db, sync_manager, event_tx, import_activity: Arc::new(AtomicUsize::new(0)), _scanner: Arc::new(scanner) }
    }

    pub(super) fn app(&self) -> Result<AppBackend, BackendError> {
        AppBackend::from_weak(&self.app)
    }

    pub fn id(&self) -> &LibraryId {
        &self.id
    }

    /// True while the local filesystem scanner is scheduled or actively
    /// walking this library.
    pub fn scanner_running(&self) -> bool {
        self._scanner.is_scanning() || self.import_activity.load(Ordering::Acquire) != 0
    }

    #[cfg(test)]
    pub(crate) fn shares_scanner_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self._scanner, &other._scanner)
    }

    /// Enables the initial filesystem scan for a handle opened in deferred mode.
    pub fn start_scanner(&self) {
        self._scanner.start_scanning();
    }

    /// Queues a fresh local filesystem scan even when this scanner is already
    /// enabled. Used by an explicit library switch, not ordinary handle access.
    pub fn request_scan(&self) {
        self._scanner.request_scanning();
    }

    pub(crate) fn recorded_taxonomy_codes(&self, content_hash: &ContentHash) -> Result<Vec<taxonomy_contract::TaxonomyCode>, BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        db::recorded_taxonomy_codes(&connection, content_hash).map_err(BackendError::operation)
    }

    pub(crate) fn recorded_taxonomy_codes_for_library(&self) -> Result<Vec<taxonomy_contract::TaxonomyCode>, BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        db::recorded_taxonomy_codes_for_library(&connection).map_err(BackendError::operation)
    }

    pub(crate) fn reload_unified_subject_assignments(&self) -> Result<(), BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        db::reload_unified_subject_assignments(&connection).map_err(BackendError::operation)?;
        let _ = self.event_tx.send(LibraryEvent::LibraryChanged);
        Ok(())
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

    /// Runs synchronous filesystem or SQLite work on Tokio's blocking pool.
    /// UI callers should prefer this over invoking browse methods directly.
    pub fn spawn_blocking<T, F>(&self, operation: F) -> BackendTask<Result<T, BackendError>>
    where
        T: Send + 'static,
        F: FnOnce(Self) -> Result<T, BackendError> + Send + 'static,
    {
        let library = self.clone();
        self.app().expect("a library operation requires its application").spawn(move |_| async move { tokio::task::spawn_blocking(move || operation(library)).await.map_err(BackendError::operation)? })
    }

    pub fn thumbnail(&self, content_hash: ContentHash) -> Result<Option<Vec<u8>>, BackendError> {
        self.load_thumbnail_bytes(content_hash).map_err(BackendError::operation)
    }

    pub fn load_thumbnail(&self, content_hash: ContentHash) -> BackendTask<Result<Option<Vec<u8>>, BackendError>> {
        let library = self.clone();
        self.app().expect("a library task requires its application").spawn(move |_| async move { library.thumbnail(content_hash) })
    }

    pub fn create_directory(&self, parent_id: &DirId, name: &str) -> Result<Dir, BackendError> {
        self.notify_library_changed(self.create_directory_on_disk(parent_id, name).map_err(BackendError::operation))
    }

    pub fn rename_directory(&self, directory_id: &DirId, name: &str) -> Result<String, BackendError> {
        self.notify_library_changed(self.rename_directory_on_disk(directory_id, name).map_err(BackendError::operation))
    }

    pub fn move_directory(&self, directory_id: &DirId, parent_id: &DirId) -> Result<String, BackendError> {
        self.notify_library_changed(self.move_directory_on_disk(directory_id, Some(parent_id), None).map_err(BackendError::operation))
    }

    pub fn move_directory_task(&self, directory_id: DirId, parent_id: DirId) -> BackendTask<Result<String, BackendError>> {
        let library = self.clone();
        self.app().expect("a library task requires its application").spawn(move |_| async move { library.move_directory(&directory_id, &parent_id) })
    }

    pub fn copy_book_to_directory(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, BackendError> {
        self.notify_library_changed(self.copy_book_placement_on_disk(content_hash, source_id, destination_id).map_err(BackendError::operation))
    }

    pub fn move_book_to_directory(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, BackendError> {
        self.notify_library_changed(self.move_book_placement_on_disk(content_hash, source_id, destination_id).map_err(BackendError::operation))
    }

    /// Removes one placement. Returns `true` when it was the final placement
    /// and the book consequently moved to Trash.
    pub fn remove_book_from_directory(&self, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, BackendError> {
        self.notify_library_changed(self.remove_book_placement_on_disk(content_hash, source_id).map_err(BackendError::operation))
    }

    pub fn move_book_to_trash(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.notify_library_changed(self.trash_book_on_disk(content_hash).map_err(BackendError::operation))
    }

    pub fn restore_book(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.notify_library_changed(self.restore_book_from_trash(content_hash).map_err(BackendError::operation))
    }

    pub fn purge_book(&self, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.notify_library_changed(self.purge_book_from_trash(content_hash).map_err(BackendError::operation))
    }

    pub fn move_directory_to_trash(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.notify_library_changed(self.trash_directory_on_disk(directory_id).map_err(BackendError::operation))
    }

    pub fn restore_directory(&self, directory_id: &DirId, parent_id: Option<&DirId>) -> Result<String, BackendError> {
        self.notify_library_changed(self.restore_directory_from_trash(directory_id, parent_id).map_err(BackendError::operation))
    }

    pub fn purge_directory(&self, directory_id: &DirId) -> Result<(), BackendError> {
        self.notify_library_changed(self.purge_directory_from_trash(directory_id).map_err(BackendError::operation))
    }

    pub fn trash(&self) -> Result<LibraryTrashView, BackendError> {
        self.load_trash().map_err(BackendError::operation)
    }

    pub fn empty_trash(&self) -> Result<(), BackendError> {
        self.notify_library_changed(self.empty_trash_on_disk().map_err(BackendError::operation))
    }

    pub fn folder_destinations(&self) -> Result<Vec<LibraryFolderDestination>, BackendError> {
        self.load_folder_destinations().map_err(BackendError::operation)
    }

    pub fn import_book(&self, parent_id: &DirId, source_path: &Path) -> Result<ContentHash, BackendError> {
        self.import_local_book(parent_id, source_path).map_err(BackendError::operation)
    }

    pub fn import_book_bytes(&self, parent_id: &DirId, file_name: String, bytes: Vec<u8>) -> Result<ContentHash, BackendError> {
        self.import_book_reader(parent_id, &mut std::io::Cursor::new(bytes), file_name, None).map_err(BackendError::operation)
    }

    pub fn import_folder(&self, parent_id: &DirId, source_path: &Path) -> Result<Vec<String>, BackendError> {
        self.notify_library_changed(self.import_local_folder(parent_id, source_path).map_err(BackendError::operation))
    }

    pub fn import_directory(&self, parent_id: &DirId, source: DirectoryImport) -> Result<Vec<String>, BackendError> {
        self.notify_library_changed(self.import_selected_directory(parent_id, source, true).map_err(BackendError::operation))
    }

    pub(crate) fn import_directory_contents(&self, source: DirectoryImport) -> Result<Vec<String>, BackendError> {
        self.import_activity.fetch_add(1, Ordering::AcqRel);
        let _activity = ImportActivity { active: self.import_activity.clone(), events: self.event_tx.clone() };
        let _ = self.event_tx.send(LibraryEvent::LibraryChanged);
        self.import_selected_directory(&sync_common::ROOT_DIR_ID, source, false).map_err(BackendError::operation)
    }

    pub(crate) fn import_downloaded_catalog_book(&self, parent_id: &DirId, source_path: &Path, file_name: &str, source_id: &CatalogSourceId, publication: &CatalogPublicationIdentity) -> Result<ContentHash, BackendError> {
        self.import_catalog_book_file(parent_id, source_path, file_name, source_id, publication).map_err(BackendError::operation)
    }

    pub(crate) fn owned_catalog_publications(&self, source_id: &CatalogSourceId, identities: &[CatalogPublicationIdentity]) -> Result<Vec<CatalogPublicationIdentity>, BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        db::owned_catalog_publications(&connection, source_id, identities).map_err(BackendError::operation)
    }

    pub async fn catalog_page(&self, source_id: &CatalogSourceId, locator: Option<&str>) -> Result<CatalogPageView, BackendError> {
        self.app()?.catalog_page_in(self.id(), source_id, locator).await
    }

    pub async fn refresh_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseView, BackendError> {
        self.app()?.refresh_catalog_subjects_in(self.id(), request).await
    }

    pub async fn catalog_subject_capabilities(&self, request: CatalogSubjectBrowseRequest) -> Result<app_contract::catalog::CatalogSubjectBrowseCapabilities, BackendError> {
        self.app()?.catalog_subject_capabilities(request).await
    }

    pub fn cached_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> Result<Option<CatalogSubjectBrowseView>, BackendError> {
        self.app()?.cached_catalog_subjects_in(self.id(), request)
    }

    pub async fn search_catalog(&self, source_id: &CatalogSourceId, search: &CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPageView, BackendError> {
        self.app()?.search_catalog_in(self.id(), source_id, search, query).await
    }

    pub async fn cached_search_catalog(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogPageView, BackendError> {
        self.app()?.cached_search_catalog_in(self.id(), source_id, query).await
    }

    pub async fn import_catalog_book(&self, directory_id: DirId, request: &CatalogImportRequest) -> Result<ContentHash, BackendError> {
        let destination = BookImportDestination::new(*self.id(), directory_id);
        self.app()?.import_catalog_book_in(&destination, request).await
    }

    pub fn resolve_book_for_reader(&self, content_hash: ContentHash) -> Result<ResolvedBook, BackendError> {
        self.resolve_book(content_hash).map_err(BackendError::operation)
    }

    pub fn is_audiobook(&self, content_hash: ContentHash) -> Result<bool, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        Ok(matches!(db::get_book_format(&conn, &content_hash).map_err(BackendError::operation)?, Some(library_model::BookFormat::M4b)))
    }

    pub fn is_epub(&self, content_hash: ContentHash) -> Result<bool, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        Ok(matches!(db::get_book_format(&conn, &content_hash).map_err(BackendError::operation)?, Some(library_model::BookFormat::Epub)))
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

    pub fn book_asset_path(&self, content_hash: ContentHash) -> Result<String, BackendError> {
        let path = self.book_path(content_hash)?;
        path.to_str().map(str::to_owned).ok_or_else(|| BackendError::operation("book file path is not valid UTF-8"))
    }

    fn book_path(&self, content_hash: ContentHash) -> Result<std::path::PathBuf, BackendError> {
        let relative =
            db::get_book_file_path(&*self.db.get().map_err(BackendError::operation)?, content_hash.clone()).map_err(BackendError::operation)?.ok_or_else(|| BackendError::operation(format!("book file is unavailable: {content_hash}")))?;
        Ok(relative.resolve_under(&self.library_root))
    }

    pub async fn download_book(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        self.app()?.download_book_in(self.id(), content_hash).await
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
        let (downloaded, total) = db::get_folder_download_counts(&*self.db.get().map_err(BackendError::operation)?, &directory_id.to_string()).map_err(BackendError::operation)?;
        Ok(FolderDownloadStatus::new(*directory_id, downloaded, total))
    }

    pub fn remove_local_book_copy(&self, content_hash: ContentHash) -> Result<DownloadState, BackendError> {
        self.evict_local_book_copy(content_hash).map_err(BackendError::operation)
    }

    pub fn remove_local_folder_copies(&self, directory_id: &DirId) -> Result<FolderDownloadStatus, BackendError> {
        self.evict_local_folder_copies(directory_id).map_err(BackendError::operation)?;
        self.folder_download_status(directory_id)
    }

    pub async fn download_folder(&self, directory_id: DirId) -> Result<FolderDownloadStatus, BackendError> {
        self.app()?.download_folder_in(self.id(), directory_id).await
    }
}

pub use library_sqlite::{LibraryAuthorsView, LibraryFolderEntry, LibraryFolderView, LibraryHierarchyEntry, LibraryHierarchyView, LibrarySubjectEntry, LibrarySubjectView};

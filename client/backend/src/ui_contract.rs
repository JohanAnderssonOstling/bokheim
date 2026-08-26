//! Native implementation of the frontend service contract.

use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;

use app_contract::account::AccountStatus;
use app_contract::backend::{AppBackend as AppBackendContract, BackendFuture, BrowserSession, CloudSnapshot, LibraryBackend as LibraryBackendContract};
use app_contract::catalog::{CatalogImportRequest, CatalogSourceId, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseRequest, CatalogSubjectBrowseView};
use app_contract::library::{BrowseContents, Dir, DirectoryImport, EditionIdentityReview, LibraryBrowseQuery, LibraryFolderDestination, LibraryHomeView, LibrarySummary, LibraryTrashView, LibraryUpdate, ReaderContent, SubjectPath};
use app_contract::preferences::{BrowsingPreferencePatch, BrowsingPreferences};
use app_contract::reader_state::{ReaderAnnotation, ReaderBookmark};
use app_contract::NavigationCrumb;
use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};

use crate::app::{AppBackend, LibraryEntry};
use crate::library::LibraryBackend;

impl LibraryBackendContract for LibraryBackend {
    fn id(&self) -> LibraryId {
        *LibraryBackend::id(self)
    }

    fn scanner_running(&self) -> bool {
        LibraryBackend::scanner_running(self)
    }

    fn folder_contents(&self, query: LibraryBrowseQuery) -> BackendFuture<BrowseContents> {
        let operation = self.spawn_blocking(move |library| {
            let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
            library.folder_contents_filtered_sorted(&query.location, &query.search, &query.file_types, query.chip_sort, query.book_sort, &languages)
        });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn subject_contents(&self, query: LibraryBrowseQuery) -> BackendFuture<BrowseContents> {
        let operation = self.spawn_blocking(move |library| {
            let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
            library.subject_contents_filtered_sorted(&query.location, &query.search, &query.file_types, query.chip_sort, query.book_sort, &languages)
        });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn directory_path(&self, directory_id: &DirId) -> Result<Vec<Dir>, String> {
        LibraryBackend::directory_path(self, directory_id).map_err(|error| error.to_string())
    }

    fn import_book(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> BackendFuture<ContentHash> {
        let operation = self.spawn_blocking(move |library| library.import_book_bytes(&parent_id, file_name, bytes));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn import_folder(&self, parent_id: DirId, source_path: PathBuf) -> BackendFuture<Vec<String>> {
        let operation = self.spawn_blocking(move |library| library.import_folder(&parent_id, &source_path));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn import_directory(&self, parent_id: DirId, source: DirectoryImport) -> BackendFuture<Vec<String>> {
        let operation = self.spawn_blocking(move |library| library.import_directory(&parent_id, source));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn create_directory(&self, parent_id: DirId, name: String) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.create_directory(&parent_id, &name).map(|_| ()));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn folder_destinations(&self) -> BackendFuture<Vec<LibraryFolderDestination>> {
        let operation = self.spawn_blocking(move |library| library.folder_destinations());
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn rename_directory(&self, directory_id: DirId, name: String) -> BackendFuture<String> {
        let operation = self.spawn_blocking(move |library| library.rename_directory(&directory_id, &name));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn move_directory(&self, directory_id: DirId, parent_id: DirId) -> BackendFuture<String> {
        let operation = self.spawn_blocking(move |library| library.move_directory(&directory_id, &parent_id));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn copy_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> BackendFuture<String> {
        let operation = self.spawn_blocking(move |library| library.copy_book_to_directory(&content_hash, &source_id, &destination_id));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn move_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> BackendFuture<String> {
        let operation = self.spawn_blocking(move |library| library.move_book_to_directory(&content_hash, &source_id, &destination_id));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn remove_book_from_directory(&self, content_hash: ContentHash, source_id: DirId) -> BackendFuture<bool> {
        let operation = self.spawn_blocking(move |library| library.remove_book_from_directory(&content_hash, &source_id));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn load_cover(&self, content_hash: ContentHash) -> BackendFuture<Option<Vec<u8>>> {
        let operation = self.load_thumbnail(content_hash);
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn load_home(&self, limit: i32) -> BackendFuture<LibraryHomeView> {
        let operation = self.spawn_blocking(move |library| library.home(limit));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn load_prepared_edition_identity_reviews(&self) -> BackendFuture<Vec<EditionIdentityReview>> {
        let operation = self.spawn(move |library| async move { library.prepared_edition_identity_reviews().await.map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn load_unmapped_subject_texts(&self) -> BackendFuture<Vec<String>> {
        let operation = self.spawn_blocking(move |library| library.unmapped_subject_texts());
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn select_edition_identity_candidate(&self, content_hash: ContentHash, edition_id: &str, isbn13: &str) -> Result<bool, String> {
        LibraryBackend::select_edition_identity_candidate(self, content_hash, edition_id, isbn13).map_err(|error| error.to_string())
    }

    fn apply_manual_edition_identity(&self, content_hash: ContentHash, isbn: String) -> BackendFuture<bool> {
        let operation = self.spawn_blocking(move |library| library.apply_manual_edition_identity(content_hash, &isbn));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn reject_edition_identity_candidates(&self, content_hash: ContentHash) -> Result<(), String> {
        LibraryBackend::reject_edition_identity_candidates(self, content_hash).map_err(|error| error.to_string())
    }

    fn retry_failed_edition_identities(&self) -> BackendFuture<usize> {
        let operation = self.spawn(move |library| async move { library.retry_failed_edition_identities().await.map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn trash_book(&self, content_hash: ContentHash) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.move_book_to_trash(&content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn trash_folder(&self, folder_id: DirId) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.move_directory_to_trash(&folder_id));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn restore_folder(&self, folder_id: DirId, parent_id: Option<DirId>) -> BackendFuture<String> {
        let operation = self.spawn_blocking(move |library| library.restore_directory(&folder_id, parent_id.as_ref()));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn load_trash(&self) -> BackendFuture<LibraryTrashView> {
        let operation = self.spawn_blocking(move |library| library.trash());
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn restore_book(&self, content_hash: ContentHash) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.restore_book(&content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn purge_book(&self, content_hash: ContentHash) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.purge_book(&content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn purge_folder(&self, folder_id: DirId) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.purge_directory(&folder_id));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn empty_trash(&self) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.empty_trash());
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn cached_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> Result<Option<CatalogSubjectBrowseView>, String> {
        LibraryBackend::cached_catalog_subjects(self, request).map_err(|error| error.to_string())
    }

    fn refresh_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> BackendFuture<CatalogSubjectBrowseView> {
        let operation = self.spawn(move |library| async move { library.refresh_catalog_subjects(request).await.map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn catalog_subject_capabilities(&self, request: CatalogSubjectBrowseRequest) -> BackendFuture<CatalogSubjectBrowseCapabilities> {
        let operation = self.spawn(move |library| async move { library.catalog_subject_capabilities(request).await.map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn import_catalog_book(&self, request: CatalogImportRequest) -> BackendFuture<()> {
        let operation = self.spawn(move |library| async move { library.import_catalog_book(ROOT_DIR_ID, &request).await.map(|_| ()).map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn open_reader_content(&self, content_hash: ContentHash) -> BackendFuture<ReaderContent> {
        let operation = self.spawn_blocking(move |library| library.resolve_book_for_reader(content_hash).map(|book| ReaderContent::new(book.format, book.reader)));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn reading_position(&self, content_hash: ContentHash) -> BackendFuture<Option<String>> {
        let operation = self.spawn_blocking(move |library| library.reading_position(content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn pdf_reading_position(&self, content_hash: ContentHash) -> BackendFuture<Option<(u32, f32)>> {
        let operation = self.spawn_blocking(move |library| library.pdf_reading_position(content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn update_reading_position(&self, content_hash: ContentHash, position: String, progress: Option<f32>) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.update_reading_position(content_hash, &position, progress));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn annotations(&self, content_hash: ContentHash) -> BackendFuture<Vec<ReaderAnnotation>> {
        let operation = self.spawn_blocking(move |library| library.annotations(content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn upsert_annotation(&self, annotation: ReaderAnnotation) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.upsert_annotation(&annotation));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn delete_annotation(&self, annotation_id: String, modified_at: i64) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.delete_annotation(&annotation_id, modified_at));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn bookmarks(&self, content_hash: ContentHash) -> BackendFuture<Vec<ReaderBookmark>> {
        let operation = self.spawn_blocking(move |library| library.bookmarks(content_hash));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn create_bookmark(&self, bookmark: ReaderBookmark) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.create_bookmark(&bookmark));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }

    fn delete_bookmark(&self, bookmark_id: String, deleted_at: i64) -> BackendFuture<()> {
        let operation = self.spawn_blocking(move |library| library.delete_bookmark(&bookmark_id, deleted_at));
        Box::pin(async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
    }
}

impl AppBackendContract for AppBackend {
    fn browser_session(&self) -> Result<BrowserSession, String> {
        AppBackend::browser_session(self).map_err(|error| error.to_string())
    }

    fn save_active_library_id(&self, library_id: Option<LibraryId>) -> Result<(), String> {
        AppBackend::save_active_library_id(self, library_id).map_err(|error| error.to_string())
    }

    fn folder_navigation_path(&self, library_id: LibraryId) -> Vec<NavigationCrumb<DirId>> {
        AppBackend::browser_session(self).map(|session| session.place(&library_id).folder_path).unwrap_or_else(|error| {
            log::warn!("failed to restore folder path: {error}");
            Vec::new()
        })
    }

    fn save_folder_navigation_path(&self, library_id: LibraryId, path: Vec<NavigationCrumb<DirId>>) -> Result<(), String> {
        AppBackend::save_library_folder_path(self, library_id, path).map_err(|error| error.to_string())
    }

    fn subject_navigation_path(&self, library_id: LibraryId) -> Vec<NavigationCrumb<SubjectPath>> {
        AppBackend::browser_session(self).map(|session| session.place(&library_id).subject_path).unwrap_or_else(|error| {
            log::warn!("failed to restore subject path: {error}");
            Vec::new()
        })
    }

    fn save_subject_navigation_path(&self, library_id: LibraryId, path: Vec<NavigationCrumb<SubjectPath>>) -> Result<(), String> {
        AppBackend::save_library_subject_path(self, library_id, path).map_err(|error| error.to_string())
    }

    fn retain_browser_session_libraries(&self, library_ids: &[LibraryId]) -> Result<(), String> {
        AppBackend::retain_browser_session_libraries(self, library_ids).map_err(|error| error.to_string())
    }

    fn libraries(&self) -> Result<Vec<LibrarySummary>, String> {
        AppBackend::libraries(self).map(library_summaries).map_err(|error| error.to_string())
    }

    fn ensure_default_library(&self) -> Result<LibrarySummary, String> {
        AppBackend::ensure_default_library(self).map(|entry| library_summary(&entry)).map_err(|error| error.to_string())
    }

    fn library(&self, library_id: &LibraryId) -> Result<Rc<dyn LibraryBackendContract>, String> {
        AppBackend::library(self, library_id).map(|library| Rc::new(library) as Rc<dyn LibraryBackendContract>).map_err(|error| error.to_string())
    }

    fn create_libraries_from_paths(&self, paths: Vec<PathBuf>) -> BackendFuture<(Vec<LibraryId>, Vec<String>)> {
        let operation = self.spawn(move |core| async move { AppBackend::create_libraries_from_paths(&core, paths).map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn create_library(&self, name: String) -> BackendFuture<LibraryId> {
        let operation = self.spawn(move |core| async move { AppBackend::create_library(&core, name, None).map(|library| *library.library_id()).map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn import_library_directory(&self, library_id: LibraryId, directory: DirectoryImport) -> BackendFuture<Vec<String>> {
        let operation =
            self.spawn(move |core| async move { tokio::task::spawn_blocking(move || AppBackend::import_library_directory(&core, &library_id, directory).map_err(|error| error.to_string())).await.map_err(|error| error.to_string())? });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn delete_library(&self, library_id: LibraryId) -> BackendFuture<()> {
        let operation = self.spawn(move |core| async move { AppBackend::delete_library(&core, &library_id).await.map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn reconcile_libraries(&self) -> BackendFuture<Vec<LibrarySummary>> {
        let operation = self.spawn(|core| async move { AppBackend::reconcile_libraries(&core).await.map(|state| library_summaries(state.libraries)).map_err(|error| error.to_string()) });
        Box::pin(async move { operation.await.map_err(|error| error.to_string())? })
    }

    fn enable_library_scanning(&self) -> Result<(), String> {
        AppBackend::enable_library_scanning(self).map_err(|error| error.to_string())
    }

    fn request_library_scan(&self, library_id: &LibraryId) -> Result<(), String> {
        AppBackend::request_library_scan(self, library_id).map_err(|error| error.to_string())
    }

    fn library_list_changes(&self) -> async_channel::Receiver<()> {
        AppBackend::library_list_changes(self)
    }

    fn library_updates(&self, library_id: LibraryId) -> async_channel::Receiver<LibraryUpdate> {
        AppBackend::library_updates(self, library_id)
    }

    fn browsing_preferences(&self) -> Result<BrowsingPreferences, String> {
        AppBackend::browsing_preferences(self).map_err(|error| error.to_string())
    }

    fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, String> {
        AppBackend::update_browsing_preference(self, patch).map_err(|error| error.to_string())
    }

    fn account_status(&self) -> Result<AccountStatus, String> {
        AppBackend::account_status(self).map_err(|error| error.to_string())
    }

    fn sync_changes(&self) -> async_channel::Receiver<()> {
        AppBackend::sync_changes(self)
    }

    fn transfer_changes(&self) -> async_channel::Receiver<()> {
        AppBackend::transfer_changes(self)
    }

    fn cloud_snapshot(&self) -> BackendFuture<CloudSnapshot> {
        load_cloud_snapshot(self.clone())
    }

    fn sync_all_libraries(&self) -> BackendFuture<CloudSnapshot> {
        let core = self.clone();
        Box::pin(async move {
            let operation = core.spawn(move |core| async move {
                AppBackend::sync_all_libraries(&core).await.map_err(|error| error.to_string())?;
                load_native_cloud_snapshot(&core).await
            });
            operation.await.map_err(|error| error.to_string())?
        })
    }

    fn login(&self, email: String, password: String) -> BackendFuture<CloudSnapshot> {
        let core = self.clone();
        Box::pin(async move {
            let operation = core.spawn(move |core| async move {
                AppBackend::login(&core, &email, &password).await.map_err(|error| error.to_string())?;
                load_native_cloud_snapshot(&core).await
            });
            operation.await.map_err(|error| error.to_string())?
        })
    }

    fn logout(&self) -> BackendFuture<CloudSnapshot> {
        let core = self.clone();
        Box::pin(async move {
            let operation = core.spawn(move |core| async move {
                AppBackend::logout(&core).await.map_err(|error| error.to_string())?;
                load_native_cloud_snapshot(&core).await
            });
            operation.await.map_err(|error| error.to_string())?
        })
    }

    fn request_registration(&self, email: String, password: String) -> BackendFuture<()> {
        native_app_operation(self, move |core| async move { core.request_public_registration(&email, &password).await.map_err(|error| error.to_string()) })
    }

    fn resend_verification(&self, email: String) -> BackendFuture<()> {
        native_app_operation(self, move |core| async move { core.resend_verification(&email).await.map_err(|error| error.to_string()) })
    }

    fn verify_email(&self, email: String, pin: String) -> BackendFuture<CloudSnapshot> {
        let core = self.clone();
        Box::pin(async move {
            let operation = core.spawn(move |core| async move {
                AppBackend::verify_email(&core, &email, &pin).await.map_err(|error| error.to_string())?;
                load_native_cloud_snapshot(&core).await
            });
            operation.await.map_err(|error| error.to_string())?
        })
    }

    fn request_password_reset(&self, email: String) -> BackendFuture<()> {
        native_app_operation(self, move |core| async move { core.request_password_reset(&email).await.map_err(|error| error.to_string()) })
    }

    fn reset_password(&self, token: String, password: String) -> BackendFuture<()> {
        native_app_operation(self, move |core| async move { core.reset_password(&token, &password).await.map_err(|error| error.to_string()) })
    }

    fn catalog_cover(&self, source_id: CatalogSourceId, image_url: String) -> BackendFuture<Vec<u8>> {
        native_app_operation(self, move |core| async move { core.catalog_cover(&source_id, &image_url).await.map_err(|error| error.to_string()) })
    }
}

fn native_app_operation<T, F, Fut>(backend: &AppBackend, operation: F) -> BackendFuture<T>
where
    T: Send + 'static,
    F: FnOnce(AppBackend) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
{
    let task = backend.spawn(operation);
    Box::pin(async move { task.await.map_err(|error| error.to_string())? })
}

fn load_cloud_snapshot(backend: AppBackend) -> BackendFuture<CloudSnapshot> {
    native_app_operation(&backend, |core| async move { load_native_cloud_snapshot(&core).await })
}

async fn load_native_cloud_snapshot(core: &AppBackend) -> Result<CloudSnapshot, String> {
    let library_list = core.reconcile_libraries().await.map_err(|error| error.to_string())?;
    let account = core.account_status().map_err(|error| error.to_string())?;
    let storage_usage = if account.signed_in() { core.account_storage_usage_if_supported().await.map_err(|error| error.to_string())? } else { None };
    let local_storage_usage = core.scan_local_library_storage_usage().await.map_err(|error| error.to_string())?;
    let server_storage_usage = if account.signed_in() { core.account_library_storage_usage_if_supported().await.map_err(|error| error.to_string())? } else { None };
    let transfers = core.transfer_statuses().map_err(|error| error.to_string())?;
    Ok(CloudSnapshot { account, storage_usage, sync_statuses: library_list.sync_statuses, local_storage_usage, server_storage_usage, transfers })
}

fn library_summary(entry: &LibraryEntry) -> LibrarySummary {
    LibrarySummary::new(*entry.library_id(), entry.library_name())
}

fn library_summaries(entries: Vec<LibraryEntry>) -> Vec<LibrarySummary> {
    entries.iter().map(library_summary).collect()
}

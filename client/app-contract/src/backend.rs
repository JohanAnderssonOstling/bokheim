//! Typed application services implemented directly by each platform backend.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;

use sync_common::{ContentHash, DirId, LibraryId};

use crate::account::{AccountStatus, AccountStorageUsage, LibrarySyncStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
use crate::catalog::{CatalogImportRequest, CatalogSourceId, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseRequest, CatalogSubjectBrowseView};
use crate::library::{BrowseContents, Dir, DirectoryImport, EditionIdentityReview, LibraryBrowseQuery, LibraryFolderDestination, LibraryHomeView, LibrarySummary, LibraryTrashView, LibraryUpdate, ReaderContent, SubjectPath};
use crate::preferences::{BrowsingPreferencePatch, BrowsingPreferences};
use crate::reader_state::{ReaderAnnotation, ReaderBookmark};
use crate::transfer::LibraryTransfers;
use crate::{BrowserSession as GenericBrowserSession, NavigationCrumb};

pub type BackendFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + 'static>>;
pub type BrowserSession = GenericBrowserSession<LibraryId, NavigationCrumb<DirId>, NavigationCrumb<SubjectPath>>;

#[derive(Clone, Debug)]
pub struct CloudSnapshot {
    pub account: AccountStatus,
    pub storage_usage: Option<AccountStorageUsage>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
    pub local_storage_usage: Vec<LocalLibraryStorageUsage>,
    pub server_storage_usage: Option<Vec<ServerLibraryStorageUsage>>,
    pub transfers: Vec<LibraryTransfers>,
}

/// Operations scoped to one selected library.
pub trait LibraryBackend {
    fn id(&self) -> LibraryId;
    fn scanner_running(&self) -> bool;
    fn folder_contents(&self, query: LibraryBrowseQuery) -> BackendFuture<BrowseContents>;
    fn subject_contents(&self, query: LibraryBrowseQuery) -> BackendFuture<BrowseContents>;
    fn directory_path(&self, directory_id: &DirId) -> Result<Vec<Dir>, String>;
    fn import_book(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> BackendFuture<ContentHash>;
    fn import_folder(&self, parent_id: DirId, source_path: PathBuf) -> BackendFuture<Vec<String>>;
    fn import_directory(&self, parent_id: DirId, source: DirectoryImport) -> BackendFuture<Vec<String>>;
    fn create_directory(&self, parent_id: DirId, name: String) -> BackendFuture<()>;
    fn folder_destinations(&self) -> BackendFuture<Vec<LibraryFolderDestination>>;
    fn rename_directory(&self, directory_id: DirId, name: String) -> BackendFuture<String>;
    fn move_directory(&self, directory_id: DirId, parent_id: DirId) -> BackendFuture<String>;
    fn copy_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> BackendFuture<String>;
    fn move_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> BackendFuture<String>;
    fn remove_book_from_directory(&self, content_hash: ContentHash, source_id: DirId) -> BackendFuture<bool>;
    fn load_cover(&self, content_hash: ContentHash) -> BackendFuture<Option<Vec<u8>>>;
    fn load_home(&self, limit: i32) -> BackendFuture<LibraryHomeView>;
    fn load_prepared_edition_identity_reviews(&self) -> BackendFuture<Vec<EditionIdentityReview>>;
    fn load_unmapped_subject_texts(&self) -> BackendFuture<Vec<String>>;
    fn select_edition_identity_candidate(&self, content_hash: ContentHash, edition_id: &str, isbn13: &str) -> Result<bool, String>;
    fn apply_manual_edition_identity(&self, content_hash: ContentHash, isbn: String) -> BackendFuture<bool>;
    fn reject_edition_identity_candidates(&self, content_hash: ContentHash) -> Result<(), String>;
    fn retry_failed_edition_identities(&self) -> BackendFuture<usize>;
    fn trash_book(&self, content_hash: ContentHash) -> BackendFuture<()>;
    fn trash_folder(&self, folder_id: DirId) -> BackendFuture<()>;
    fn restore_folder(&self, folder_id: DirId, parent_id: Option<DirId>) -> BackendFuture<String>;
    fn load_trash(&self) -> BackendFuture<LibraryTrashView>;
    fn restore_book(&self, content_hash: ContentHash) -> BackendFuture<()>;
    fn purge_book(&self, content_hash: ContentHash) -> BackendFuture<()>;
    fn purge_folder(&self, folder_id: DirId) -> BackendFuture<()>;
    fn empty_trash(&self) -> BackendFuture<()>;
    fn cached_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> Result<Option<CatalogSubjectBrowseView>, String>;
    fn refresh_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> BackendFuture<CatalogSubjectBrowseView>;
    fn catalog_subject_capabilities(&self, request: CatalogSubjectBrowseRequest) -> BackendFuture<CatalogSubjectBrowseCapabilities>;
    fn import_catalog_book(&self, request: CatalogImportRequest) -> BackendFuture<()>;

    fn open_reader_content(&self, content_hash: ContentHash) -> BackendFuture<ReaderContent>;
    fn reading_position(&self, content_hash: ContentHash) -> BackendFuture<Option<String>>;
    fn pdf_reading_position(&self, content_hash: ContentHash) -> BackendFuture<Option<(u32, f32)>>;
    fn update_reading_position(&self, content_hash: ContentHash, position: String, progress: Option<f32>) -> BackendFuture<()>;
    fn annotations(&self, content_hash: ContentHash) -> BackendFuture<Vec<ReaderAnnotation>>;
    fn upsert_annotation(&self, annotation: ReaderAnnotation) -> BackendFuture<()>;
    fn delete_annotation(&self, annotation_id: String, modified_at: i64) -> BackendFuture<()>;
    fn bookmarks(&self, content_hash: ContentHash) -> BackendFuture<Vec<ReaderBookmark>>;
    fn create_bookmark(&self, bookmark: ReaderBookmark) -> BackendFuture<()>;
    fn delete_bookmark(&self, bookmark_id: String, deleted_at: i64) -> BackendFuture<()>;
}

/// Application-, account-, and library-registry operations.
pub trait AppBackend {
    fn browser_session(&self) -> Result<BrowserSession, String>;
    fn save_active_library_id(&self, library_id: Option<LibraryId>) -> Result<(), String>;
    fn folder_navigation_path(&self, library_id: LibraryId) -> Vec<NavigationCrumb<DirId>>;
    fn save_folder_navigation_path(&self, library_id: LibraryId, path: Vec<NavigationCrumb<DirId>>) -> Result<(), String>;
    fn subject_navigation_path(&self, library_id: LibraryId) -> Vec<NavigationCrumb<SubjectPath>>;
    fn save_subject_navigation_path(&self, library_id: LibraryId, path: Vec<NavigationCrumb<SubjectPath>>) -> Result<(), String>;
    fn retain_browser_session_libraries(&self, library_ids: &[LibraryId]) -> Result<(), String>;

    fn libraries(&self) -> Result<Vec<LibrarySummary>, String>;
    fn ensure_default_library(&self) -> Result<LibrarySummary, String>;
    fn library(&self, library_id: &LibraryId) -> Result<Rc<dyn LibraryBackend>, String>;
    fn create_libraries_from_paths(&self, paths: Vec<PathBuf>) -> BackendFuture<(Vec<LibraryId>, Vec<String>)>;
    fn create_library(&self, name: String) -> BackendFuture<LibraryId>;
    fn import_library_directory(&self, library_id: LibraryId, directory: DirectoryImport) -> BackendFuture<Vec<String>>;
    fn delete_library(&self, library_id: LibraryId) -> BackendFuture<()>;
    fn reconcile_libraries(&self) -> BackendFuture<Vec<LibrarySummary>>;
    fn enable_library_scanning(&self) -> Result<(), String>;
    fn request_library_scan(&self, library_id: &LibraryId) -> Result<(), String>;
    fn library_list_changes(&self) -> async_channel::Receiver<()>;
    fn library_updates(&self, library_id: LibraryId) -> async_channel::Receiver<LibraryUpdate>;

    fn browsing_preferences(&self) -> Result<BrowsingPreferences, String>;
    fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, String>;

    fn account_status(&self) -> Result<AccountStatus, String>;
    fn sync_changes(&self) -> async_channel::Receiver<()>;
    fn transfer_changes(&self) -> async_channel::Receiver<()>;
    fn cloud_snapshot(&self) -> BackendFuture<CloudSnapshot>;
    fn sync_all_libraries(&self) -> BackendFuture<CloudSnapshot>;
    fn login(&self, email: String, password: String) -> BackendFuture<CloudSnapshot>;
    fn logout(&self) -> BackendFuture<CloudSnapshot>;
    fn request_registration(&self, email: String, password: String) -> BackendFuture<()>;
    fn resend_verification(&self, email: String) -> BackendFuture<()>;
    fn verify_email(&self, email: String, pin: String) -> BackendFuture<CloudSnapshot>;
    fn request_password_reset(&self, email: String) -> BackendFuture<()>;
    fn reset_password(&self, token: String, password: String) -> BackendFuture<()>;

    fn catalog_cover(&self, source_id: CatalogSourceId, image_url: String) -> BackendFuture<Vec<u8>>;
}

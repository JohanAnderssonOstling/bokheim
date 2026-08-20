mod commands;
mod events;
mod state;

use epub_svelte_lib::app::preferences::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences};
use epub_svelte_lib::app::{
    AccountSessionSummary, AccountStatus, AccountStorageUsage, AppBackend, CatalogPageView, CatalogSubjectBrowseView, CatalogWideSearchView, LibraryListState, LibrarySyncStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage,
};
use epub_svelte_lib::app::{AppDataLocation, LibraryEntry};
use epub_svelte_lib::catalog::{CatalogAuthorDiscoveryResult, CatalogImportRequest, CatalogRecommendationResponse, CatalogSearch, CatalogSearchQuery, CatalogSource, CatalogSourceId, CatalogSubjectBrowseRequest};
use epub_svelte_lib::library::links::BookLink;
use epub_svelte_lib::library::marks::{ReaderAnnotation, ReaderBookmark};
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::library::{
    AudiobookChapter, AuthorSort, BookDetail, BookImportDestination, Dir, EditionIdentityReview, LibraryAuthorsView, LibraryFileTypeFilter, LibraryFolderView, LibraryHomeView, LibrarySearchResult, LibrarySubjectView, LibraryTrashView,
};
use epub_svelte_lib::{DownloadState, FolderDownloadStatus, LibraryTransfers, NetworkStatus};
use specta::Type;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tauri::ipc::{InvokeResponseBody, IpcResponse};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_log::{Target, TargetKind};

use state::AppState;

#[derive(Type)]
#[specta(type = Vec<u8>)]
struct BinaryBytes(Vec<u8>);

#[derive(serde::Serialize, Type)]
struct RemoteEpubSession {
    id: String,
    #[specta(type = specta_typescript::Number)]
    size: u64,
}

#[derive(Clone, serde::Serialize)]
struct CatalogSearchRefresh {
    library_id: String,
    query: String,
    view: CatalogWideSearchView,
}

#[derive(Clone, serde::Serialize)]
struct CatalogSourceSearchRefresh {
    library_id: String,
    source_id: CatalogSourceId,
    query: String,
    view: CatalogPageView,
}

impl IpcResponse for BinaryBytes {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(self.0.into())
    }
}

async fn run_blocking<T, F>(operation: &'static str, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let started = Instant::now();
    let result = tauri::async_runtime::spawn_blocking(task).await.map_err(|error| error.to_string())?;
    #[cfg(debug_assertions)]
    log::info!(target: "performance", "tauri.command operation={operation} duration_ms={:.2}", started.elapsed().as_secs_f64() * 1000.0);
    result
}

fn library(core: &AppBackend, library_id: &str) -> Result<LibraryBackend, String> {
    let library_id = epub_svelte_lib::LibraryId::parse_str(library_id).map_err(|error| error.to_string())?;
    core.library(&library_id).map_err(|error| error.to_string())
}

fn parse_content_hash(value: &str) -> Result<epub_svelte_lib::ContentHash, String> {
    Ok(epub_svelte_lib::ContentHash::new(&value))
}

#[tauri::command]
#[specta::specta]
fn init_logging() {
    epub_svelte_lib::init_logging();
}

#[tauri::command]
#[specta::specta]
async fn get_libraries(state: State<'_, AppState>) -> Result<Vec<LibraryEntry>, String> {
    let core = state.core.clone();
    run_blocking("get_libraries", move || core.libraries().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_account_status(state: State<'_, AppState>) -> Result<AccountStatus, String> {
    state.core.account_status().map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_account_storage_usage(state: State<'_, AppState>) -> Result<AccountStorageUsage, String> {
    state.core.account_storage_usage().await.map_err(|error| error.to_string())
}

/// `None` when the server predates the per-library storage endpoint, which the
/// frontend shows as a quota with no breakdown rather than as an empty account.
#[tauri::command]
#[specta::specta]
async fn get_server_library_storage_usage(state: State<'_, AppState>) -> Result<Option<Vec<ServerLibraryStorageUsage>>, String> {
    state.core.account_library_storage_usage_if_supported().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_account_sessions(state: State<'_, AppState>) -> Result<Vec<AccountSessionSummary>, String> {
    state.core.account_sessions().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn revoke_account_session(state: State<'_, AppState>, session_id: String) -> Result<AccountStatus, String> {
    state.core.revoke_account_session(&session_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn revoke_other_account_sessions(state: State<'_, AppState>) -> Result<u32, String> {
    state.core.revoke_other_account_sessions().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn login(state: State<'_, AppState>, email: String, password: String) -> Result<AccountStatus, String> {
    state.core.login(&email, &password).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn request_public_registration(state: State<'_, AppState>, email: String, password: String) -> Result<(), String> {
    state.core.request_public_registration(&email, &password).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn resend_verification(state: State<'_, AppState>, email: String) -> Result<(), String> {
    state.core.resend_verification(&email).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn verify_email(state: State<'_, AppState>, email: String, pin: String) -> Result<AccountStatus, String> {
    state.core.verify_email(&email, &pin).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn request_password_reset(state: State<'_, AppState>, email: String) -> Result<(), String> {
    state.core.request_password_reset(&email).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn reset_password(state: State<'_, AppState>, token: String, new_password: String) -> Result<(), String> {
    state.core.reset_password(&token, &new_password).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn bootstrap_remote_libraries(state: State<'_, AppState>) -> Result<Vec<LibraryEntry>, String> {
    state.core.bootstrap_remote_libraries().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn sync_all_libraries(state: State<'_, AppState>) -> Result<(), String> {
    state.core.sync_all_libraries().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_library_sync_statuses(state: State<'_, AppState>) -> Result<Vec<LibrarySyncStatus>, String> {
    state.core.library_sync_statuses().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn reconcile_libraries(state: State<'_, AppState>) -> Result<LibraryListState, String> {
    state.core.reconcile_libraries().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_transfer_statuses(state: State<'_, AppState>) -> Result<Vec<LibraryTransfers>, String> {
    let core = state.core.clone();
    run_blocking("get_transfer_statuses", move || core.transfer_statuses().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_local_library_storage_usage(state: State<'_, AppState>) -> Result<Vec<LocalLibraryStorageUsage>, String> {
    let core = state.core.clone();
    run_blocking("get_local_library_storage_usage", move || core.local_library_storage_usage().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
fn get_network_status(state: State<'_, AppState>) -> NetworkStatus {
    state.core.network_status()
}

#[tauri::command]
#[specta::specta]
async fn update_network_status(state: State<'_, AppState>, status: NetworkStatus) -> Result<NetworkStatus, String> {
    state.core.update_network_status(status).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn logout(state: State<'_, AppState>) -> Result<AccountStatus, String> {
    state.core.logout().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_reader_preferences(state: State<'_, AppState>) -> Result<ReaderPreferences, String> {
    let core = state.core.clone();
    run_blocking("get_reader_preferences", move || core.reader_preferences().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_browsing_preferences(state: State<'_, AppState>) -> Result<BrowsingPreferences, String> {
    let core = state.core.clone();
    run_blocking("get_browsing_preferences", move || core.browsing_preferences().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_browsing_preference(state: State<'_, AppState>, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, String> {
    let core = state.core.clone();
    run_blocking("update_browsing_preference", move || core.update_browsing_preference(patch).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_reader_preference(state: State<'_, AppState>, patch: ReaderPreferencePatch) -> Result<ReaderPreferences, String> {
    let core = state.core.clone();
    run_blocking("update_reader_preference", move || core.update_reader_preference(patch).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn create_library(state: State<'_, AppState>, name: String) -> Result<LibraryEntry, String> {
    let core = state.core.clone();
    run_blocking("create_library", move || core.create_library(name, None).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn open_library_folder(state: State<'_, AppState>, path: String) -> Result<LibraryEntry, String> {
    let core = state.core.clone();
    run_blocking("open_library_folder", move || {
        let path = std::path::PathBuf::from(path);
        if !path.is_dir() {
            return Err("The selected library folder does not exist".to_owned());
        }
        let name = path.file_name().and_then(|value| value.to_str()).filter(|value| !value.trim().is_empty()).unwrap_or("Library").to_owned();
        core.create_library(name, Some(path)).map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn remove_library(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    let library_id = epub_svelte_lib::LibraryId::parse_str(&library_id).map_err(|error| error.to_string())?;
    state.core.delete_library(&library_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_library_folder(state: State<'_, AppState>, library_id: String, dir_id: String, file_type: Option<LibraryFileTypeFilter>) -> Result<LibraryFolderView, String> {
    let library = library(&state.core, &library_id)?;
    let dir_id = epub_svelte_lib::DirId::parse_str(&dir_id).map_err(|error| error.to_string())?;
    run_blocking("get_library_folder", move || library.folder_filtered(&dir_id, file_type.unwrap_or_default()).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_library_subjects(state: State<'_, AppState>, library_id: String, subject_path: Option<String>, file_type: Option<LibraryFileTypeFilter>) -> Result<LibrarySubjectView, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_library_subjects", move || library.subjects_filtered(subject_path.as_deref(), file_type.unwrap_or_default()).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_edition_identity_reviews(state: State<'_, AppState>, library_id: String) -> Result<Vec<EditionIdentityReview>, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_edition_identity_reviews", move || library.edition_identity_reviews().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn select_edition_identity_candidate(state: State<'_, AppState>, library_id: String, content_hash: String, edition_id: String, isbn13: String) -> Result<bool, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("select_edition_identity_candidate", move || library.select_edition_identity_candidate(content_hash, &edition_id, &isbn13).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn apply_manual_edition_identity(state: State<'_, AppState>, library_id: String, content_hash: String, isbn: String) -> Result<bool, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("apply_manual_edition_identity", move || library.apply_manual_edition_identity(content_hash, &isbn).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn reject_edition_identity_candidates(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("reject_edition_identity_candidates", move || library.reject_edition_identity_candidates(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn retry_edition_identity(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    library.retry_edition_identity(parse_content_hash(&content_hash)?).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_child_directories(state: State<'_, AppState>, library_id: String, parent_id: String) -> Result<Vec<Dir>, String> {
    let library = library(&state.core, &library_id)?;
    let parent_id = epub_svelte_lib::DirId::parse_str(&parent_id).map_err(|error| error.to_string())?;
    run_blocking("get_child_directories", move || library.child_directories(&parent_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_library_folder_path(state: State<'_, AppState>, library_id: String, dir_id: String) -> Result<Vec<Dir>, String> {
    let library = library(&state.core, &library_id)?;
    let dir_id = epub_svelte_lib::DirId::parse_str(&dir_id).map_err(|error| error.to_string())?;
    run_blocking("get_library_folder_path", move || library.directory_path(&dir_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn create_directory(state: State<'_, AppState>, library_id: String, parent_id: String, name: String) -> Result<Dir, String> {
    let library = library(&state.core, &library_id)?;
    let parent_id = epub_svelte_lib::DirId::parse_str(&parent_id).map_err(|error| error.to_string())?;
    run_blocking("create_directory", move || library.create_directory(&parent_id, &name).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_folder_destinations(state: State<'_, AppState>, library_id: String, source_id: String, source_parent_id: Option<String>) -> Result<Vec<epub_svelte_lib::library::LibraryFolderDestination>, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_folder_destinations", move || {
        let destinations = library.folder_destinations().map_err(|error| error.to_string())?;
        let policy = match source_parent_id {
            Some(source_parent_id) => {
                let source_path = destinations.iter().find(|destination| destination.id.to_string() == source_id).map(|destination| destination.path.clone()).ok_or_else(|| "The folder is no longer available".to_owned())?;
                epub_svelte_lib::FolderDestinationPolicy::MoveFolder { source_parent_id, source_path }
            }
            None => epub_svelte_lib::FolderDestinationPolicy::Book { source_id },
        };
        Ok(destinations.into_iter().filter(|destination| policy.can_select(&destination.id.to_string(), &destination.path)).collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn rename_folder(state: State<'_, AppState>, library_id: String, directory_id: String, name: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let directory_id = epub_svelte_lib::DirId::parse_str(&directory_id).map_err(|error| error.to_string())?;
    run_blocking("rename_folder", move || library.rename_directory(&directory_id, &name).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn move_folder(state: State<'_, AppState>, library_id: String, directory_id: String, destination_id: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let directory_id = epub_svelte_lib::DirId::parse_str(&directory_id).map_err(|error| error.to_string())?;
    let destination_id = epub_svelte_lib::DirId::parse_str(&destination_id).map_err(|error| error.to_string())?;
    run_blocking("move_folder", move || library.move_directory(&directory_id, &destination_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn copy_book_to_folder(state: State<'_, AppState>, library_id: String, content_hash: String, source_id: String, destination_id: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    let source_id = epub_svelte_lib::DirId::parse_str(&source_id).map_err(|error| error.to_string())?;
    let destination_id = epub_svelte_lib::DirId::parse_str(&destination_id).map_err(|error| error.to_string())?;
    run_blocking("copy_book_to_folder", move || library.copy_book_to_directory(&content_hash, &source_id, &destination_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn move_book_to_folder(state: State<'_, AppState>, library_id: String, content_hash: String, source_id: String, destination_id: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    let source_id = epub_svelte_lib::DirId::parse_str(&source_id).map_err(|error| error.to_string())?;
    let destination_id = epub_svelte_lib::DirId::parse_str(&destination_id).map_err(|error| error.to_string())?;
    run_blocking("move_book_to_folder", move || library.move_book_to_directory(&content_hash, &source_id, &destination_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn remove_book_from_folder(state: State<'_, AppState>, library_id: String, content_hash: String, source_id: String) -> Result<bool, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    let source_id = epub_svelte_lib::DirId::parse_str(&source_id).map_err(|error| error.to_string())?;
    run_blocking("remove_book_from_folder", move || library.remove_book_from_directory(&content_hash, &source_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_library_trash(state: State<'_, AppState>, library_id: String) -> Result<LibraryTrashView, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_library_trash", move || library.trash().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn move_book_to_trash(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("move_book_to_trash", move || library.move_book_to_trash(&content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn move_folder_to_trash(state: State<'_, AppState>, library_id: String, directory_id: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let directory_id = epub_svelte_lib::DirId::parse_str(&directory_id).map_err(|error| error.to_string())?;
    run_blocking("move_folder_to_trash", move || library.move_directory_to_trash(&directory_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn restore_trashed_book(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("restore_trashed_book", move || library.restore_book(&content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn purge_trashed_book(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("purge_trashed_book", move || library.purge_book(&content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn restore_trashed_folder(state: State<'_, AppState>, library_id: String, directory_id: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let directory_id = epub_svelte_lib::DirId::parse_str(&directory_id).map_err(|error| error.to_string())?;
    run_blocking("restore_trashed_folder", move || library.restore_directory(&directory_id, None).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn purge_trashed_folder(state: State<'_, AppState>, library_id: String, directory_id: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let directory_id = epub_svelte_lib::DirId::parse_str(&directory_id).map_err(|error| error.to_string())?;
    run_blocking("purge_trashed_folder", move || library.purge_directory(&directory_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn empty_library_trash(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("empty_library_trash", move || library.empty_trash().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn import_book(state: State<'_, AppState>, destination: BookImportDestination, source_path: String, file_name: String) -> Result<(), String> {
    let library = state.core.library(destination.library_id()).map_err(|error| error.to_string())?;
    let directory_id = *destination.directory_id();
    run_blocking("import_book", move || library.import_book(&directory_id, std::path::Path::new(&source_path), &file_name).map(|_| ()).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
fn get_catalog_sources(state: State<'_, AppState>) -> Vec<CatalogSource> {
    state.core.catalog_sources()
}

#[tauri::command]
#[specta::specta]
async fn refresh_catalog_sources(state: State<'_, AppState>) -> Result<Vec<CatalogSource>, String> {
    state.core.refresh_catalog_sources().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
fn get_custom_catalog_sources(state: State<'_, AppState>) -> Result<Vec<CatalogSource>, String> {
    state.core.custom_catalog_sources().map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn add_custom_catalog(state: State<'_, AppState>, title: String, url: String) -> Result<CatalogSource, String> {
    let core = state.core.clone();
    run_blocking("add_custom_catalog", move || core.add_custom_catalog(&title, &url).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn remove_custom_catalog(state: State<'_, AppState>, source_id: CatalogSourceId) -> Result<(), String> {
    let core = state.core.clone();
    run_blocking("remove_custom_catalog", move || core.remove_custom_catalog(&source_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_catalogs(app: AppHandle, state: State<'_, AppState>, library_id: String, query: String) -> Result<CatalogWideSearchView, String> {
    let library = library(&state.core, &library_id)?;
    let refresh_library = library.clone();
    let refresh_library_id = library_id.clone();
    let refresh_query = query.clone();
    tauri::async_runtime::spawn(async move {
        if let Ok(view) = refresh_library.search_catalogs(&refresh_query).await {
            let _ = app.emit("catalog:search-refreshed", CatalogSearchRefresh { library_id: refresh_library_id, query: refresh_query, view });
        }
    });
    library.cached_search_catalogs(&query).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_page(state: State<'_, AppState>, library_id: String, source_id: CatalogSourceId, locator: Option<String>) -> Result<CatalogPageView, String> {
    library(&state.core, &library_id)?.catalog_page(&source_id, locator.as_deref()).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_subjects(state: State<'_, AppState>, library_id: String, locator: Option<String>, source_ids: Vec<CatalogSourceId>, query: String, limit: u32) -> Result<CatalogSubjectBrowseView, String> {
    library(&state.core, &library_id)?.browse_catalog_subjects(CatalogSubjectBrowseRequest { locator, supported_languages: Vec::new(), source_ids, query, limit }).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_subject_recommendations(state: State<'_, AppState>, library_id: String, subject_path: Option<String>, limit: u32) -> Result<CatalogRecommendationResponse, String> {
    let library = library(&state.core, &library_id)?;
    match subject_path {
        Some(path) => library.catalog_subject_recommendations(&path, limit).await,
        None => library.catalog_recommendations(limit).await,
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn search_catalog(app: AppHandle, state: State<'_, AppState>, library_id: String, source_id: CatalogSourceId, search: CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPageView, String> {
    let library = library(&state.core, &library_id)?;
    let refresh_library = library.clone();
    let refresh_library_id = library_id.clone();
    let refresh_source_id = source_id.clone();
    let refresh_query = query.clone();
    let query_text = [Some(query.query.as_str()).filter(|value| !value.is_empty()), query.title.as_deref(), query.author.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let refresh_query_text = query_text.clone();
    tauri::async_runtime::spawn(async move {
        if let Ok(view) = refresh_library.search_catalog(&refresh_source_id, &search, refresh_query).await {
            let _ = app.emit("catalog:source-search-refreshed", CatalogSourceSearchRefresh { library_id: refresh_library_id, source_id: refresh_source_id, query: refresh_query_text, view });
        }
    });
    library.cached_search_catalog(&source_id, &query_text).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_cover(state: State<'_, AppState>, source_id: CatalogSourceId, image_url: String) -> Result<BinaryBytes, String> {
    state.core.catalog_cover(&source_id, &image_url).await.map(BinaryBytes).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn import_catalog_book(state: State<'_, AppState>, destination: BookImportDestination, request: CatalogImportRequest) -> Result<(), String> {
    let library = state.core.library(destination.library_id()).map_err(|error| error.to_string())?;
    library.import_catalog_book(*destination.directory_id(), &request).await.map(|_| ()).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_library_home(state: State<'_, AppState>, library_id: String, limit: i32) -> Result<LibraryHomeView, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_library_home", move || library.home(limit).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_book_detail(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<BookDetail, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_book_detail", move || library.book_detail(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_library(state: State<'_, AppState>, library_id: String, query: String, _limit: u32) -> Result<LibrarySearchResult, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("search_library", move || library.search(&query).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_library_folder(state: State<'_, AppState>, library_id: String, dir_id: String, query: String, _limit: u32, file_type: Option<LibraryFileTypeFilter>) -> Result<LibrarySearchResult, String> {
    let library = library(&state.core, &library_id)?;
    let dir_id = epub_svelte_lib::DirId::parse_str(&dir_id).map_err(|error| error.to_string())?;
    run_blocking("search_library_folder", move || library.search_folder_filtered(&dir_id, &query, file_type.unwrap_or_default()).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_library_subject(state: State<'_, AppState>, library_id: String, subject_path: Option<String>, query: String, _limit: u32, file_type: Option<LibraryFileTypeFilter>) -> Result<LibrarySearchResult, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("search_library_subject", move || library.search_subject_filtered(subject_path.as_deref(), &query, file_type.unwrap_or_default()).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_library_authors(state: State<'_, AppState>, library_id: String, selected_author_id: Option<String>, sort: Option<AuthorSort>) -> Result<LibraryAuthorsView, String> {
    let library = library(&state.core, &library_id)?;
    let selected = selected_author_id.map(|author_id| epub_svelte_lib::AgentId::parse_str(&author_id).map_err(|error| error.to_string())).transpose()?;
    let sort = sort.unwrap_or_default();
    run_blocking("get_library_authors", move || library.authors(selected, sort).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn discover_catalog_author_books(state: State<'_, AppState>, library_id: String, author_id: String) -> Result<CatalogAuthorDiscoveryResult, String> {
    let library = library(&state.core, &library_id)?;
    let author_id = epub_svelte_lib::AgentId::parse_str(&author_id).map_err(|error| error.to_string())?;
    library.discover_author_catalog(author_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_book_thumbnail(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<BinaryBytes, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    let bytes = run_blocking("get_book_thumbnail", move || library.thumbnail(content_hash).map(|bytes| bytes.unwrap_or_default()).map_err(|error| error.to_string())).await?;
    Ok(BinaryBytes(bytes))
}

#[tauri::command]
#[specta::specta]
async fn open_book(app: AppHandle, state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    let path = run_blocking("open_book", move || library.book_asset_path(content_hash).map_err(|error| error.to_string())).await?;
    app.asset_protocol_scope().allow_file(&path).map_err(|error| format!("could not allow reader access to {path}: {error}"))?;
    Ok(path)
}

#[tauri::command]
#[specta::specta]
async fn open_pdf_range_session(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<RemoteEpubSession, String> {
    // PDF.js is routed through this IPC-backed range reader instead of Tauri's
    // asset protocol (which `open_book` uses for EPUB/other formats) because
    // some Android system WebViews fail to serve `asset://`/`http://asset.localhost`
    // byte-range requests to the PDF.js worker, leaving PDFs unable to open.
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    let readers = Arc::clone(&state.range_reader_sessions);
    let id = state.next_range_reader_id.fetch_add(1, Ordering::Relaxed).to_string();
    run_blocking("open_pdf_range_session", move || {
        let mut book = library.resolve_book_for_reader(content_hash).map_err(|error| error.to_string())?;
        let size = book.reader.seek(SeekFrom::End(0)).map_err(|error| error.to_string())?;
        book.reader.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
        readers.lock().map_err(|_| "range reader lock is poisoned".to_owned())?.insert(id.clone(), book);
        Ok(RemoteEpubSession { id, size })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn read_pdf_range(state: State<'_, AppState>, session_id: String, offset: f64, length: u32) -> Result<BinaryBytes, String> {
    read_range_session(&state, "PDF", session_id, offset, length).await
}

#[tauri::command]
#[specta::specta]
fn close_pdf_range_session(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    state.range_reader_sessions.lock().map_err(|_| "range reader lock is poisoned".to_owned())?.remove(&session_id);
    Ok(())
}

async fn read_range_session(state: &State<'_, AppState>, label: &'static str, session_id: String, offset: f64, length: u32) -> Result<BinaryBytes, String> {
    if !offset.is_finite() || offset < 0.0 || offset.fract() != 0.0 || offset > u64::MAX as f64 {
        return Err(format!("{label} range offset is invalid"));
    }
    let offset = offset as u64;
    let readers = Arc::clone(&state.range_reader_sessions);
    run_blocking("read_range_session", move || {
        let mut readers = readers.lock().map_err(|_| "range reader lock is poisoned".to_owned())?;
        let book = readers.get_mut(&session_id).ok_or_else(|| format!("{label} reader session has expired"))?;
        let mut bytes = vec![0; length as usize];
        book.reader.seek(SeekFrom::Start(offset)).map_err(|error| error.to_string())?;
        book.reader.read_exact(&mut bytes).map_err(|error| error.to_string())?;
        Ok(BinaryBytes(bytes))
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn get_book_file_name(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_book_file_name", move || library.book_file_name(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_book_reading_position(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Option<String>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_book_reading_position", move || library.reading_position(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_pdf_reading_position(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Option<(u32, f32)>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_pdf_reading_position", move || library.pdf_reading_position(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_reading_position(state: State<'_, AppState>, library_id: String, content_hash: String, cfi: String, progress: f32) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("update_reading_position", move || library.update_reading_position(content_hash, &cfi, progress).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_pdf_reading_position(state: State<'_, AppState>, library_id: String, content_hash: String, page: u32, full_page_position: f32) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("update_pdf_reading_position", move || library.update_pdf_reading_position(content_hash, page, full_page_position).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_audiobook_reading_position(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Option<f64>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_audiobook_reading_position", move || library.audiobook_reading_position(content_hash).map(|position| position.map(|position| position as f64)).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_audiobook_chapters(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Vec<AudiobookChapter>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_audiobook_chapters", move || library.audiobook_chapters(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_audiobook_reading_position(state: State<'_, AppState>, library_id: String, content_hash: String, position_ms: f64, progress: f32) -> Result<(), String> {
    if !position_ms.is_finite() || position_ms < 0.0 || position_ms.fract() != 0.0 || position_ms > u64::MAX as f64 {
        return Err("audiobook position is invalid".to_owned());
    }
    let position_ms = position_ms as u64;
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("update_audiobook_reading_position", move || library.update_audiobook_reading_position(content_hash, position_ms, progress).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_annotations(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Vec<ReaderAnnotation>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_annotations", move || library.annotations(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn upsert_annotation(state: State<'_, AppState>, library_id: String, annotation: ReaderAnnotation) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("upsert_annotation", move || library.upsert_annotation(&annotation).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn delete_annotation(state: State<'_, AppState>, library_id: String, annotation_id: String, modified_at: f64) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    if !modified_at.is_finite() || modified_at.fract() != 0.0 || modified_at < 0.0 || modified_at > i64::MAX as f64 {
        return Err("modified_at must be a non-negative integer timestamp".to_owned());
    }
    let modified_at = modified_at as i64;
    run_blocking("delete_annotation", move || library.delete_annotation(&annotation_id, modified_at).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_bookmarks(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Vec<ReaderBookmark>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_bookmarks", move || library.bookmarks(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn create_bookmark(state: State<'_, AppState>, library_id: String, bookmark: ReaderBookmark) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("create_bookmark", move || library.create_bookmark(&bookmark).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn delete_bookmark(state: State<'_, AppState>, library_id: String, bookmark_id: String, deleted_at: f64) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    if !deleted_at.is_finite() || deleted_at.fract() != 0.0 || deleted_at < 0.0 || deleted_at > i64::MAX as f64 {
        return Err("deleted_at must be a non-negative integer timestamp".to_owned());
    }
    let deleted_at = deleted_at as i64;
    run_blocking("delete_bookmark", move || library.delete_bookmark(&bookmark_id, deleted_at).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_book_links(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<Vec<BookLink>, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("get_book_links", move || library.book_links(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn upsert_book_link(state: State<'_, AppState>, library_id: String, link: BookLink) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("upsert_book_link", move || library.upsert_book_link(&link).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn delete_book_link(state: State<'_, AppState>, library_id: String, link_id: String, modified_at: f64) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    if !modified_at.is_finite() || modified_at.fract() != 0.0 || modified_at < 0.0 || modified_at > i64::MAX as f64 {
        return Err("modified_at must be a non-negative integer timestamp".to_owned());
    }
    run_blocking("delete_book_link", move || library.delete_book_link(&link_id, modified_at as i64).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn download_book(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<DownloadState, String> {
    let library = library(&state.core, &library_id)?;
    library.download_book(parse_content_hash(&content_hash)?).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn remove_local_book_copy(state: State<'_, AppState>, library_id: String, content_hash: String) -> Result<DownloadState, String> {
    let library = library(&state.core, &library_id)?;
    let content_hash = parse_content_hash(&content_hash)?;
    run_blocking("remove_local_book_copy", move || library.remove_local_book_copy(content_hash).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn download_folder(state: State<'_, AppState>, library_id: String, dir_id: String) -> Result<FolderDownloadStatus, String> {
    let library = library(&state.core, &library_id)?;
    let dir_id = epub_svelte_lib::DirId::parse_str(&dir_id).map_err(|error| error.to_string())?;
    library.download_folder(dir_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
fn backend_link_status() -> String {
    log::info!("backend_link_status invoked; backend crate linked");
    "Backend crate linked and Rust logging is active.".to_string()
}

pub fn export_bindings(path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    commands::export_bindings(path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let command_builder = commands::builder();
    #[cfg(all(debug_assertions, desktop))]
    export_bindings(Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/bindings.ts")).expect("failed to export Tauri bindings");

    tauri::Builder::default()
        .setup(|app| {
            #[cfg(mobile)]
            let app_data = app.path().app_data_dir()?;
            // The location policy lives in the backend. The app shell only
            // chooses shared-desktop or OS-provided platform-local storage.
            #[cfg(desktop)]
            let app_data_location = AppDataLocation::SharedDesktop;
            #[cfg(mobile)]
            let app_data_location = AppDataLocation::PlatformLocal(app_data);

            let core = AppBackend::initialize_on_tokio_runtime(app_data_location, tauri::async_runtime::handle().inner().clone()).map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let events = core.take_events()?;
            app.manage(AppState::new(core));
            events::spawn_library_event_forwarder(app.handle().clone(), events);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).targets([Target::new(TargetKind::Stdout), Target::new(TargetKind::LogDir { file_name: None }), Target::new(TargetKind::Webview)]).build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(command_builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use epub_svelte_lib::app::{AccountSessionSummary, AccountStatus, AppBackend, AppEvent, LibraryCatalogueState, LibrarySyncStatus};
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::catalog::{
    opds_endpoint_id, opds_source, CatalogAuthorDiscoveryResult, CatalogImportRequest, CatalogPage, CatalogPublicationIdentity, CatalogPublicationOwnership, CatalogSearch, CatalogSearchQuery, CatalogSource, CatalogSourceId,
    CatalogWideSearchResult,
};
use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::{AppDataLocation, LibraryEntry};
use epub_svelte_lib::library::{BookCardRow, BookImportDestination, Dir, LibraryAuthorsView, LibraryFolderView, LibraryHomeView};
use epub_svelte_lib::app::preferences::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences, TransferPreferencePatch, TransferPreferences};
use epub_svelte_lib::catalog::opds::{OpdsCredentials, OpdsEndpoint};
use epub_svelte_lib::library::links::BookLink;
use epub_svelte_lib::library::marks::{ReaderAnnotation, ReaderBookmark};
use epub_svelte_lib::platform::scanner::LibraryEvent;
use epub_svelte_lib::{DownloadState, FolderDownloadStatus, LibraryTransfers, NetworkStatus};
use specta::Type;
use specta_typescript::Typescript;
use std::path::Path;
use std::time::Instant;
use tauri::ipc::{InvokeResponseBody, IpcResponse};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_log::{Target, TargetKind};
use tauri_specta::{collect_commands, Builder, ErrorHandlingMode};

struct AppState {
    core: AppBackend,
}

#[derive(Type)]
#[specta(type = Vec<u8>)]
struct BinaryBytes(Vec<u8>);

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

fn opds_credentials(endpoint: &OpdsEndpoint, username: Option<String>, password: Option<String>) -> Result<Option<OpdsCredentials>, String> {
    let Some(username) = username.map(|username| username.trim().to_owned()).filter(|username| !username.is_empty()) else { return Ok(None) };
    OpdsCredentials::for_endpoint(endpoint, username, password.unwrap_or_default()).map(Some).map_err(|error| error.to_string())
}

fn catalog_credentials(core: &AppBackend, source_id: &CatalogSourceId, username: Option<String>, password: Option<String>) -> Result<Option<OpdsCredentials>, String> {
    match core.opds_endpoint_for_catalog_source(source_id).map_err(|error| error.to_string())? {
        Some(endpoint) => opds_credentials(&endpoint, username, password),
        None => Ok(None),
    }
}

fn library(core: &AppBackend, library_id: &str) -> Result<LibraryBackend, String> {
    core.library(library_id).map_err(|error| error.to_string())
}

fn parse_book_id(value: &str) -> Result<epub_svelte_lib::BookId, String> {
    epub_svelte_lib::BookId::parse(value).map_err(|error| error.to_string())
}

fn library_event_name(event: &LibraryEvent) -> &'static str {
    match event {
        LibraryEvent::BookAdded { .. } => "library:book-added",
        LibraryEvent::BookRemoved { .. } => "library:book-removed",
        LibraryEvent::DirectoryAdded { .. } => "library:directory-added",
        LibraryEvent::DirectoryRemoved { .. } => "library:directory-removed",
        LibraryEvent::ThumbnailGenerated { .. } => "library:thumbnail-generated",
        LibraryEvent::DownloadStatusChanged { .. } => "library:download-status",
        LibraryEvent::FolderDownloadStatusChanged { .. } => "library:folder-download-status",
        LibraryEvent::TransferStatusChanged => "library:transfer-status",
        LibraryEvent::BookOpened { .. } => "library:book-opened",
        LibraryEvent::ScanComplete => "library:scan-complete",
    }
}

fn spawn_library_event_forwarder(app_handle: AppHandle, event_rx: crossbeam_channel::Receiver<AppEvent>) {
    std::thread::spawn(move || {
        while let Ok(app_event) = event_rx.recv() {
            let event_name = library_event_name(&app_event.event);
            if let Err(e) = app_handle.emit(event_name, &app_event) {
                log::error!("Failed to emit event {}: {}", event_name, e);
            }
        }
    });
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
async fn login(state: State<'_, AppState>, username: String, password: String) -> Result<AccountStatus, String> {
    state.core.login(&username, &password).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn request_public_registration(state: State<'_, AppState>, email: String, username: String, password: String) -> Result<(), String> {
    state.core.request_public_registration(&email, &username, &password).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn resend_verification(state: State<'_, AppState>, email: String) -> Result<(), String> {
    state.core.resend_verification(&email).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn verify_email(state: State<'_, AppState>, token: String) -> Result<AccountStatus, String> {
    state.core.verify_email(&token).await.map_err(|error| error.to_string())
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
async fn reconcile_library_catalogue(state: State<'_, AppState>) -> Result<LibraryCatalogueState, String> {
    state.core.reconcile_library_catalogue().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_transfer_statuses(state: State<'_, AppState>) -> Result<Vec<LibraryTransfers>, String> {
    let core = state.core.clone();
    run_blocking("get_transfer_statuses", move || core.transfer_statuses().map_err(|error| error.to_string())).await
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
async fn get_transfer_preferences(state: State<'_, AppState>) -> Result<TransferPreferences, String> {
    let core = state.core.clone();
    run_blocking("get_transfer_preferences", move || core.transfer_preferences().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_transfer_preference(state: State<'_, AppState>, patch: TransferPreferencePatch) -> Result<TransferPreferences, String> {
    state.core.update_transfer_preference(patch).await.map_err(|error| error.to_string())
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
    let core = state.core.clone();
    run_blocking("remove_library", move || core.remove_library(&library_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_library_folder(state: State<'_, AppState>, library_id: String, dir_id: String) -> Result<LibraryFolderView, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_library_folder", move || library.folder(&dir_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_child_directories(state: State<'_, AppState>, library_id: String, parent_id: String) -> Result<Vec<Dir>, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_child_directories", move || library.child_directories(&parent_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn create_directory(state: State<'_, AppState>, library_id: String, parent_id: String, name: String) -> Result<Dir, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("create_directory", move || library.create_directory(&parent_id, &name).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn import_book(state: State<'_, AppState>, destination: BookImportDestination, source_path: String, file_name: String) -> Result<(), String> {
    let library = library(&state.core, destination.library_id().as_str())?;
    let directory_id = destination.directory_id().as_str().to_owned();
    run_blocking("import_book", move || library.import_book(&directory_id, std::path::Path::new(&source_path), &file_name).map(|_| ()).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
fn get_opds_endpoints(state: State<'_, AppState>) -> Vec<OpdsEndpoint> {
    state.core.opds_endpoints()
}

#[tauri::command]
#[specta::specta]
async fn refresh_opds_endpoints(state: State<'_, AppState>) -> Result<Vec<OpdsEndpoint>, String> {
    state.core.refresh_opds_endpoints().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
fn get_catalog_sources(state: State<'_, AppState>) -> Vec<CatalogSource> {
    state.core.catalog_sources()
}

#[tauri::command]
#[specta::specta]
async fn refresh_catalog_sources(state: State<'_, AppState>) -> Result<Vec<CatalogSource>, String> {
    state.core.refresh_opds_endpoints().await.map(|_| state.core.catalog_sources()).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
fn get_custom_catalog_sources(state: State<'_, AppState>) -> Result<Vec<CatalogSource>, String> {
    state.core.custom_catalog_sources().map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn add_custom_catalog(state: State<'_, AppState>, title: String, url: String, username_label: Option<String>, password_required: bool) -> Result<CatalogSource, String> {
    let core = state.core.clone();
    run_blocking("add_custom_catalog", move || core.add_custom_opds_endpoint(&title, &url, username_label.as_deref(), password_required).map(opds_source).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn remove_custom_catalog(state: State<'_, AppState>, source_id: CatalogSourceId) -> Result<(), String> {
    let endpoint_id = opds_endpoint_id(&source_id).map_err(|error| error.to_string())?;
    let core = state.core.clone();
    run_blocking("remove_custom_catalog", move || core.remove_custom_opds_endpoint(&endpoint_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_catalogs(state: State<'_, AppState>, query: String) -> Result<CatalogWideSearchResult, String> {
    state.core.search_catalogs(&query).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_page(state: State<'_, AppState>, source_id: CatalogSourceId, locator: Option<String>, username: Option<String>, password: Option<String>) -> Result<CatalogPage, String> {
    let credentials = catalog_credentials(&state.core, &source_id, username, password)?;
    state.core.catalog_page(&source_id, locator.as_deref(), credentials.as_ref()).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn search_catalog(state: State<'_, AppState>, source_id: CatalogSourceId, search: CatalogSearch, query: CatalogSearchQuery, username: Option<String>, password: Option<String>) -> Result<CatalogPage, String> {
    let credentials = catalog_credentials(&state.core, &source_id, username, password)?;
    state.core.search_catalog(&source_id, &search, query, credentials.as_ref()).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_cover(state: State<'_, AppState>, source_id: CatalogSourceId, image_url: String) -> Result<BinaryBytes, String> {
    state.core.catalog_cover(&source_id, &image_url).await.map(BinaryBytes).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_catalog_ownership(state: State<'_, AppState>, library_id: String, source_id: CatalogSourceId, publications: Vec<CatalogPublicationIdentity>) -> Result<Vec<CatalogPublicationOwnership>, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_catalog_ownership", move || library.catalog_ownership(&source_id, &publications).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn import_catalog_book(state: State<'_, AppState>, destination: BookImportDestination, request: CatalogImportRequest, username: Option<String>, password: Option<String>) -> Result<(), String> {
    let credentials = catalog_credentials(&state.core, &request.source_id, username, password)?;
    let library = library(&state.core, destination.library_id().as_str())?;
    library.import_catalog_book(destination.directory_id().as_str(), &request, credentials.as_ref()).await.map(|_| ()).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_library_home(state: State<'_, AppState>, library_id: String, limit: i32) -> Result<LibraryHomeView, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("get_library_home", move || library.home(limit).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_library(state: State<'_, AppState>, library_id: String, query: String, limit: u32) -> Result<Vec<BookCardRow>, String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("search_library", move || library.search(&query, limit as usize).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_library_authors(state: State<'_, AppState>, library_id: String, selected_author_id: Option<String>) -> Result<LibraryAuthorsView, String> {
    let library = library(&state.core, &library_id)?;
    let selected = selected_author_id.map(|author_id| epub_svelte_lib::AuthorId::parse(&author_id).map_err(|error| error.to_string())).transpose()?;
    run_blocking("get_library_authors", move || library.authors(selected).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn discover_catalog_author_books(state: State<'_, AppState>, library_id: String, author_id: String) -> Result<CatalogAuthorDiscoveryResult, String> {
    let library = library(&state.core, &library_id)?;
    let author_id = epub_svelte_lib::AuthorId::parse(&author_id).map_err(|error| error.to_string())?;
    library.discover_author_catalog(author_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_book_thumbnail(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<BinaryBytes, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    let bytes = run_blocking("get_book_thumbnail", move || library.thumbnail(book_id).map(|bytes| bytes.unwrap_or_default()).map_err(|error| error.to_string())).await?;
    Ok(BinaryBytes(bytes))
}

#[tauri::command]
#[specta::specta]
async fn open_book(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<BinaryBytes, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    let bytes = run_blocking("open_book", move || library.read_book_bytes(book_id).map_err(|error| error.to_string())).await?;
    Ok(BinaryBytes(bytes))
}

#[tauri::command]
#[specta::specta]
async fn get_book_file_name(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<String, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("get_book_file_name", move || library.book_file_name(book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_book_reading_position(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<Option<String>, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("get_book_reading_position", move || library.reading_position(book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_reading_position(state: State<'_, AppState>, library_id: String, book_id: String, cfi: String, progress: f32) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("update_reading_position", move || library.update_reading_position(book_id, &cfi, progress).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_last_read(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("update_last_read", move || library.update_last_read(book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_annotations(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<Vec<ReaderAnnotation>, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("get_annotations", move || library.annotations(book_id).map_err(|error| error.to_string())).await
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
async fn get_bookmarks(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<Vec<ReaderBookmark>, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("get_bookmarks", move || library.bookmarks(book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn upsert_bookmark(state: State<'_, AppState>, library_id: String, bookmark: ReaderBookmark) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    run_blocking("upsert_bookmark", move || library.upsert_bookmark(&bookmark).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn delete_bookmark(state: State<'_, AppState>, library_id: String, bookmark_id: String, modified_at: f64) -> Result<(), String> {
    let library = library(&state.core, &library_id)?;
    if !modified_at.is_finite() || modified_at.fract() != 0.0 || modified_at < 0.0 || modified_at > i64::MAX as f64 {
        return Err("modified_at must be a non-negative integer timestamp".to_owned());
    }
    let modified_at = modified_at as i64;
    run_blocking("delete_bookmark", move || library.delete_bookmark(&bookmark_id, modified_at).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_book_links(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<Vec<BookLink>, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("get_book_links", move || library.book_links(book_id).map_err(|error| error.to_string())).await
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
async fn download_book(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<DownloadState, String> {
    let library = library(&state.core, &library_id)?;
    library.download_book(parse_book_id(&book_id)?).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn remove_local_book_copy(state: State<'_, AppState>, library_id: String, book_id: String) -> Result<DownloadState, String> {
    let library = library(&state.core, &library_id)?;
    let book_id = parse_book_id(&book_id)?;
    run_blocking("remove_local_book_copy", move || library.remove_local_book_copy(book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn download_folder(state: State<'_, AppState>, library_id: String, dir_id: String) -> Result<FolderDownloadStatus, String> {
    let library = library(&state.core, &library_id)?;
    let dir_id = epub_svelte_lib::DirId::parse(&dir_id).map_err(|error| error.to_string())?;
    library.download_folder(dir_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
fn backend_link_status() -> String {
    let sample_book_id = epub_svelte_lib::BookId::new(0);
    log::info!("backend_link_status invoked; backend crate linked with sample book id {}", sample_book_id);
    "Backend crate linked and Rust logging is active.".to_string()
}

fn command_builder() -> Builder<tauri::Wry> {
    Builder::new()
        .commands(collect_commands![
            init_logging,
            backend_link_status,
            get_libraries,
            get_account_status,
            get_account_sessions,
            revoke_account_session,
            revoke_other_account_sessions,
            login,
            request_public_registration,
            resend_verification,
            verify_email,
            request_password_reset,
            reset_password,
            bootstrap_remote_libraries,
            sync_all_libraries,
            get_library_sync_statuses,
            reconcile_library_catalogue,
            get_transfer_statuses,
            get_network_status,
            update_network_status,
            get_transfer_preferences,
            update_transfer_preference,
            logout,
            get_reader_preferences,
            update_reader_preference,
            get_browsing_preferences,
            update_browsing_preference,
            create_library,
            open_library_folder,
            remove_library,
            get_library_folder,
            get_child_directories,
            create_directory,
            import_book,
            get_opds_endpoints,
            refresh_opds_endpoints,
            get_catalog_sources,
            refresh_catalog_sources,
            get_custom_catalog_sources,
            add_custom_catalog,
            remove_custom_catalog,
            search_catalogs,
            get_catalog_page,
            search_catalog,
            get_catalog_cover,
            get_catalog_ownership,
            import_catalog_book,
            get_library_home,
            search_library,
            get_library_authors,
            discover_catalog_author_books,
            get_book_thumbnail,
            open_book,
            get_book_file_name,
            get_book_reading_position,
            update_reading_position,
            update_last_read,
            get_annotations,
            upsert_annotation,
            delete_annotation,
            get_bookmarks,
            upsert_bookmark,
            delete_bookmark,
            get_book_links,
            upsert_book_link,
            delete_book_link,
            download_book,
            remove_local_book_copy,
            download_folder
        ])
        .typ::<FrontendContract>()
        .constant("FRONTEND_CONTRACT", FrontendContract::default())
        .error_handling(ErrorHandlingMode::Throw)
}

pub fn export_bindings(path: impl AsRef<Path>) -> Result<(), specta_typescript::Error> {
    command_builder().export(Typescript::default(), path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let command_builder = command_builder();
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
            spawn_library_event_forwarder(app.handle().clone(), events);
            app.manage(AppState { core });
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

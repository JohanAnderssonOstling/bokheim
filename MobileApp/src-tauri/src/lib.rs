use epub_svelte_lib::app_core::{AccountStatus, AppCore, AppEvent, LibraryCatalogueState, LibrarySyncStatus};
use epub_svelte_lib::frontend_contract::FrontendContract;
use epub_svelte_lib::home::{AppDataLocation, LibraryEntry};
use epub_svelte_lib::library::{AuthorFolderBookSummaries, AuthorSummary, BookSummary, Dir, DirSummaryContent};
use epub_svelte_lib::opds::{OpdsCatalog, OpdsCredentials, OpdsEndpoint, OpdsEndpointId, OpdsEpubAcquisition, OpdsSearch};
use epub_svelte_lib::preferences::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences, TransferPreferencePatch, TransferPreferences};
use epub_svelte_lib::scanner::LibraryEvent;
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
    core: AppCore,
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

fn opds_credentials(endpoint: OpdsEndpointId, username: Option<String>, password: Option<String>) -> Result<Option<OpdsCredentials>, String> {
    let Some(username) = username.map(|username| username.trim().to_owned()).filter(|username| !username.is_empty()) else { return Ok(None) };
    OpdsCredentials::for_endpoint(endpoint, username, password.unwrap_or_default()).map(Some).map_err(|error| error.to_string())
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
async fn login(state: State<'_, AppState>, username: String, password: String) -> Result<AccountStatus, String> {
    state.core.login(&username, &password).await.map_err(|error| error.to_string())
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
async fn upload_and_sync_library(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    state.core.upload_and_sync_library(&library_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn disable_library_sync(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    state.core.disable_library_sync(&library_id).await.map_err(|error| error.to_string())
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
async fn remove_library(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    let core = state.core.clone();
    run_blocking("remove_library", move || core.remove_library(&library_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn select_library(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    let core = state.core.clone();
    run_blocking("select_library", move || core.select_library(&library_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_dir_contents(state: State<'_, AppState>, dir_id: String) -> Result<DirSummaryContent, String> {
    let core = state.core.clone();
    run_blocking("get_dir_contents", move || core.directory_summary(&dir_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn create_directory(state: State<'_, AppState>, parent_id: String, name: String) -> Result<Dir, String> {
    let core = state.core.clone();
    run_blocking("create_directory", move || core.create_directory(&parent_id, &name).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn import_book(state: State<'_, AppState>, parent_id: String, source_path: String, file_name: String) -> Result<BookSummary, String> {
    let core = state.core.clone();
    run_blocking("import_book", move || core.import_book(&parent_id, std::path::Path::new(&source_path), &file_name).map(BookSummary::from).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
fn get_opds_endpoints(state: State<'_, AppState>) -> Vec<OpdsEndpoint> {
    state.core.opds_endpoints()
}

#[tauri::command]
#[specta::specta]
async fn get_opds_catalog(state: State<'_, AppState>, endpoint: OpdsEndpointId, url: Option<String>, username: Option<String>, password: Option<String>) -> Result<OpdsCatalog, String> {
    let credentials = opds_credentials(endpoint, username, password)?;
    match url {
        Some(url) => state.core.opds_catalog(&url, credentials.as_ref()).await,
        None => state.core.opds_endpoint_catalog(endpoint, credentials.as_ref()).await,
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn search_opds_catalog(state: State<'_, AppState>, endpoint: OpdsEndpointId, search: OpdsSearch, query: String, username: Option<String>, password: Option<String>) -> Result<OpdsCatalog, String> {
    let credentials = opds_credentials(endpoint, username, password)?;
    state.core.search_opds(&search, &query, credentials.as_ref()).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn import_opds_epub(state: State<'_, AppState>, endpoint: OpdsEndpointId, acquisition: OpdsEpubAcquisition, username: Option<String>, password: Option<String>) -> Result<BookSummary, String> {
    let credentials = opds_credentials(endpoint, username, password)?;
    state.core.import_opds_epub("__root__", &acquisition, credentials.as_ref()).await.map(BookSummary::from).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn get_recently_read(state: State<'_, AppState>, limit: i32) -> Result<Vec<BookSummary>, String> {
    let core = state.core.clone();
    run_blocking("get_recently_read", move || core.recently_read_summaries(limit).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_recently_added(state: State<'_, AppState>, limit: i32) -> Result<Vec<BookSummary>, String> {
    let core = state.core.clone();
    run_blocking("get_recently_added", move || core.recently_added_summaries(limit).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn search_books(state: State<'_, AppState>, query: String, limit: u32) -> Result<Vec<BookSummary>, String> {
    let core = state.core.clone();
    run_blocking("search_books", move || core.search_book_summaries(&query, limit as usize).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_authors(state: State<'_, AppState>) -> Result<Vec<AuthorSummary>, String> {
    let core = state.core.clone();
    run_blocking("get_authors", move || core.authors().map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_author_folder_books(state: State<'_, AppState>, author_name: String) -> Result<Vec<AuthorFolderBookSummaries>, String> {
    let core = state.core.clone();
    run_blocking("get_author_folder_books", move || core.author_folder_book_summaries(&author_name).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn get_book_thumbnail(state: State<'_, AppState>, book_id: String) -> Result<BinaryBytes, String> {
    let core = state.core.clone();
    let bytes = run_blocking("get_book_thumbnail", move || core.thumbnail(&book_id).map(|bytes| bytes.unwrap_or_default()).map_err(|error| error.to_string())).await?;
    Ok(BinaryBytes(bytes))
}

#[tauri::command]
#[specta::specta]
async fn open_book(state: State<'_, AppState>, book_id: String) -> Result<BinaryBytes, String> {
    let core = state.core.clone();
    let bytes = run_blocking("open_book", move || core.read_book_bytes(&book_id).map_err(|error| error.to_string())).await?;
    Ok(BinaryBytes(bytes))
}

#[tauri::command]
#[specta::specta]
async fn get_book_reading_position(state: State<'_, AppState>, book_id: String) -> Result<Option<String>, String> {
    let core = state.core.clone();
    run_blocking("get_book_reading_position", move || core.reading_position(&book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_reading_position(state: State<'_, AppState>, book_id: String, cfi: String) -> Result<(), String> {
    let core = state.core.clone();
    run_blocking("update_reading_position", move || core.update_reading_position(&book_id, &cfi).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn update_last_read(state: State<'_, AppState>, book_id: String) -> Result<(), String> {
    let core = state.core.clone();
    run_blocking("update_last_read", move || core.update_last_read(&book_id).map_err(|error| error.to_string())).await
}

#[tauri::command]
#[specta::specta]
async fn download_book(state: State<'_, AppState>, book_id: String) -> Result<DownloadState, String> {
    state.core.download_book(&book_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn download_folder(state: State<'_, AppState>, dir_id: String) -> Result<FolderDownloadStatus, String> {
    state.core.download_folder(&dir_id).await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
async fn sync_once(state: State<'_, AppState>) -> Result<(), String> {
    let started = Instant::now();
    let result = state.core.sync_once().await.map_err(|error| error.to_string());
    #[cfg(debug_assertions)]
    log::info!(target: "performance", "tauri.command operation=sync_once duration_ms={:.2}", started.elapsed().as_secs_f64() * 1000.0);
    result
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
            login,
            bootstrap_remote_libraries,
            sync_all_libraries,
            get_library_sync_statuses,
            reconcile_library_catalogue,
            upload_and_sync_library,
            disable_library_sync,
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
            remove_library,
            select_library,
            get_dir_contents,
            create_directory,
            import_book,
            get_opds_endpoints,
            get_opds_catalog,
            search_opds_catalog,
            import_opds_epub,
            get_recently_read,
            get_recently_added,
            search_books,
            get_authors,
            get_author_folder_books,
            get_book_thumbnail,
            open_book,
            get_book_reading_position,
            update_reading_position,
            update_last_read,
            download_book,
            download_folder,
            sync_once
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

            let core = AppCore::initialize(app_data_location).map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
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

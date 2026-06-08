use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use epub_svelte_lib::home::{self, LibraryEntry};
use epub_svelte_lib::library::{AuthorFolderBooks, AuthorSummary, DirContent, Library};
use epub_svelte_lib::scanner::LibraryEvent;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_log::{Target, TargetKind};

struct AppState {
    libraries: Mutex<HashMap<String, Arc<Library>>>,
    active_library_id: Mutex<Option<String>>,
    app_data: PathBuf,
}

async fn resolve_active_library(state: &State<'_, AppState>) -> Result<Arc<Library>, String> {
    let active_id = {
        let guard = state.active_library_id.lock().await;
        guard.clone().ok_or_else(|| "no active library selected".to_string())?
    };
    let guard = state.libraries.lock().await;
    guard.get(&active_id).cloned().ok_or_else(|| format!("active library handle missing: {active_id}"))
}

fn library_event_name(event: &LibraryEvent) -> &'static str {
    match event {
        LibraryEvent::BookAdded { .. } => "library:book-added",
        LibraryEvent::BookRemoved { .. } => "library:book-removed",
        LibraryEvent::DirectoryAdded { .. } => "library:directory-added",
        LibraryEvent::DirectoryRemoved { .. } => "library:directory-removed",
        LibraryEvent::ThumbnailGenerated { .. } => "library:thumbnail-generated",
        LibraryEvent::DownloadStatusChanged { .. } => "library:download-status",
        LibraryEvent::BookOpened { .. } => "library:book-opened",
        LibraryEvent::ScanComplete => "library:scan-complete",
    }
}

fn spawn_library_event_forwarder(app_handle: AppHandle, event_rx: crossbeam_channel::Receiver<LibraryEvent>) {
    std::thread::spawn(move || {
        while let Ok(event) = event_rx.recv() {
            let event_name = library_event_name(&event);
            if let Err(e) = app_handle.emit(event_name, &event) {
                log::error!("Failed to emit event {}: {}", event_name, e);
            }
        }
    });
}

#[tauri::command]
fn init_logging() {
    epub_svelte_lib::init_logging();
}

#[tauri::command]
fn get_libraries() -> Vec<LibraryEntry> {
    home::get_library_entries()
}

#[tauri::command]
fn create_library(name: String) -> Result<LibraryEntry, String> {
    home::create_library(name, None).map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_library(state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    let Some(library_path) = home::get_library_path_by_id(&library_id) else {
        return Err(format!("library not found: {library_id}"));
    };
    home::remove_library(&library_path.to_string_lossy());

    {
        let mut libraries = state.libraries.lock().await;
        libraries.remove(&library_id);
    }
    {
        let mut active = state.active_library_id.lock().await;
        if active.as_deref() == Some(library_id.as_str()) {
            *active = None;
        }
    }
    Ok(())
}

#[tauri::command]
async fn select_library(app: AppHandle, state: State<'_, AppState>, library_id: String) -> Result<(), String> {
    let maybe_existing = {
        let libraries = state.libraries.lock().await;
        libraries.get(&library_id).cloned()
    };
    if maybe_existing.is_none() {
        let entry = home::get_library_entries()
            .into_iter()
            .find(|entry| entry.library_id == library_id)
            .ok_or_else(|| format!("library not found: {}", library_id))?;
        let (library, event_rx) = Library::new(&entry.library_path, &entry.library_id);
        let library = Arc::new(library);
        spawn_library_event_forwarder(app.clone(), event_rx);
        let mut libraries = state.libraries.lock().await;
        libraries.insert(library_id.clone(), library);
    }
    let mut active = state.active_library_id.lock().await;
    *active = Some(library_id);
    Ok(())
}

#[tauri::command]
async fn get_dir_contents(state: State<'_, AppState>, dir_id: String) -> Result<DirContent, String> {
    let library = resolve_active_library(&state).await?;
    Ok(library.get_dir_contents(&dir_id))
}

#[tauri::command]
async fn get_recently_read(state: State<'_, AppState>, limit: i32) -> Result<Vec<epub_svelte_lib::library::Book>, String> {
    let library = resolve_active_library(&state).await?;
    Ok(library.get_recently_read(limit))
}

#[tauri::command]
async fn get_recently_added(state: State<'_, AppState>, limit: i32) -> Result<Vec<epub_svelte_lib::library::Book>, String> {
    let library = resolve_active_library(&state).await?;
    Ok(library.get_recently_added(limit))
}

#[tauri::command]
async fn get_authors(state: State<'_, AppState>) -> Result<Vec<AuthorSummary>, String> {
    let library = resolve_active_library(&state).await?;
    Ok(library.get_authors())
}

#[tauri::command]
async fn get_author_folder_books(state: State<'_, AppState>, author_name: String) -> Result<Vec<AuthorFolderBooks>, String> {
    let library = resolve_active_library(&state).await?;
    Ok(library.get_author_folder_books(&author_name))
}

#[tauri::command]
async fn open_book(state: State<'_, AppState>, book_id: String) -> Result<Vec<u8>, String> {
    let library = resolve_active_library(&state).await?;
    library.read_book_bytes(&book_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_reading_position(state: State<'_, AppState>, book_id: u64, cfi: String) -> Result<(), String> {
    let library = resolve_active_library(&state).await?;
    library.update_reading_position(book_id, &cfi);
    Ok(())
}

#[tauri::command]
async fn update_last_read(state: State<'_, AppState>, book_id: String) -> Result<(), String> {
    let library = resolve_active_library(&state).await?;
    library.update_last_read(&book_id);
    Ok(())
}

#[tauri::command]
async fn request_book_download(state: State<'_, AppState>, book_id: String) -> Result<(), String> {
    let library = resolve_active_library(&state).await?;
    library.request_book_download(&book_id);
    Ok(())
}

#[tauri::command]
async fn sync_once(state: State<'_, AppState>) -> Result<(), String> {
    let library = resolve_active_library(&state).await?;
    library.sync_once().await.map_err(|e| e.to_string())
}

#[tauri::command]
fn backend_link_status() -> String {
    let sample_book_id: epub_svelte_lib::BookID = 0;
    log::info!(
        "backend_link_status invoked; backend crate linked with sample book id {}",
        sample_book_id
    );
    "Backend crate linked and Rust logging is active.".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(app_data.join("libraries"))?;
            epub_svelte_lib::home::set_app_data_dir(app_data.clone()).map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
            app.manage(AppState {
                libraries: Mutex::new(HashMap::new()),
                active_library_id: Mutex::new(None),
                app_data,
            });
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            init_logging,
            backend_link_status,
            get_libraries,
            create_library,
            remove_library,
            select_library,
            get_dir_contents,
            get_recently_read,
            get_recently_added,
            get_authors,
            get_author_folder_books,
            open_book,
            update_reading_position,
            update_last_read,
            request_book_download,
            sync_once
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

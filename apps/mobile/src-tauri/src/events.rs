use epub_svelte_lib::app::AppEvent;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use tauri::{AppHandle, Emitter};

fn library_event_name(event: &LibraryEvent) -> &'static str {
    match event {
        LibraryEvent::BookAdded { .. } => "library:book-added",
        LibraryEvent::BookRemoved { .. } => "library:book-removed",
        LibraryEvent::DirectoryAdded { .. } => "library:directory-added",
        LibraryEvent::DirectoryRemoved { .. } => "library:directory-removed",
        LibraryEvent::ThumbnailGenerated { .. } => "library:thumbnail-generated",
        LibraryEvent::ThumbnailBatchProcessed { .. } => "library:thumbnail-batch-processed",
        LibraryEvent::SubjectEnrichmentBatchProcessed { .. } => "library:subject-enrichment-batch-processed",
        LibraryEvent::DownloadStatusChanged { .. } => "library:download-status",
        LibraryEvent::FolderDownloadStatusChanged { .. } => "library:folder-download-status",
        LibraryEvent::TransferStatusChanged => "library:transfer-status",
        LibraryEvent::LibraryChanged => "library:changed",
        LibraryEvent::LibraryListChanged => "library:list-changed",
        LibraryEvent::ScanComplete => "library:scan-complete",
    }
}
pub(crate) fn spawn_library_event_forwarder(app_handle: AppHandle, event_rx: crossbeam_channel::Receiver<AppEvent>) {
    std::thread::spawn(move || {
        while let Ok(app_event) = event_rx.recv() {
            let event_name = library_event_name(&app_event.event);
            if let Err(error) = app_handle.emit(event_name, &app_event) {
                log::error!("Failed to emit event {}: {}", event_name, error);
            }
        }
    });
}

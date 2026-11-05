//! UI-owned browser launching; the selected library owns downloads and imports.
use app_preferences::BrowsingPreferences;
#[cfg(not(target_arch = "wasm32"))]
use embedded_browser::{BrowserEvent, BrowserFeedback, BrowserWindow};
use library_backend::{BackendError, LibraryClient};
use library_backend::executor::BackendExecutor;
#[cfg(not(target_arch = "wasm32"))]
use library_backend::{Url, executor::run_blocking};
#[cfg(not(target_arch = "wasm32"))]
use library_model::BrowserDownloadPhase;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn available(executor: &BackendExecutor) -> bool {
    executor.with_io_context(run_blocking(embedded_browser::available)).await.unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn available(_executor: &BackendExecutor) -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn open(executor: &BackendExecutor, library: LibraryClient, preferences: BrowsingPreferences) -> Result<(), BackendError> {
    executor.with_io_context(async {
        let downloads = library.browser_downloads().await?;
        let url = Url::parse("about:blank").map_err(BackendError::operation)?;
        let window = embedded_browser::open(&url, "Bokheim – Browser", downloads.directory(), preferences).await?;
        attach_downloads(executor, downloads, window);
        Ok(())
    }).await
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn open(_executor: &BackendExecutor, _library: LibraryClient, _preferences: BrowsingPreferences) -> Result<(), BackendError> {
    Err(BackendError::message("the embedded browser is unavailable on this platform"))
}

#[cfg(not(target_arch = "wasm32"))]
/// Retain the session while the window reports downloads and import results.
fn attach_downloads(executor: &BackendExecutor, downloads: library_backend::client::BrowserDownloads, mut window: BrowserWindow) {
    executor.spawn_detached(async move {
        let feedback = window.feedback_sender();
        while let Some(event) = window.next_event().await {
            match event {
                BrowserEvent::Downloaded { path, download } => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let _ = feedback.send(BrowserFeedback { download: download.clone(), name: name.clone(), phase: BrowserDownloadPhase::Preparing, percent: None, message: None }).await;
                    let phase = match downloads.import(path).await {
                        Ok(()) => BrowserDownloadPhase::Completed,
                        Err(error) => {
                            log::warn!("a browser download was not added to the selected library: {error}");
                            BrowserDownloadPhase::Failed(error.failure())
                        }
                    };
                    let _ = feedback.send(BrowserFeedback { download, name, phase, percent: None, message: None }).await;
                }
                BrowserEvent::CancelDownload { download } => {
                    let _ = feedback.send(BrowserFeedback { download, name: String::new(), phase: BrowserDownloadPhase::Dismissed, percent: None, message: None }).await;
                }
                BrowserEvent::Opened => {}
            }
        }
    });
}

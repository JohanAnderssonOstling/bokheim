//! Download sessions bound to one concrete library, independent of browser UI.
use super::LibraryClient;
use crate::executor::spawn_detached;
use crate::{BackendError, ROOT_DIR_ID};
use embedded_browser::{BrowserEvent, BrowserFeedback, BrowserWindow};
use library_model::BrowserDownloadPhase;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Owns a library's staging folder until its browser and imports finish.
pub struct BrowserDownloads {
    library: LibraryClient,
    directory: TempDir,
}

impl LibraryClient {
    /// Prepare a hidden download folder in this library's current root.
    pub async fn browser_downloads(&self) -> Result<BrowserDownloads, BackendError> {
        let directory = self.transport.handle().await?.prepare_browser_downloads().await?;
        Ok(BrowserDownloads { library: self.clone(), directory })
    }
}

impl BrowserDownloads {
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// Move a completed download into this library's root and register it.
    pub async fn import(&self, path: PathBuf) -> Result<(), BackendError> {
        let path = path.canonicalize().map_err(BackendError::operation)?;
        let directory = self.directory.path().canonicalize().map_err(BackendError::operation)?;
        if path.parent() != Some(directory.as_path()) || !path.is_file() {
            return Err(BackendError::message("browser download is outside this library's staging folder"));
        }
        let failures = self.library.import_paths(ROOT_DIR_ID, vec![path], true).await?;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(BackendError::message(failures.into_iter().map(|failure| failure.diagnostic.unwrap_or_else(|| "book import failed".into())).collect::<Vec<_>>().join("; ")))
        }
    }

    /// Retain the session while the window reports downloads and import results.
    pub fn attach(self, mut window: BrowserWindow) {
        spawn_detached(async move {
            let feedback = window.feedback_sender();
            while let Some(event) = window.next_event().await {
                match event {
                    BrowserEvent::Downloaded { path, download } => {
                        let download = download.unwrap_or_else(|| path.to_string_lossy().into_owned());
                        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let _ = feedback.send(BrowserFeedback { download: download.clone(), name: name.clone(), phase: BrowserDownloadPhase::Preparing, percent: None, message: None }).await;
                        let phase = match self.import(path).await {
                            Ok(()) => BrowserDownloadPhase::Completed,
                            Err(error) => {
                                log::warn!("a browser download was not added to library {}: {error}", self.library.id());
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
}

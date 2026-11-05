//! Download sessions bound to one concrete library, independent of browser UI.
use super::LibraryClient;
use crate::{BackendError, ROOT_DIR_ID};
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
        let directory = self.prepare_browser_downloads().await?;
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
}

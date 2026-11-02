//! Bookstores: websites opened in a window of their own, where what is bought
//! and downloaded is explicitly imported into the library's root. Kept
//! on this device, like the store's sign-in, which lives in this device's
//! webview.
//!
//! Native only: desktop uses a helper window and Android uses an in-app WebView.

use super::Url;
use super::core::*;
use crate::executor::spawn_detached;
use library_backend::LibraryClient;
use library_model::{BrowserDownloadPhase, CardKey, Store, StoreId};
use library_opds::browser::{BrowserFeedback, StoreEvent};
use sync_common::ROOT_DIR_ID;

const STORES: &str = "stores";

impl AppBackend {
    /// The stores, by name; none where a store window cannot open, such as
    /// without WebKitGTK or WebView2.
    pub(crate) fn stores(&self) -> Result<Vec<Store>, BackendError> {
        if !library_opds::browser::stores_available() {
            return Ok(Vec::new());
        }
        let mut stores: Vec<Store> = self.read_registry_json(STORES)?;
        stores.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(stores)
    }

    /// Adds the store at `url`, named `name` or else by its host.
    pub(crate) fn add_store(&self, url: String, name: Option<String>) -> Result<(), BackendError> {
        let url = Url::parse(url.trim()).map_err(|error| BackendError::classified(Failure::InvalidAddress, format!("not a web address: {error}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(BackendError::classified(Failure::InvalidAddress, "a store address must use https:// or http://"));
        }
        let name = name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()).unwrap_or_else(|| url.host_str().unwrap_or("Store").to_owned());
        let store = Store { id: uuid::Uuid::new_v4(), name, url: url.to_string() };
        self.with_storage(move |this| {
            let mut stores: Vec<Store> = this.read_registry_json(STORES)?;
            stores.push(store);
            this.write_registry_json(STORES, &stores)
        })
    }

    pub(crate) fn remove_store(&self, id: StoreId) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            let mut stores: Vec<Store> = this.read_registry_json(STORES)?;
            stores.retain(|store| store.id != id);
            this.write_registry_json(STORES, &stores)
        })
    }

    /// Opens a store's window; what it downloads goes into `library_id`'s root.
    pub(crate) async fn open_store(&self, id: StoreId, library_id: LibraryId) -> Result<(), BackendError> {
        let store = self.stores()?.into_iter().find(|store| store.id == id).ok_or("this store is no longer added")?;
        let url = Url::parse(&store.url).map_err(BackendError::operation)?;
        self.open_store_window(&url, &store.name, library_id).await
    }

    pub(crate) async fn open_browser(&self, library_id: LibraryId) -> Result<(), BackendError> {
        let url = Url::parse("about:blank").map_err(BackendError::operation)?;
        self.open_store_window(&url, "Bokheim – Browser", library_id).await
    }

    /// Opens the store page that sells a listed feed book.
    pub(crate) async fn buy_opds_book(&self, entry: CardKey, library_id: LibraryId) -> Result<(), BackendError> {
        let url = library_opds::buy_page(entry)?;
        let title = url.host_str().unwrap_or("Store").to_owned();
        self.open_store_window(&url, &title, library_id).await
    }

    /// Downloads are saved in a hidden folder inside the root, which the scan
    /// skips, and each finished book is explicitly placed and imported into
    /// the root. The folder goes when the window closes.
    async fn open_store_window(&self, url: &Url, title: &str, library_id: LibraryId) -> Result<(), BackendError> {
        let root = PathBuf::from(self.library_entry(&library_id)?.storage_locator());
        if !root.is_dir() {
            return Err(BackendError::classified(Failure::LibraryFolderUnavailable, "this library's folder is unavailable"));
        }
        let downloads = tempfile::Builder::new().prefix(".bokheim-store-").tempdir_in(&root).map_err(BackendError::operation)?;
        let preferences = self.browsing_preferences()?;
        let library = self.state.library_directory.get(&library_id)?;
        let library = LibraryClient::new(library_id, library);
        let mut window = library_opds::browser::open_store(url, title, downloads.path(), preferences).await?;
        spawn_detached(async move {
            let feedback = window.feedback_sender();
            while let Some(event) = window.next_event().await {
                match event {
                    StoreEvent::Downloaded { path, download } => {
                        let download = download.unwrap_or_else(|| path.to_string_lossy().into_owned());
                        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let _ = feedback.send(BrowserFeedback { download: download.clone(), name: name.clone(), phase: BrowserDownloadPhase::Preparing, percent: None, message: Some("Adding to library".into()) }).await;
                        // Register the book immediately on both desktop and
                        // mobile, and report the actual import result.
                        let (phase, message) = match import_browser_download(&library, path).await {
                            Ok(()) => (BrowserDownloadPhase::Completed, Some("Added to library".into())),
                            Err(error) => {
                                log::warn!("a store download was not added to the library: {error}");
                                (BrowserDownloadPhase::Failed(error.failure()), None)
                            }
                        };
                        let _ = feedback.send(BrowserFeedback { download, name, phase, percent: None, message }).await;
                    }
                    StoreEvent::CancelDownload { download } => {
                        let _ = feedback.send(BrowserFeedback { download, name: String::new(), phase: BrowserDownloadPhase::Dismissed, percent: None, message: None }).await;
                    }
                    StoreEvent::Opened => {}
                }
            }
            drop(downloads);
        });
        Ok(())
    }
}

async fn import_browser_download(library: &LibraryClient, path: PathBuf) -> Result<(), BackendError> {
    let failures = library.import_paths(ROOT_DIR_ID, vec![path], true).await?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(BackendError::message(failures.into_iter().map(|failure| failure.diagnostic.unwrap_or_else(|| "book import failed".into())).collect::<Vec<_>>().join("; ")))
    }
}

#[cfg(test)]
mod progress_tests {
    use super::super::core::AppDataLocation;
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn browser_download_is_registered_before_import_returns() {
        let data = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(data.path())).unwrap()).unwrap();
        let entry = backend.ensure_import_library().unwrap();
        let library = LibraryClient::new(*entry.library_id(), backend.state.library_directory.get(entry.library_id()).unwrap());
        let staging = tempfile::Builder::new().prefix(".bokheim-store-").tempdir_in(entry.storage_locator()).unwrap();
        let path = staging.path().join("browser.epub");
        let bytes = include_bytes!("../../../library-backend/crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
        fs::write(&path, bytes).unwrap();

        import_browser_download(&library, path.clone()).await.unwrap();
        assert!(!path.exists());
        assert_eq!(library.book_count().await.unwrap(), 1);
        assert_eq!(fs::read(Path::new(entry.storage_locator()).join("browser.epub")).unwrap(), bytes);
    }

    #[tokio::test]
    async fn browser_download_reports_import_failures() {
        let data = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(data.path())).unwrap()).unwrap();
        let entry = backend.ensure_import_library().unwrap();
        let library = LibraryClient::new(*entry.library_id(), backend.state.library_directory.get(entry.library_id()).unwrap());
        let staging = tempfile::Builder::new().prefix(".bokheim-store-").tempdir_in(entry.storage_locator()).unwrap();
        let path = staging.path().join("broken.epub");
        fs::write(&path, b"invalid EPUB").unwrap();

        assert!(import_browser_download(&library, path).await.is_err());
        assert_eq!(library.book_count().await.unwrap(), 0);
    }
}

//! Bookstores: websites opened in a window of their own, where what is bought
//! and downloaded is explicitly imported into the library's root. Kept
//! on this device, like the store's sign-in, which lives in this device's
//! webview.
//!
//! Native only: desktop uses a helper window and Android uses an in-app WebView.

use super::Url;
use super::core::*;
use crate::executor::spawn_detached;
use client_platform_runtime::transfers;
use library_backend::LibraryClient;
use library_model::{BrowserDownloadPhase, CardKey, Store, StoreId, TorrentInput, TorrentStatus};
use library_opds::browser::{BrowserFeedback, StoreEvent, StoreFeedbackSender};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
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
        // Snapshot jobs already downloading before accepting this window's
        // events. New requests enter the progress list after their hash resolves.
        let initial = library.torrent_transfers().await.unwrap_or_default();
        spawn_detached(async move {
            let feedback = window.feedback_sender();
            let mut polling = tokio::time::interval(Duration::from_secs(1));
            let (added, mut accepted) = tokio::sync::mpsc::unbounded_channel::<(String, String, Result<String, BackendError>)>();
            let mut tracked: HashMap<String, String> = initial.into_iter().filter(|transfer| !transfer.imported).map(|transfer| (transfer.info_hash.clone(), format!("torrent:{}", transfer.info_hash))).collect();
            let mut pending: HashMap<String, String> = HashMap::new();
            let mut pending_requests: HashMap<String, String> = HashMap::new();
            let mut handovers: HashMap<String, String> = HashMap::new();
            let mut sources: HashMap<String, String> = HashMap::new();
            let mut reported = HashMap::new();
            let mut cancelled = HashSet::new();
            loop {
                let event = tokio::select! {
                    event = window.next_event() => match event { Some(event) => event, None => break },
                    Some((download, request, result)) = accepted.recv() => {
                        if cancelled.remove(&request) {
                            if let Ok(hash) = result {
                                if let Err(error) = library.cancel_torrent(hash).await { log::warn!("Could not cancel torrent download: {error}"); }
                            }
                            continue;
                        }
                        let name = if pending_requests.get(&download) == Some(&request) {
                            pending_requests.remove(&download);
                            pending.remove(&download)
                        } else { None }.unwrap_or_else(|| "Torrent download".into());
                        match result {
                            Ok(hash) => {
                                sources.insert(download.clone(), hash.clone());
                                let id = format!("torrent:{hash}");
                                reported.remove(&id);
                                tracked.insert(hash.clone(), id);
                                if let Some(previous) = handovers.insert(hash, download.clone()).filter(|previous| previous != &download) {
                                    report_feedback(&feedback, &mut reported, BrowserFeedback { replaces: None, download: previous, name: String::new(), phase: BrowserDownloadPhase::Dismissed, percent: None, message: None }).await;
                                }
                            }
                            Err(error) => {
                                log::warn!("a store torrent could not start: {error}");
                                report_feedback(&feedback, &mut reported, BrowserFeedback { replaces: None, download, name, phase: BrowserDownloadPhase::Failed(error.failure()), percent: None, message: None }).await
                            }
                        }
                        continue;
                    }
                    _ = polling.tick() => {
                        for (download, name) in &pending {
                            report_feedback(&feedback, &mut reported, preparing_feedback(download.clone(), name.clone())).await;
                        }
                        if let Ok(torrents) = library.torrent_transfers().await {
                            for torrent in torrents {
                                if let Some(download) = tracked.get(&torrent.info_hash) {
                                    let previous = handovers.remove(&torrent.info_hash);
                                    let mut update = torrent_feedback(download.clone(), torrent, transfers::allowed());
                                    update.replaces = previous;
                                    report_feedback(&feedback, &mut reported, update).await;
                                }
                            }
                        }
                        continue;
                    }
                };
                let (input, download, name) = match event {
                    StoreEvent::CancelDownload { download } => {
                        let hash = tracked.iter().find(|(_, id)| **id == download).map(|(hash, _)| hash.clone())
                            .or_else(|| sources.get(&download).cloned());
                        let was_pending = pending.remove(&download).is_some();
                        let pending_hash = if let Some(request) = pending_requests.remove(&download) {
                            cancelled.insert(request.clone());
                            match library.cancel_torrent_request(request).await {
                                Ok(hash) => hash,
                                Err(error) => { log::warn!("Could not cancel torrent request: {error}"); None }
                            }
                        } else {
                            if was_pending { log::warn!("No torrent request was recorded for pending download {download}"); }
                            None
                        };
                        if let Some(hash) = hash.or(pending_hash) {
                            tracked.remove(&hash);
                            handovers.remove(&hash);
                            sources.retain(|_, source_hash| source_hash != &hash);
                            if let Err(error) = library.cancel_torrent(hash).await { log::warn!("Could not cancel torrent download: {error}"); }
                        }
                        report_feedback(&feedback, &mut reported, BrowserFeedback { replaces: None, download, name: String::new(), phase: BrowserDownloadPhase::Dismissed, percent: None, message: None }).await;
                        continue;
                    }
                    StoreEvent::Magnet { url } => (TorrentInput::Magnet(url), format!("torrent-request:{}", uuid::Uuid::new_v4()), "Magnet download".into()),
                    StoreEvent::TorrentFile { path, download } => {
                        let download = download.unwrap_or_else(|| path.to_string_lossy().into_owned());
                        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        // Read before the browser's temporary directory closes;
                        // the library owns the durable payload staging.
                        if tokio::fs::metadata(&path).await.is_ok_and(|metadata| metadata.len() > 16 * 1024 * 1024) {
                            report_feedback(&feedback, &mut reported, BrowserFeedback { replaces: None, download, name, phase: BrowserDownloadPhase::Failed(Some(Failure::UnreadableBook)), percent: None, message: None }).await;
                            continue;
                        }
                        match tokio::fs::read(&path).await {
                            Ok(bytes) => (TorrentInput::File(bytes.into()), download, name),
                            Err(error) => {
                                log::warn!("could not read torrent metadata: {error}");
                                report_feedback(&feedback, &mut reported, BrowserFeedback { replaces: None, download, name, phase: BrowserDownloadPhase::Failed(Failure::from_io(error.kind())), percent: None, message: None }).await;
                                continue;
                            }
                        }
                    }
                    StoreEvent::Downloaded { path, download } => {
                        let download = download.unwrap_or_else(|| path.to_string_lossy().into_owned());
                        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let _ = feedback.send(BrowserFeedback { replaces: None, download: download.clone(), name: name.clone(), phase: BrowserDownloadPhase::Preparing, percent: None, message: Some("Adding to library".into()) }).await;
                        // Register the book immediately on both desktop and
                        // mobile, and report the actual import result.
                        let (phase, message) = match import_browser_download(&library, path).await {
                            Ok(()) => (BrowserDownloadPhase::Completed, Some("Added to library".into())),
                            Err(error) => {
                                log::warn!("a store download was not added to the library: {error}");
                                (BrowserDownloadPhase::Failed(error.failure()), None)
                            }
                        };
                        let _ = feedback.send(BrowserFeedback { replaces: None, download, name, phase, percent: None, message }).await;
                        continue;
                    }
                    StoreEvent::Opened => continue,
                };
                let request = uuid::Uuid::new_v4().to_string();
                if let Err(error) = library.prepare_torrent_request(request.clone()).await {
                    log::warn!("a store torrent could not be prepared: {error}");
                    report_feedback(&feedback, &mut reported, BrowserFeedback { replaces: None, download, name, phase: BrowserDownloadPhase::Failed(error.failure()), percent: None, message: None }).await;
                    continue;
                }
                reported.remove(&download);
                pending.insert(download.clone(), name.clone());
                pending_requests.insert(download.clone(), request.clone());
                report_feedback(&feedback, &mut reported, preparing_feedback(download.clone(), name)).await;
                let library = library.clone();
                let added = added.clone();
                spawn_detached(async move {
                    let result = library.add_torrent_request(input, request.clone()).await;
                    let _ = added.send((download, request, result));
                });
            }
            // The library owns running torrents. Clear their browser bars when
            // this view stops polling, without leaving a frozen progress toast.
            for download in pending.keys().chain(tracked.values()).chain(handovers.values()) {
                if reported.get(download).is_none_or(|update| !matches!(update.phase, BrowserDownloadPhase::Completed | BrowserDownloadPhase::Failed(_))) {
                    let _ = feedback.send(BrowserFeedback { replaces: None, download: download.clone(), name: String::new(), phase: BrowserDownloadPhase::Dismissed, percent: None, message: None }).await;
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

fn preparing_feedback(download: String, name: String) -> BrowserFeedback {
    BrowserFeedback {
        replaces: None,
        download,
        name,
        phase: BrowserDownloadPhase::Preparing,
        percent: None,
        message: Some("Resolving torrent metadata".into()),
    }
}

fn torrent_feedback(download: String, torrent: TorrentStatus, allowed: bool) -> BrowserFeedback {
    // The torrent engine reports failures only as text; they stay unclassified.
    let (phase, percent, message) = if torrent.error.is_some() {
        (BrowserDownloadPhase::Failed(None), None, None)
    } else if torrent.imported {
        (
            BrowserDownloadPhase::Completed,
            None,
            Some(
                if torrent.keep_seeding && !allowed {
                    "Added to library; seeding waits for an unmetered connection"
                } else if torrent.keep_seeding {
                    "Added to library; seeding continues"
                } else {
                    "Added to library"
                }
                .into(),
            ),
        )
    } else if torrent.total_bytes > 0 {
        (BrowserDownloadPhase::Downloading, Some((torrent.progress_bytes.saturating_mul(100) / torrent.total_bytes).min(100) as u8), None)
    } else {
        (BrowserDownloadPhase::Preparing, None, None)
    };
    BrowserFeedback { replaces: None, download, name: torrent.name, phase, percent, message }
}

async fn report_feedback(sender: &StoreFeedbackSender, reported: &mut HashMap<String, BrowserFeedback>, update: BrowserFeedback) {
    let mut cached = update.clone();
    cached.replaces = None;
    if update.replaces.is_some() || reported.get(&update.download) != Some(&cached) {
        reported.insert(update.download.clone(), cached);
        let _ = sender.send(update).await;
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

    fn torrent() -> TorrentStatus {
        TorrentStatus { info_hash: "a".repeat(40), name: "Books".into(), progress_bytes: 42, total_bytes: 100, uploaded_bytes: 0, imported: false, keep_seeding: true, error: None }
    }

    #[test]
    fn torrent_updates_preserve_the_transfer_id_and_use_shared_phases() {
        let update = torrent_feedback("metadata-download".into(), torrent(), true);
        assert_eq!(update.download, "metadata-download");
        assert_eq!(update.phase, BrowserDownloadPhase::Downloading);
        assert_eq!(update.percent, Some(42));
        let metered = torrent_feedback("metadata-download".into(), torrent(), false);
        assert_eq!(metered.phase, BrowserDownloadPhase::Downloading);
        assert_eq!(metered.percent, Some(42));
        let mut checking = torrent();
        checking.total_bytes = 0;
        assert_eq!(torrent_feedback("second".into(), checking, true).phase, BrowserDownloadPhase::Preparing);
        let mut oversized = torrent();
        oversized.progress_bytes = 101;
        assert_eq!(torrent_feedback("second".into(), oversized, true).percent, Some(100));
    }

    #[test]
    fn completion_and_errors_preserve_the_download_name_and_seeding_state() {
        let mut complete = torrent();
        complete.imported = true;
        let update = torrent_feedback("first".into(), complete.clone(), true);
        assert_eq!(update.phase, BrowserDownloadPhase::Completed);
        assert_eq!(update.name, "Books");
        assert!(update.message.unwrap().contains("seeding continues"));
        let waiting = torrent_feedback("first".into(), complete.clone(), false);
        assert_eq!(waiting.phase, BrowserDownloadPhase::Completed);
        assert!(waiting.message.unwrap().contains("seeding waits"));
        complete.keep_seeding = false;
        assert_eq!(torrent_feedback("first".into(), complete.clone(), true).message.as_deref(), Some("Added to library"));
        complete.error = Some("payload missing".into());
        let failed = torrent_feedback("first".into(), complete, true);
        assert_eq!(failed.phase, BrowserDownloadPhase::Failed(None));
        assert_eq!(failed.message, None);
    }
}

use crate::frontend_contract::FrontendContract;
use crate::home::{AppDataLocation, BackendContext, LibraryEntry, LibrarySyncBinding};
use crate::library::{AuthorFolderBookSummaries, AuthorFolderBooks, AuthorSort, AuthorSummary, Book, BookSummary, Dir, DirContent, DirSummaryContent, FolderSort, Library};
use crate::opds::{built_in_opds_endpoints, OpdsCatalog, OpdsClient, OpdsCredentials, OpdsEndpoint, OpdsEndpointId, OpdsEpubAcquisition, OpdsSearch};
use crate::preferences::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences, TransferPreferencePatch, TransferPreferences};
use crate::scanner::LibraryEvent;
use crate::{BookDownloadStatus, BookId, FolderDownloadStatus, LibraryId, LibraryTransfers, NetworkStatus};
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

const BACKGROUND_SYNC_INTERVAL: Duration = Duration::from_secs(30);
const BACKGROUND_SYNC_DEBOUNCE: Duration = Duration::from_millis(750);
const AUTH_EXPIRY_SAFETY_WINDOW_SECONDS: i64 = 60;

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct AccountStatus {
    signed_in: bool,
    server_url: Option<String>,
    username: Option<String>,
}

impl AccountStatus {
    fn from_session(session: Option<&crate::auth::Session>) -> Self {
        Self { signed_in: session.is_some(), server_url: session.map(|session| session.server_url().as_str().to_owned()), username: session.map(|session| session.username().to_owned()) }
    }

    pub const fn signed_in(&self) -> bool {
        self.signed_in
    }

    pub fn server_url(&self) -> Option<&str> {
        self.server_url.as_deref()
    }

    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySyncState {
    LocalOnly,
    Synced,
    SignInRequired,
    DifferentAccount,
    RemoteMissing,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct LibrarySyncStatus {
    pub library_id: String,
    pub library_name: String,
    pub state: LibrarySyncState,
    pub server_url: Option<String>,
    pub account_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct LibraryCatalogueState {
    pub libraries: Vec<LibraryEntry>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
}

#[derive(Clone, Debug)]
pub struct AppCoreError(String);

impl AppCoreError {
    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn operation(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for AppCoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AppCoreError {}

#[derive(Clone, Debug, Serialize)]
pub struct AppEvent {
    pub library_id: LibraryId,
    pub event: LibraryEvent,
}

struct AppCoreState {
    backend: BackendContext,
    libraries: Mutex<HashMap<LibraryId, Arc<Library>>>,
    active_library_id: Mutex<Option<LibraryId>>,
    catalogue_updates: Mutex<()>,
    preference_updates: Mutex<()>,
    account: Mutex<Option<crate::auth::Session>>,
    opds_client: OpdsClient,
    event_tx: Sender<AppEvent>,
    event_rx: Mutex<Option<Receiver<AppEvent>>>,
    sync_wake_tx: Sender<()>,
    sync_stop: Arc<AtomicBool>,
    sync_thread: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for AppCoreState {
    fn drop(&mut self) {
        self.sync_stop.store(true, Ordering::Release);
        let _ = self.sync_wake_tx.try_send(());
        let thread = self.sync_thread.get_mut().ok().and_then(Option::take);
        if let Some(thread) = thread {
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

#[derive(Clone)]
pub struct AppCore {
    state: Arc<AppCoreState>,
}

fn run_sync_schedule(mut cycle: impl FnMut(), wake_rx: Receiver<()>, stop: &AtomicBool, interval: Duration, debounce: Duration) {
    loop {
        match wake_rx.recv_timeout(interval) {
            Ok(()) => {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                loop {
                    match wake_rx.recv_timeout(debounce) {
                        Ok(()) if stop.load(Ordering::Acquire) => return,
                        Ok(()) => {}
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if stop.load(Ordering::Acquire) {
            return;
        }
        cycle();
    }
}

impl AppCore {
    fn blocking_runtime() -> Result<tokio::runtime::Runtime, AppCoreError> {
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(AppCoreError::operation)
    }

    fn start_background_sync(&self, wake_rx: Receiver<()>) -> Result<(), AppCoreError> {
        let state: Weak<AppCoreState> = Arc::downgrade(&self.state);
        let stop = self.state.sync_stop.clone();
        let thread = std::thread::Builder::new()
            .name(format!("{}-sync", crate::APP_NAME_LOWER))
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name(format!("{}-sync-worker", crate::APP_NAME_LOWER)).enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        log::error!("failed to start background synchronization runtime: {error}");
                        return;
                    }
                };
                run_sync_schedule(
                    || {
                        let Some(state) = state.upgrade() else {
                            return;
                        };
                        let core = AppCore { state };
                        if let Err(error) = runtime.block_on(core.background_sync_cycle()) {
                            log::warn!("background synchronization failed: {error}");
                        }
                    },
                    wake_rx,
                    &stop,
                    BACKGROUND_SYNC_INTERVAL,
                    BACKGROUND_SYNC_DEBOUNCE,
                );
            })
            .map_err(AppCoreError::operation)?;
        *self.lock(&self.state.sync_thread)? = Some(thread);
        Ok(())
    }

    fn request_background_sync(&self) {
        let _ = self.state.sync_wake_tx.try_send(());
    }

    pub fn initialize(location: AppDataLocation) -> Result<Self, AppCoreError> {
        Self::new(BackendContext::initialize(location).map_err(AppCoreError::message)?)
    }

    pub fn new(backend: BackendContext) -> Result<Self, AppCoreError> {
        let (event_tx, event_rx) = unbounded();
        let (sync_wake_tx, sync_wake_rx) = bounded(1);
        let sync_stop = Arc::new(AtomicBool::new(false));
        let api_server = crate::api_server_url();
        let account = crate::auth::Session::load_from(&backend.app_data_dir().join("session.json")).map_err(AppCoreError::operation)?.filter(|session| session.server_url() == &api_server);
        let resume_sync = account.is_some();
        let transfer_preferences = backend.load_transfer_preferences().map_err(AppCoreError::operation)?;
        let opds_client = OpdsClient::new().map_err(AppCoreError::operation)?;
        backend.transfer_runtime().set_allow_background_on_metered(transfer_preferences.allow_background_on_metered());
        let core = Self {
            state: Arc::new(AppCoreState {
                backend,
                libraries: Mutex::new(HashMap::new()),
                active_library_id: Mutex::new(None),
                catalogue_updates: Mutex::new(()),
                preference_updates: Mutex::new(()),
                account: Mutex::new(account),
                opds_client,
                event_tx,
                event_rx: Mutex::new(Some(event_rx)),
                sync_wake_tx,
                sync_stop,
                sync_thread: Mutex::new(None),
            }),
        };
        core.start_background_sync(sync_wake_rx)?;
        if resume_sync {
            core.request_background_sync();
        }
        Ok(core)
    }

    pub fn app_data_dir(&self) -> &Path {
        self.state.backend.app_data_dir()
    }

    pub fn take_events(&self) -> Result<Receiver<AppEvent>, AppCoreError> {
        self.lock(&self.state.event_rx)?.take().ok_or_else(|| AppCoreError::message("application event receiver has already been taken"))
    }

    pub fn libraries(&self) -> Result<Vec<LibraryEntry>, AppCoreError> {
        self.state.backend.try_get_library_entries().map_err(AppCoreError::operation)
    }

    pub fn create_library(&self, name: String, root: Option<PathBuf>) -> Result<LibraryEntry, AppCoreError> {
        let _guard = self.lock(&self.state.catalogue_updates)?;
        self.state.backend.create_library(name, root).map_err(AppCoreError::operation)
    }

    pub fn remove_library(&self, library_id: &str) -> Result<(), AppCoreError> {
        let _guard = self.lock(&self.state.catalogue_updates)?;
        let library_id = LibraryId::parse(library_id).map_err(AppCoreError::operation)?;
        let entry = self.library_entry(&library_id)?;
        self.state.backend.remove_library(&entry.library_path().to_string_lossy()).map_err(AppCoreError::operation)?;
        let mut libraries = self.lock(&self.state.libraries)?;
        let mut active = self.lock(&self.state.active_library_id)?;
        libraries.remove(&library_id);
        if active.as_ref() == Some(&library_id) {
            *active = None;
        }
        Ok(())
    }

    fn open_library(&self, library_id: &str) -> Result<Arc<Library>, AppCoreError> {
        let library_id = LibraryId::parse(library_id).map_err(AppCoreError::operation)?;
        let account = self.current_account()?;
        let mut libraries = self.lock(&self.state.libraries)?;
        if let Some(library) = libraries.get(&library_id).cloned() {
            return Ok(library);
        }
        let entry = self.library_entry(&library_id)?;
        let path = entry.library_path().to_string_lossy();
        let binding = entry.sync_binding().cloned();
        let (library, events) = match binding.as_ref() {
            Some(binding) => Library::try_new_with_sync_binding(&self.state.backend, &path, library_id.as_str(), binding.server_url(), binding.account_id()),
            None => Library::try_new(&self.state.backend, &path, library_id.as_str()),
        }
        .map_err(AppCoreError::operation)?;
        if let Some(binding) = binding.as_ref() {
            if let Some(session) = account.filter(|session| binding.matches_session(session)) {
                library.adopt_session(session).map_err(AppCoreError::message)?;
            }
        }
        let library = Arc::new(library);
        libraries.insert(library_id.clone(), library.clone());
        drop(libraries);
        self.forward_library_events(library_id, events);
        Ok(library)
    }

    pub fn library_handle(&self, library_id: &str) -> Result<Arc<Library>, AppCoreError> {
        self.open_library(library_id)
    }

    pub fn select_library(&self, library_id: &str) -> Result<(), AppCoreError> {
        let library = self.open_library(library_id)?;
        let libraries = self.lock(&self.state.libraries)?;
        if !libraries.contains_key(library.id()) {
            return Err(AppCoreError::message(format!("library was removed while being selected: {library_id}")));
        }
        *self.lock(&self.state.active_library_id)? = Some(library.id().clone());
        Ok(())
    }

    fn active_library(&self) -> Result<Arc<Library>, AppCoreError> {
        let libraries = self.lock(&self.state.libraries)?;
        let library_id = self.lock(&self.state.active_library_id)?.clone().ok_or_else(|| AppCoreError::message("no active library selected"))?;
        libraries.get(&library_id).cloned().ok_or_else(|| AppCoreError::message(format!("active library handle missing: {library_id}")))
    }

    pub fn directory(&self, dir_id: &str) -> Result<DirContent, AppCoreError> {
        self.active_library()?.try_get_dir_contents(dir_id).map_err(AppCoreError::operation)
    }

    pub fn directory_sorted(&self, dir_id: &str, sort: FolderSort) -> Result<DirContent, AppCoreError> {
        self.active_library()?.try_get_dir_contents_sorted(dir_id, sort).map_err(AppCoreError::operation)
    }

    pub fn directory_summary(&self, dir_id: &str) -> Result<DirSummaryContent, AppCoreError> {
        self.active_library()?.try_get_dir_summary(dir_id).map_err(AppCoreError::operation)
    }

    pub fn directory_summary_sorted(&self, dir_id: &str, sort: FolderSort) -> Result<DirSummaryContent, AppCoreError> {
        self.active_library()?.try_get_dir_summary_sorted(dir_id, sort).map_err(AppCoreError::operation)
    }

    pub fn directory_in(&self, library_id: &str, dir_id: &str) -> Result<DirContent, AppCoreError> {
        self.open_library(library_id)?.try_get_dir_contents(dir_id).map_err(AppCoreError::operation)
    }

    pub fn directory_sorted_in(&self, library_id: &str, dir_id: &str, sort: FolderSort) -> Result<DirContent, AppCoreError> {
        self.open_library(library_id)?.try_get_dir_contents_sorted(dir_id, sort).map_err(AppCoreError::operation)
    }

    pub fn create_directory(&self, parent_id: &str, name: &str) -> Result<Dir, AppCoreError> {
        self.active_library()?.create_directory(parent_id, name).map_err(AppCoreError::operation)
    }

    pub fn create_directory_in(&self, library_id: &str, parent_id: &str, name: &str) -> Result<Dir, AppCoreError> {
        self.open_library(library_id)?.create_directory(parent_id, name).map_err(AppCoreError::operation)
    }

    pub fn import_book(&self, parent_id: &str, source_path: &Path, file_name: &str) -> Result<Book, AppCoreError> {
        self.active_library()?.import_book(parent_id, source_path, file_name).map_err(AppCoreError::operation)
    }

    pub fn opds_endpoints(&self) -> Vec<OpdsEndpoint> {
        built_in_opds_endpoints()
    }

    pub async fn opds_endpoint_catalog(&self, endpoint: OpdsEndpointId, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        self.opds_catalog(&endpoint.endpoint().url, credentials).await
    }

    pub fn opds_endpoint_catalog_blocking(&self, endpoint: OpdsEndpointId, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.opds_endpoint_catalog(endpoint, credentials))
    }

    pub async fn opds_catalog(&self, url: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        self.state.opds_client.fetch_catalog(url, credentials).await.map_err(AppCoreError::operation)
    }

    pub fn opds_catalog_blocking(&self, url: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.opds_catalog(url, credentials))
    }

    pub async fn search_opds_catalog(&self, catalog: &OpdsCatalog, query: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        self.state.opds_client.search(catalog, query, credentials).await.map_err(AppCoreError::operation)
    }

    pub async fn search_opds(&self, search: &OpdsSearch, query: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        self.state.opds_client.search_definition(search, query, credentials).await.map_err(AppCoreError::operation)
    }

    pub fn search_opds_catalog_blocking(&self, catalog: &OpdsCatalog, query: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.search_opds_catalog(catalog, query, credentials))
    }

    pub fn search_opds_blocking(&self, search: &OpdsSearch, query: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.search_opds(search, query, credentials))
    }

    pub async fn import_opds_epub(&self, parent_id: &str, acquisition: &OpdsEpubAcquisition, credentials: Option<&OpdsCredentials>) -> Result<Book, AppCoreError> {
        let temporary_directory = self.app_data_dir().join("opds-downloads");
        let download = self.state.opds_client.download_epub(acquisition, credentials, &temporary_directory).await.map_err(AppCoreError::operation)?;
        let library = self.active_library()?;
        let parent_id = parent_id.to_owned();
        tokio::task::spawn_blocking(move || library.import_book(&parent_id, download.path(), download.file_name()).map_err(|error| error.to_string())).await.map_err(AppCoreError::operation)?.map_err(AppCoreError::message)
    }

    pub fn import_opds_epub_blocking(&self, parent_id: &str, acquisition: &OpdsEpubAcquisition, credentials: Option<&OpdsCredentials>) -> Result<Book, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.import_opds_epub(parent_id, acquisition, credentials))
    }

    pub fn recently_read(&self, limit: i32) -> Result<Vec<Book>, AppCoreError> {
        self.active_library()?.try_get_recently_read(limit).map_err(AppCoreError::operation)
    }

    pub fn recently_read_summaries(&self, limit: i32) -> Result<Vec<BookSummary>, AppCoreError> {
        self.active_library()?.try_get_recently_read_summaries(limit).map_err(AppCoreError::operation)
    }

    pub fn recently_read_in(&self, library_id: &str, limit: i32) -> Result<Vec<Book>, AppCoreError> {
        self.open_library(library_id)?.try_get_recently_read(limit).map_err(AppCoreError::operation)
    }

    pub fn recently_added(&self, limit: i32) -> Result<Vec<Book>, AppCoreError> {
        self.active_library()?.try_get_recently_added(limit).map_err(AppCoreError::operation)
    }

    pub fn recently_added_summaries(&self, limit: i32) -> Result<Vec<BookSummary>, AppCoreError> {
        self.active_library()?.try_get_recently_added_summaries(limit).map_err(AppCoreError::operation)
    }

    pub fn recently_added_in(&self, library_id: &str, limit: i32) -> Result<Vec<Book>, AppCoreError> {
        self.open_library(library_id)?.try_get_recently_added(limit).map_err(AppCoreError::operation)
    }

    pub fn search_books(&self, query: &str, limit: usize) -> Result<Vec<Book>, AppCoreError> {
        self.active_library()?.try_search_books(query.trim(), limit).map_err(AppCoreError::operation)
    }

    pub fn search_book_summaries(&self, query: &str, limit: usize) -> Result<Vec<BookSummary>, AppCoreError> {
        self.active_library()?.try_search_book_summaries(query.trim(), limit).map_err(AppCoreError::operation)
    }

    pub fn authors(&self) -> Result<Vec<AuthorSummary>, AppCoreError> {
        self.active_library()?.try_get_authors().map_err(AppCoreError::operation)
    }

    pub fn authors_sorted(&self, sort: AuthorSort) -> Result<Vec<AuthorSummary>, AppCoreError> {
        self.active_library()?.try_get_authors_sorted(sort).map_err(AppCoreError::operation)
    }

    pub fn authors_in(&self, library_id: &str) -> Result<Vec<AuthorSummary>, AppCoreError> {
        self.open_library(library_id)?.try_get_authors().map_err(AppCoreError::operation)
    }

    pub fn authors_sorted_in(&self, library_id: &str, sort: AuthorSort) -> Result<Vec<AuthorSummary>, AppCoreError> {
        self.open_library(library_id)?.try_get_authors_sorted(sort).map_err(AppCoreError::operation)
    }

    pub fn author_folder_books(&self, author_name: &str) -> Result<Vec<AuthorFolderBooks>, AppCoreError> {
        self.active_library()?.try_get_author_folder_books(author_name).map_err(AppCoreError::operation)
    }

    pub fn author_folder_book_summaries(&self, author_name: &str) -> Result<Vec<AuthorFolderBookSummaries>, AppCoreError> {
        self.active_library()?.try_get_author_folder_book_summaries(author_name).map_err(AppCoreError::operation)
    }

    pub fn thumbnail(&self, book_id: &str) -> Result<Option<Vec<u8>>, AppCoreError> {
        self.active_library()?.try_get_thumbnail(Self::book_id(book_id)?).map_err(AppCoreError::operation)
    }

    pub fn thumbnail_in(&self, library_id: &str, book_id: BookId) -> Result<Option<Vec<u8>>, AppCoreError> {
        self.open_library(library_id)?.try_get_thumbnail(book_id).map_err(AppCoreError::operation)
    }

    pub fn author_folder_books_in(&self, library_id: &str, author_name: &str) -> Result<Vec<AuthorFolderBooks>, AppCoreError> {
        self.open_library(library_id)?.try_get_author_folder_books(author_name).map_err(AppCoreError::operation)
    }

    pub fn read_book_bytes(&self, book_id: &str) -> Result<Vec<u8>, AppCoreError> {
        self.active_library()?.read_book_bytes(Self::book_id(book_id)?).map_err(AppCoreError::operation)
    }

    pub fn read_book_bytes_in(&self, library_id: &str, book_id: BookId) -> Result<Vec<u8>, AppCoreError> {
        self.open_library(library_id)?.read_book_bytes(book_id).map_err(AppCoreError::operation)
    }

    pub fn reading_position(&self, book_id: &str) -> Result<Option<String>, AppCoreError> {
        self.active_library()?.get_book_reading_position(Self::book_id(book_id)?).map_err(AppCoreError::operation)
    }

    pub fn reading_position_in(&self, library_id: &str, book_id: BookId) -> Result<Option<String>, AppCoreError> {
        self.open_library(library_id)?.get_book_reading_position(book_id).map_err(AppCoreError::operation)
    }

    pub fn update_reading_position(&self, book_id: &str, cfi: &str) -> Result<(), AppCoreError> {
        self.active_library()?.update_reading_position(Self::book_id(book_id)?, cfi).map_err(AppCoreError::operation)
    }

    pub fn update_reading_position_in(&self, library_id: &str, book_id: BookId, cfi: &str) -> Result<(), AppCoreError> {
        self.open_library(library_id)?.update_reading_position(book_id, cfi).map_err(AppCoreError::operation)
    }

    pub fn update_last_read(&self, book_id: &str) -> Result<(), AppCoreError> {
        self.active_library()?.update_last_read(Self::book_id(book_id)?).map_err(AppCoreError::operation)
    }

    pub fn update_last_read_in(&self, library_id: &str, book_id: BookId) -> Result<(), AppCoreError> {
        self.open_library(library_id)?.update_last_read(book_id).map_err(AppCoreError::operation)
    }

    pub fn mark_book_opened_in(&self, library_id: &str, book_id: BookId) -> Result<(), AppCoreError> {
        self.open_library(library_id)?.mark_book_opened(book_id).map_err(AppCoreError::operation)
    }

    pub fn validate_book_in(&self, library_id: &str, book_id: BookId) -> Result<(), AppCoreError> {
        self.open_library(library_id)?.open_book_reader(book_id).map(|_| ()).map_err(AppCoreError::operation)
    }

    pub fn request_book_download(&self, book_id: &str) -> Result<(), AppCoreError> {
        self.active_library()?.request_book_download(Self::book_id(book_id)?).map_err(AppCoreError::operation)
    }

    pub fn is_book_downloaded(&self, book_id: &str) -> Result<bool, AppCoreError> {
        self.active_library()?.is_book_downloaded(Self::book_id(book_id)?).map_err(AppCoreError::operation)
    }

    pub fn book_download_statuses(&self, book_ids: &[String]) -> Result<Vec<BookDownloadStatus>, AppCoreError> {
        let book_ids = book_ids.iter().map(|book_id| Self::book_id(book_id)).collect::<Result<Vec<_>, _>>()?;
        self.active_library()?.book_download_statuses(&book_ids).map_err(AppCoreError::operation)
    }

    pub fn folder_download_statuses(&self, dir_ids: &[String]) -> Result<Vec<FolderDownloadStatus>, AppCoreError> {
        let dir_ids = dir_ids.iter().map(|dir_id| crate::DirId::parse(dir_id).map_err(AppCoreError::operation)).collect::<Result<Vec<_>, _>>()?;
        self.active_library()?.folder_download_statuses(&dir_ids).map_err(AppCoreError::operation)
    }

    pub fn request_folder_download(&self, dir_id: &str) -> Result<usize, AppCoreError> {
        let dir_id = crate::DirId::parse(dir_id).map_err(AppCoreError::operation)?;
        self.active_library()?.request_folder_download(&dir_id).map_err(AppCoreError::operation)
    }

    pub async fn download_book(&self, book_id: &str) -> Result<crate::DownloadState, AppCoreError> {
        let library = self.active_library()?;
        let book_id = Self::book_id(book_id)?;
        if library.is_book_downloaded(book_id.clone()).map_err(AppCoreError::operation)? {
            return Ok(crate::DownloadState::Downloaded);
        }
        library.request_book_download(book_id.clone()).map_err(AppCoreError::operation)?;
        self.sync_library(&library).await?;
        library.book_download_state(book_id).map_err(AppCoreError::operation)
    }

    pub async fn download_book_in(&self, library_id: &str, book_id: BookId) -> Result<crate::DownloadState, AppCoreError> {
        let library = self.open_library(library_id)?;
        if library.is_book_downloaded(book_id.clone()).map_err(AppCoreError::operation)? {
            return Ok(crate::DownloadState::Downloaded);
        }
        library.request_book_download(book_id.clone()).map_err(AppCoreError::operation)?;
        self.sync_library(&library).await?;
        library.book_download_state(book_id).map_err(AppCoreError::operation)
    }

    pub async fn download_folder(&self, dir_id: &str) -> Result<FolderDownloadStatus, AppCoreError> {
        let library = self.active_library()?;
        let dir_id = crate::DirId::parse(dir_id).map_err(AppCoreError::operation)?;
        let requested = library.request_folder_download(&dir_id).map_err(AppCoreError::operation)?;
        if requested > 0 {
            self.sync_library(&library).await?;
        }
        library.folder_download_status(&dir_id).map_err(AppCoreError::operation)
    }

    pub async fn download_folder_in(&self, library_id: &str, dir_id: crate::DirId) -> Result<FolderDownloadStatus, AppCoreError> {
        let library = self.open_library(library_id)?;
        let requested = library.request_folder_download(&dir_id).map_err(AppCoreError::operation)?;
        if requested > 0 {
            self.sync_library(&library).await?;
        }
        library.folder_download_status(&dir_id).map_err(AppCoreError::operation)
    }

    pub async fn sync_once(&self) -> Result<(), AppCoreError> {
        let library = self.active_library()?;
        library.sync_user_initiated().await.map_err(AppCoreError::operation)
    }

    async fn sync_library(&self, library: &Library) -> Result<(), AppCoreError> {
        library.sync_once().await.map_err(AppCoreError::operation)
    }

    pub fn account_status(&self) -> Result<AccountStatus, AppCoreError> {
        let account = self.current_account()?;
        Ok(AccountStatus::from_session(account.as_ref()))
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<AccountStatus, AppCoreError> {
        let session = crate::auth::login(username, password).await.map_err(AppCoreError::operation)?;
        session.save_to(&self.state.backend.app_data_dir().join("session.json")).map_err(AppCoreError::operation)?;
        *self.lock(&self.state.account)? = Some(session.clone());
        self.lock(&self.state.libraries)?.clear();
        *self.lock(&self.state.active_library_id)? = None;
        self.request_background_sync();
        Ok(AccountStatus::from_session(Some(&session)))
    }

    pub fn login_blocking(&self, username: &str, password: &str) -> Result<AccountStatus, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.login(username, password))
    }

    pub async fn logout(&self) -> Result<AccountStatus, AppCoreError> {
        let session = self.lock(&self.state.account)?.clone();
        if let Some(session) = session.as_ref() {
            if let Err(error) = crate::auth::logout(session).await {
                log::warn!("Failed to notify server about logout: {error}");
            }
        }
        crate::auth::Session::clear_from(&self.state.backend.app_data_dir().join("session.json")).map_err(AppCoreError::operation)?;
        *self.lock(&self.state.account)? = None;
        self.lock(&self.state.libraries)?.clear();
        *self.lock(&self.state.active_library_id)? = None;
        Ok(AccountStatus::from_session(None))
    }

    pub fn logout_blocking(&self) -> Result<AccountStatus, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.logout())
    }

    pub async fn bootstrap_remote_libraries(&self) -> Result<Vec<LibraryEntry>, AppCoreError> {
        let catalogue = self.reconcile_library_catalogue().await?;
        let mut attached = Vec::new();
        for status in catalogue.sync_statuses {
            if status.state == LibrarySyncState::Synced {
                self.open_library(&status.library_id)?.sync_user_initiated().await.map_err(AppCoreError::operation)?;
                if let Some(entry) = self.libraries()?.into_iter().find(|entry| entry.library_id().as_str() == status.library_id) {
                    attached.push(entry);
                }
            }
        }
        Ok(attached)
    }

    pub async fn reconcile_library_catalogue(&self) -> Result<LibraryCatalogueState, AppCoreError> {
        let Some(session) = self.current_account()? else {
            return self.catalogue_state(None, None);
        };
        let remote = match crate::auth::list_libraries(&session).await {
            Ok(remote) => remote,
            Err(error) => {
                if error.is_authentication_failure() {
                    self.clear_account_locally()?;
                }
                return Err(AppCoreError::operation(error));
            }
        };
        let remote_ids = self.attach_remote_catalogue(&session, &remote)?;
        self.catalogue_state(Some(&session), Some(&remote_ids))
    }

    fn attach_remote_catalogue(&self, session: &crate::auth::Session, remote: &[sync_common::api::libraries::LibrarySummary]) -> Result<std::collections::HashSet<LibraryId>, AppCoreError> {
        let _guard = self.lock(&self.state.catalogue_updates)?;
        let existing = self.libraries()?.into_iter().map(|entry| (entry.library_id().clone(), entry)).collect::<HashMap<_, _>>();
        let binding = LibrarySyncBinding::from_session(session);
        let mut remote_ids = std::collections::HashSet::with_capacity(remote.len());
        for library in remote {
            remote_ids.insert(library.library_id.clone());
            if existing.get(&library.library_id).and_then(LibraryEntry::sync_binding).is_some_and(|current| !current.matches_session(&session)) {
                log::warn!("not attaching remote library {} because the local entry belongs to another account", library.library_id);
                continue;
            }
            self.state.backend.attach_remote_library(&library.library_id, &library.library_name, binding.clone()).map_err(AppCoreError::operation)?;
            self.lock(&self.state.libraries)?.remove(&library.library_id);
        }
        Ok(remote_ids)
    }

    pub fn reconcile_library_catalogue_blocking(&self) -> Result<LibraryCatalogueState, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.reconcile_library_catalogue())
    }

    fn catalogue_state(&self, session: Option<&crate::auth::Session>, remote: Option<&std::collections::HashSet<LibraryId>>) -> Result<LibraryCatalogueState, AppCoreError> {
        let libraries = self.libraries()?;
        let sync_statuses = libraries
            .iter()
            .map(|library| {
                let (state, server_url, account_id) = match library.sync_binding() {
                    None => (LibrarySyncState::LocalOnly, None, None),
                    Some(binding) => {
                        let state = match session {
                            None => LibrarySyncState::SignInRequired,
                            Some(session) if !binding.matches_session(session) => LibrarySyncState::DifferentAccount,
                            Some(_) if remote.is_some_and(|remote| remote.contains(library.library_id())) => LibrarySyncState::Synced,
                            Some(_) => LibrarySyncState::RemoteMissing,
                        };
                        (state, Some(binding.server_url().to_owned()), Some(binding.account_id().to_owned()))
                    }
                };
                LibrarySyncStatus { library_id: library.library_id().as_str().to_owned(), library_name: library.library_name().to_owned(), state, server_url, account_id }
            })
            .collect();
        Ok(LibraryCatalogueState { libraries, sync_statuses })
    }

    pub async fn sync_all_libraries(&self) -> Result<(), AppCoreError> {
        let session = self.require_account()?;
        for entry in self.libraries()? {
            if entry.sync_binding().is_some_and(|binding| binding.matches_session(&session)) {
                let library = self.open_library(entry.library_id().as_str())?;
                self.publish_library(&library).await?;
                library.sync_user_initiated().await.map_err(AppCoreError::operation)?;
            }
        }
        Ok(())
    }

    pub async fn upload_and_sync_library(&self, library_id: &str) -> Result<(), AppCoreError> {
        let session = self.require_account()?;
        let library = self.prepare_library_sync(library_id, &session)?;
        self.publish_library(&library).await?;
        library.sync_user_initiated().await.map_err(AppCoreError::operation)
    }

    fn prepare_library_sync(&self, library_id: &str, session: &crate::auth::Session) -> Result<Arc<Library>, AppCoreError> {
        let library_id = LibraryId::parse(library_id).map_err(AppCoreError::operation)?;
        let entry = self.library_entry(&library_id)?;
        let binding = LibrarySyncBinding::from_session(&session);
        if let Some(existing) = entry.sync_binding() {
            if existing != &binding {
                return Err(AppCoreError::message("library sync is bound to a different server or account; disable sync before changing the binding"));
            }
        } else {
            let library = self.open_library(library_id.as_str())?;
            if library.bound_user_id().map_err(AppCoreError::message)?.is_some_and(|bound| bound != session.user_id()) {
                return Err(AppCoreError::message("library history is bound to a different account and requires an explicit migration"));
            }
            let _guard = self.lock(&self.state.catalogue_updates)?;
            self.state.backend.set_library_sync_binding(&library_id, binding).map_err(AppCoreError::operation)?;
            self.lock(&self.state.libraries)?.remove(&library_id);
        }
        let library = self.open_library(library_id.as_str())?;
        library.adopt_session(session.clone()).map_err(AppCoreError::message)?;
        Ok(library)
    }

    pub fn upload_and_sync_library_blocking(&self, library_id: &str) -> Result<(), AppCoreError> {
        Self::blocking_runtime()?.block_on(self.upload_and_sync_library(library_id))
    }

    pub async fn disable_library_sync(&self, library_id: &str) -> Result<(), AppCoreError> {
        let library_id = LibraryId::parse(library_id).map_err(AppCoreError::operation)?;
        let entry = self.library_entry(&library_id)?;
        if entry.sync_binding().is_none() {
            return Ok(());
        }
        let library = self.open_library(library_id.as_str())?;
        library.suspend_sync().await.map_err(AppCoreError::message)?;
        let _guard = self.lock(&self.state.catalogue_updates)?;
        self.state.backend.clear_library_sync_binding(&library_id).map_err(AppCoreError::operation)?;
        self.lock(&self.state.libraries)?.remove(&library_id);
        self.open_library(library_id.as_str())?;
        Ok(())
    }

    pub fn disable_library_sync_blocking(&self, library_id: &str) -> Result<(), AppCoreError> {
        Self::blocking_runtime()?.block_on(self.disable_library_sync(library_id))
    }

    pub async fn library_sync_statuses(&self) -> Result<Vec<LibrarySyncStatus>, AppCoreError> {
        let session = self.current_account()?;
        let remote = match session.as_ref() {
            Some(session) => match crate::auth::list_libraries(session).await {
                Ok(remote) => Some(remote.into_iter().map(|library| library.library_id).collect::<std::collections::HashSet<_>>()),
                Err(error) => {
                    if error.is_authentication_failure() {
                        self.clear_account_locally()?;
                    }
                    return Err(AppCoreError::operation(error));
                }
            },
            None => None,
        };
        Ok(self.catalogue_state(session.as_ref(), remote.as_ref())?.sync_statuses)
    }

    pub fn library_sync_statuses_blocking(&self) -> Result<Vec<LibrarySyncStatus>, AppCoreError> {
        Self::blocking_runtime()?.block_on(self.library_sync_statuses())
    }

    pub fn transfer_statuses(&self) -> Result<Vec<LibraryTransfers>, AppCoreError> {
        let mut groups = Vec::new();
        for entry in self.libraries()? {
            let library = self.open_library(entry.library_id().as_str())?;
            groups.push(LibraryTransfers { library_id: entry.library_id().as_str().to_owned(), library_name: entry.library_name().to_owned(), transfers: library.transfer_statuses().map_err(AppCoreError::operation)? });
        }
        Ok(groups)
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.state.backend.transfer_runtime().network_status()
    }

    pub async fn update_network_status(&self, status: NetworkStatus) -> Result<NetworkStatus, AppCoreError> {
        let became_allowed = self.state.backend.transfer_runtime().set_network_status(status);
        if became_allowed {
            self.resume_transfers().await?;
            self.request_background_sync();
        }
        Ok(self.network_status())
    }

    pub fn transfer_preferences(&self) -> Result<TransferPreferences, AppCoreError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_transfer_preferences().map_err(AppCoreError::operation)
    }

    pub async fn update_transfer_preference(&self, patch: TransferPreferencePatch) -> Result<TransferPreferences, AppCoreError> {
        let preferences = {
            let _guard = self.lock(&self.state.preference_updates)?;
            self.state.backend.apply_transfer_preference_patch(patch).map_err(AppCoreError::operation)?
        };
        let became_allowed = self.state.backend.transfer_runtime().set_allow_background_on_metered(preferences.allow_background_on_metered());
        if became_allowed {
            self.resume_transfers().await?;
        }
        Ok(preferences)
    }

    async fn resume_transfers(&self) -> Result<(), AppCoreError> {
        let session = self.lock(&self.state.account)?.clone();
        for entry in self.libraries()? {
            if entry.sync_binding().is_some_and(|binding| session.as_ref().is_some_and(|session| binding.matches_session(session))) {
                self.open_library(entry.library_id().as_str())?.resume_transfers().await.map_err(AppCoreError::operation)?;
            }
        }
        Ok(())
    }

    async fn background_sync_cycle(&self) -> Result<(), AppCoreError> {
        if !self.network_status().connected || self.current_account()?.is_none() {
            return Ok(());
        }
        let catalogue = self.reconcile_library_catalogue().await?;
        for status in catalogue.sync_statuses {
            if !matches!(status.state, LibrarySyncState::Synced | LibrarySyncState::RemoteMissing) {
                continue;
            }
            let library = match self.open_library(&status.library_id) {
                Ok(library) => library,
                Err(error) => {
                    log::warn!("failed to open {} for background synchronization: {error}", status.library_name);
                    continue;
                }
            };
            if let Err(error) = self.publish_library(&library).await {
                log::warn!("failed to publish {} during background synchronization: {error}", status.library_name);
                if self.current_account()?.is_none() {
                    break;
                }
                continue;
            }
            if let Err(error) = library.sync_once().await {
                log::warn!("failed to synchronize {} in the background: {error}", status.library_name);
                if !library.is_signed_in() {
                    self.clear_account_locally()?;
                    break;
                }
            }
        }
        Ok(())
    }

    async fn publish_library(&self, library: &Library) -> Result<(), AppCoreError> {
        let session = self.require_account()?;
        let entry = self.library_entry(library.id())?;
        let binding = entry.sync_binding().ok_or_else(|| AppCoreError::message("library sync is not enabled"))?;
        if !binding.matches_session(&session) {
            return Err(AppCoreError::message("library sync belongs to a different server or account"));
        }
        if let Err(error) = crate::auth::upsert_library(&session, library.id().clone(), entry.library_name().to_owned()).await {
            if error.is_authentication_failure() {
                self.clear_account_locally()?;
            }
            return Err(AppCoreError::operation(error));
        }
        Ok(())
    }

    fn require_account(&self) -> Result<crate::auth::Session, AppCoreError> {
        self.current_account()?.ok_or_else(|| AppCoreError::message("not signed in"))
    }

    fn current_account(&self) -> Result<Option<crate::auth::Session>, AppCoreError> {
        let account = self.lock(&self.state.account)?.clone();
        if account.as_ref().is_some_and(|session| session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS)) {
            self.clear_account_locally()?;
            return Ok(None);
        }
        Ok(account)
    }

    fn clear_account_locally(&self) -> Result<(), AppCoreError> {
        crate::auth::Session::clear_from(&self.state.backend.app_data_dir().join("session.json")).map_err(AppCoreError::operation)?;
        *self.lock(&self.state.account)? = None;
        self.lock(&self.state.libraries)?.clear();
        *self.lock(&self.state.active_library_id)? = None;
        Ok(())
    }

    pub fn reader_preferences(&self) -> Result<ReaderPreferences, AppCoreError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_reader_preferences().map_err(AppCoreError::operation)
    }

    pub fn browsing_preferences(&self) -> Result<BrowsingPreferences, AppCoreError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_browsing_preferences().map_err(AppCoreError::operation)
    }

    pub fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, AppCoreError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.apply_browsing_preference_patch(patch).map_err(AppCoreError::operation)
    }

    pub fn has_persisted_reader_preferences(&self) -> bool {
        self.state.backend.reader_preferences_path().exists()
    }

    pub fn update_reader_preference(&self, patch: ReaderPreferencePatch) -> Result<ReaderPreferences, AppCoreError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.apply_reader_preference_patch(patch).map_err(AppCoreError::operation)
    }

    pub fn frontend_contract(&self) -> FrontendContract {
        FrontendContract::default()
    }

    pub fn adopt_session(&self, session: crate::auth::Session) -> Result<(), AppCoreError> {
        if session.server_url() != &crate::api_server_url() {
            return Err(AppCoreError::message(format!("application sessions must belong to {}", crate::API_SERVER_ORIGIN)));
        }
        session.save_to(&self.state.backend.app_data_dir().join("session.json")).map_err(AppCoreError::operation)?;
        *self.lock(&self.state.account)? = Some(session);
        self.lock(&self.state.libraries)?.clear();
        *self.lock(&self.state.active_library_id)? = None;
        Ok(())
    }

    pub fn clear_library_sessions(&self) -> Result<(), AppCoreError> {
        crate::auth::Session::clear_from(&self.state.backend.app_data_dir().join("session.json")).map_err(AppCoreError::operation)?;
        *self.lock(&self.state.account)? = None;
        self.lock(&self.state.libraries)?.clear();
        *self.lock(&self.state.active_library_id)? = None;
        Ok(())
    }

    fn library_entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, AppCoreError> {
        self.libraries()?.into_iter().find(|entry| entry.library_id() == library_id).ok_or_else(|| AppCoreError::message(format!("library not found: {library_id}")))
    }

    fn forward_library_events(&self, library_id: LibraryId, events: Receiver<LibraryEvent>) {
        let event_tx = self.state.event_tx.clone();
        let sync_wake_tx = self.state.sync_wake_tx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = events.recv() {
                let _ = sync_wake_tx.try_send(());
                if event_tx.send(AppEvent { library_id: library_id.clone(), event }).is_err() {
                    break;
                }
            }
        });
    }

    fn book_id(value: &str) -> Result<BookId, AppCoreError> {
        BookId::parse(value).map_err(|_| AppCoreError::message(format!("invalid book id: {value}")))
    }

    fn lock<'a, T>(&self, mutex: &'a Mutex<T>) -> Result<MutexGuard<'a, T>, AppCoreError> {
        mutex.lock().map_err(|_| AppCoreError::message("application state lock is poisoned"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::{ApplicationTheme, BrowsingPreferencePatch, ReaderPreferenceKey, ReaderTheme, UiFontFamily};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;

    #[test]
    fn background_sync_schedule_debounces_wake_bursts() {
        let (wake_tx, wake_rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let cycles = Arc::new(AtomicUsize::new(0));
        let worker_stop = stop.clone();
        let worker_cycles = cycles.clone();
        let worker = std::thread::spawn(move || {
            run_sync_schedule(
                || {
                    worker_cycles.fetch_add(1, Ordering::AcqRel);
                },
                wake_rx,
                &worker_stop,
                Duration::from_secs(60),
                Duration::from_millis(20),
            );
        });
        wake_tx.try_send(()).unwrap();
        let _ = wake_tx.try_send(());
        for _ in 0..100 {
            if cycles.load(Ordering::Acquire) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(cycles.load(Ordering::Acquire), 1);
        stop.store(true, Ordering::Release);
        let _ = wake_tx.try_send(());
        worker.join().unwrap();
    }

    #[test]
    fn app_core_owns_library_selection_handles_and_feature_operations() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Test".to_string(), Some(root.path().join("library"))).unwrap();
        core.select_library(entry.library_id().as_str()).unwrap();
        let first = core.active_library().unwrap();
        let second = core.open_library(entry.library_id().as_str()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let created = core.create_directory("__root__", "Shelf").unwrap();
        assert_eq!(core.directory("__root__").unwrap().dirs[0].id(), created.id());
    }

    #[test]
    fn app_core_exposes_canonical_preferences_and_capabilities() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().to_path_buf())).unwrap();
        let preferences = core.update_reader_preference(ReaderPreferencePatch::Theme(ReaderTheme::Dark)).unwrap();
        assert_eq!(preferences.theme(), ReaderTheme::Dark);
        let browsing = core.update_browsing_preference(BrowsingPreferencePatch::Theme(ApplicationTheme::LibrarySerif)).unwrap();
        assert_eq!(browsing.theme(), ApplicationTheme::LibrarySerif);
        let browsing = core.update_browsing_preference(BrowsingPreferencePatch::InterfaceFont(UiFontFamily::Serif)).unwrap();
        assert_eq!(browsing.interface_font(), UiFontFamily::Serif);
        assert_eq!(browsing.heading_font(), UiFontFamily::Serif);
        let contract = core.frontend_contract().reader_preferences;
        assert!(contract.foliate_capabilities.supports(ReaderPreferenceKey::Theme));
        assert!(!contract.native_capabilities.supports(ReaderPreferenceKey::Theme));
    }

    #[test]
    fn concurrent_first_open_reuses_one_library_handle() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library_id = core.create_library("Test".to_string(), Some(root.path().join("library"))).unwrap().library_id().as_str().to_owned();
        let barrier = Arc::new(Barrier::new(8));
        let threads = (0..8)
            .map(|_| {
                let core = core.clone();
                let library_id = library_id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    core.open_library(&library_id).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let libraries = threads.into_iter().map(|thread| thread.join().unwrap()).collect::<Vec<_>>();
        assert!(libraries.windows(2).all(|pair| Arc::ptr_eq(&pair[0], &pair[1])));
    }

    #[test]
    fn login_does_not_bind_a_local_only_library() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Local".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(session).unwrap();

        let library = core.open_library(entry.library_id().as_str()).unwrap();
        assert!(!library.is_signed_in());
        assert!(library.bound_user_id().unwrap().is_none());
        assert!(core.library_entry(entry.library_id()).unwrap().sync_binding().is_none());
    }

    #[test]
    fn expired_account_is_cleared_without_changing_library_bindings() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Linked".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = crate::auth::Session::try_new_with_expiry(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43), Some(1)).unwrap();
        core.state.backend.set_library_sync_binding(entry.library_id(), LibrarySyncBinding::from_session(&session)).unwrap();
        core.adopt_session(session).unwrap();

        assert!(!core.account_status().unwrap().signed_in());
        assert!(core.library_entry(entry.library_id()).unwrap().sync_binding().is_some());
        assert!(!core.app_data_dir().join("session.json").exists());
    }

    #[test]
    fn remote_catalogue_marks_matching_local_and_creates_remote_only_entries() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let local = core.create_library("Local name".to_owned(), Some(root.path().join("library"))).unwrap();
        let remote_only_id = LibraryId::parse("remote-only").unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        let remote = vec![
            sync_common::api::libraries::LibrarySummary { library_id: local.library_id().clone(), library_name: "Server name".to_owned() },
            sync_common::api::libraries::LibrarySummary { library_id: remote_only_id.clone(), library_name: "Remote only".to_owned() },
        ];

        let remote_ids = core.attach_remote_catalogue(&session, &remote).unwrap();
        let state = core.catalogue_state(Some(&session), Some(&remote_ids)).unwrap();
        assert_eq!(state.libraries.len(), 2);
        assert!(state.libraries.iter().all(|library| library.sync_binding().is_some()));
        assert!(state.sync_statuses.iter().all(|status| status.state == LibrarySyncState::Synced));
        assert_eq!(core.library_entry(local.library_id()).unwrap().library_name(), "Server name");
        assert_eq!(core.library_entry(&remote_only_id).unwrap().library_name(), "Remote only");
    }

    #[test]
    fn selecting_a_local_library_persists_its_api_sync_binding_before_upload() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Local".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();

        let library = core.prepare_library_sync(entry.library_id().as_str(), &session).unwrap();
        assert!(library.is_signed_in());
        let binding = core.library_entry(entry.library_id()).unwrap().sync_binding().cloned().unwrap();
        assert_eq!(binding.server_url(), "https://api.bokheim.se/");
        assert_eq!(binding.account_id(), "user-1");
    }

    #[tokio::test]
    async fn disabling_sync_preserves_library_and_changes_catalogue_mode() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Synced".to_owned(), Some(root.path().join("library"))).unwrap();
        let binding = LibrarySyncBinding::validated(crate::API_SERVER_ORIGIN, "user-1").unwrap();
        core.state.backend.set_library_sync_binding(entry.library_id(), binding).unwrap();

        let statuses = core.library_sync_statuses().await.unwrap();
        assert_eq!(statuses[0].state, LibrarySyncState::SignInRequired);
        core.disable_library_sync(entry.library_id().as_str()).await.unwrap();
        assert!(core.library_entry(entry.library_id()).unwrap().sync_binding().is_none());
        assert!(core.open_library(entry.library_id().as_str()).is_ok());
    }

    #[test]
    fn concurrent_preference_patches_cannot_lose_each_other() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let threads = [ReaderPreferencePatch::Theme(ReaderTheme::Dark), ReaderPreferencePatch::Zoom(1.7)]
            .into_iter()
            .map(|patch| {
                let core = core.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    core.update_reader_preference(patch).unwrap();
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        let preferences = core.reader_preferences().unwrap();
        assert_eq!(preferences.theme(), ReaderTheme::Dark);
        assert_eq!(preferences.zoom(), 1.7);
    }

    #[test]
    fn a_library_linked_to_another_account_remains_locally_browseable() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library_id = core.create_library("Test".to_string(), None).unwrap().library_id().clone();
        core.select_library(library_id.as_str()).unwrap();

        let first = crate::auth::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-a".to_string(), "reader-a".to_string(), "a".repeat(43)).unwrap();
        core.adopt_session(first).unwrap();
        core.state.backend.set_library_sync_binding(&library_id, LibrarySyncBinding::from_session(core.state.account.lock().unwrap().as_ref().unwrap())).unwrap();

        let second = crate::auth::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-b".to_string(), "reader-b".to_string(), "b".repeat(43)).unwrap();
        second.save_to(&core.app_data_dir().join("session.json")).unwrap();
        *core.state.account.lock().unwrap() = Some(second);
        core.state.libraries.lock().unwrap().clear();
        *core.state.active_library_id.lock().unwrap() = None;

        let library = core.open_library(library_id.as_str()).unwrap();
        assert!(!library.is_signed_in());
        assert_eq!(library.bound_user_id().unwrap(), None);
    }
}

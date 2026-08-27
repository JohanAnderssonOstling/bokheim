use super::data::{AppDataLocation, BackendContext, LibraryEntry};
use super::executor::{BackendExecutor, BackendTask};
use super::preferences::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences};
use crate::catalog::{CatalogImportRequest, CatalogPage, CatalogPublicationIdentity, CatalogSearch, CatalogSearchQuery, CatalogSource, CatalogSourceId, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse};
use crate::library::database as db;
use crate::platform::library_events::{LibraryEvent, LibraryEventReceiver};
use crate::sync::{AssetSync, ReadingPositionSync, SyncError, SyncPolicy};
use crate::{ContentHash, FolderDownloadStatus, LibraryId, LibraryTransfers, NetworkStatus};
use book_metadata::librarything::LibraryThingClient;
use catalog_client::CatalogClient;
use crossbeam_channel::{unbounded, Receiver, Sender};
use library_model::BookImportDestination;
use metadata_client::MetadataClient;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;
use taxonomy_client::TaxonomyClient;
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
type AuthFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, sync_client::AuthError>> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
type AuthFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, sync_client::AuthError>> + 'a>>;

use crate::library::LibraryBackend;
use library_model::LibraryUpdate;

#[derive(Default)]
struct AppEventSubscribers {
    all: Option<Sender<AppEvent>>,
    libraries: Vec<(LibraryId, async_channel::Sender<LibraryUpdate>)>,
    library_contents: Vec<(LibraryId, async_channel::Sender<()>)>,
    library_lists: Vec<async_channel::Sender<()>>,
    sync: Vec<async_channel::Sender<()>>,
    transfers: Vec<async_channel::Sender<()>>,
    book_removals: Vec<async_channel::Sender<(LibraryId, ContentHash)>>,
}

#[derive(Clone, Default)]
struct AppEventBus(Arc<Mutex<AppEventSubscribers>>);

impl AppEventBus {
    fn send(&self, app_event: AppEvent) {
        let mut subscribers = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if subscribers.all.as_ref().is_some_and(|sender| sender.send(app_event.clone()).is_err()) {
            subscribers.all = None;
        }

        if matches!(&app_event.event, LibraryEvent::TransferStatusChanged | LibraryEvent::ScanStarted | LibraryEvent::ScanComplete) {
            subscribers.transfers.retain(|sender| sender.try_send(()).is_ok());
        }

        match &app_event.event {
            LibraryEvent::ThumbnailGenerated { content_hash } => {
                send_library_update(&mut subscribers.libraries, &app_event.library_id, LibraryUpdate::Covers(vec![content_hash.clone()]));
            }
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, .. } => {
                send_library_update(&mut subscribers.libraries, &app_event.library_id, LibraryUpdate::Covers(content_hashes.clone()));
            }
            LibraryEvent::DownloadStatusChanged { .. } => {}
            LibraryEvent::TransferStatusChanged | LibraryEvent::ScanStarted => {}
            LibraryEvent::LibraryListChanged => {
                subscribers.library_lists.retain(|sender| sender.try_send(()).is_ok());
            }
            LibraryEvent::BookRemoved { content_hash, .. } => {
                subscribers.book_removals.retain(|sender| sender.try_send((app_event.library_id.clone(), content_hash.clone())).is_ok());
                send_library_update(&mut subscribers.libraries, &app_event.library_id, LibraryUpdate::Contents);
                send_library_contents_changed(&mut subscribers.library_contents, &app_event.library_id);
            }
            LibraryEvent::BookAdded { .. }
            | LibraryEvent::DirectoryAdded { .. }
            | LibraryEvent::DirectoryRemoved { .. }
            | LibraryEvent::SubjectEnrichmentBatchProcessed { .. }
            | LibraryEvent::FolderDownloadStatusChanged { .. }
            | LibraryEvent::LibraryChanged
            | LibraryEvent::ScanComplete => {
                send_library_update(&mut subscribers.libraries, &app_event.library_id, LibraryUpdate::Contents);
                send_library_contents_changed(&mut subscribers.library_contents, &app_event.library_id);
            }
        }
    }
}

fn send_library_update(subscribers: &mut Vec<(LibraryId, async_channel::Sender<LibraryUpdate>)>, library_id: &LibraryId, update: LibraryUpdate) {
    subscribers.retain(|(subscribed_id, sender)| subscribed_id != library_id || sender.try_send(update.clone()).is_ok());
}

fn send_library_contents_changed(subscribers: &mut Vec<(LibraryId, async_channel::Sender<()>)>, library_id: &LibraryId) {
    subscribers.retain(|(subscribed_id, sender)| {
        if subscribed_id != library_id {
            return true;
        }
        match sender.try_send(()) {
            Ok(()) | Err(async_channel::TrySendError::Full(())) => true,
            Err(async_channel::TrySendError::Closed(())) => false,
        }
    });
}

const BACKGROUND_SYNC_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const BACKGROUND_RECONCILE_INTERVAL: Duration = Duration::from_secs(10 * 60);
const BACKGROUND_SYNC_DEBOUNCE: Duration = Duration::from_millis(750);
const READING_POSITION_SYNC_DEBOUNCE: Duration = Duration::from_secs(30);
const AUTH_EXPIRY_SAFETY_WINDOW_SECONDS: i64 = 60;
const AUTH_REFRESH_WINDOW_SECONDS: i64 = 15 * 60;

fn account_status_from_session(session: Option<&sync_client::Session>) -> AccountStatus {
    session.map(|session| AccountStatus::authenticated(session.server_url().as_str(), session.email())).unwrap_or_else(AccountStatus::signed_out)
}

fn session_is_usable(session: &sync_client::Session, server: &crate::ServerUrl) -> bool {
    session.server_url() == server && if session.refresh_token().is_some() { !session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { !session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) }
}

use super::account_models::{AccountStatus, AccountStorageUsage, LibrarySyncState, LibrarySyncStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage};

#[cfg(not(target_arch = "wasm32"))]
fn regular_files_apparent_size(paths: impl IntoIterator<Item = PathBuf>) -> Result<u64, BackendError> {
    let mut bytes = 0_u64;
    for path in paths {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => bytes = bytes.saturating_add(metadata.len()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(BackendError::operation(error)),
        }
    }
    Ok(bytes)
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AccountSessionSummary {
    pub session_id: String,
    pub created_at: f64,
    pub expires_at: f64,
    pub current: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct LibraryListState {
    pub libraries: Vec<LibraryEntry>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogPageView {
    pub page: CatalogPage,
    pub owned_publications: HashSet<CatalogPublicationKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub struct CatalogPublicationKey {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
}

impl CatalogPublicationKey {
    pub fn new(source_id: CatalogSourceId, publication: CatalogPublicationIdentity) -> Self {
        Self { source_id, publication }
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct CatalogSubjectBrowseView {
    pub response: CatalogSubjectBrowseResponse,
    pub owned_publications: HashSet<CatalogPublicationKey>,
}

#[derive(Clone, Debug)]
pub struct BackendError(String);

impl BackendError {
    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn operation(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

#[derive(Clone, Debug, Serialize)]
pub struct AppEvent {
    pub library_id: LibraryId,
    pub event: LibraryEvent,
}

pub(crate) struct AppState {
    executor: BackendExecutor,
    backend: BackendContext,
    active_library: Mutex<Option<LibraryBackend>>,
    open_libraries: Mutex<HashMap<LibraryId, LibraryBackend>>,
    #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
    library_scanning_enabled: AtomicBool,
    library_list_updates: Mutex<()>,
    preference_updates: Mutex<()>,
    browser_session_updates: Mutex<()>,
    account: Mutex<Option<sync_client::Session>>,
    account_refresh: tokio::sync::Mutex<()>,
    catalogs: CatalogClient,
    metadata: MetadataClient,
    taxonomy: TaxonomyClient,
    taxonomy_updates: tokio::sync::Mutex<()>,
    librarything: Option<LibraryThingClient>,
    events: AppEventBus,
    events_taken: AtomicBool,
    sync_wake_tx: async_channel::Sender<()>,
    pending_sync: Arc<Mutex<PendingSyncWork>>,
    sync_stop: Arc<AtomicBool>,
    sync_task: Mutex<Option<BackendTask<()>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LibrarySyncWork {
    assets: bool,
}

#[derive(Clone, Debug, Default)]
struct PendingSyncWork {
    full_reconcile: bool,
    library_list: bool,
    libraries: HashMap<LibraryId, LibrarySyncWork>,
    reading_positions: HashMap<LibraryId, Instant>,
}

impl PendingSyncWork {
    fn library(&mut self, library_id: LibraryId, assets: bool) {
        self.libraries.entry(library_id).or_default().assets |= assets;
    }

    fn reading_position(&mut self, library_id: LibraryId, delay: Duration) {
        let deadline = Instant::now() + delay;
        self.reading_positions.entry(library_id).and_modify(|current| *current = (*current).max(deadline)).or_insert(deadline);
    }

    fn take_ready(&mut self, now: Instant) -> PendingSyncWork {
        let mut ready = PendingSyncWork { full_reconcile: std::mem::take(&mut self.full_reconcile), library_list: std::mem::take(&mut self.library_list), libraries: std::mem::take(&mut self.libraries), reading_positions: HashMap::new() };
        self.reading_positions.retain(|library_id, deadline| {
            if *deadline > now {
                return true;
            }
            ready.library(library_id.clone(), false);
            false
        });
        ready
    }

    fn next_reading_deadline(&self) -> Option<Instant> {
        self.reading_positions.values().copied().min()
    }

    fn merge(&mut self, other: PendingSyncWork) {
        self.full_reconcile |= other.full_reconcile;
        self.library_list |= other.library_list;
        for (library_id, work) in other.libraries {
            self.library(library_id, work.assets);
        }
        for (library_id, deadline) in other.reading_positions {
            self.reading_positions.entry(library_id).and_modify(|current| *current = (*current).max(deadline)).or_insert(deadline);
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.sync_stop.store(true, Ordering::Release);
        let _ = self.sync_wake_tx.try_send(());
        self.sync_task.get_mut().ok().and_then(Option::take);
    }
}

#[derive(Clone)]
pub struct AppBackend {
    state: Arc<AppState>,
}

async fn run_sync_schedule<C, F>(
    mut cycle: C, mut next_reading_deadline: impl FnMut() -> Option<Instant>, wake_rx: async_channel::Receiver<()>, stop: Arc<AtomicBool>, retry_interval: Duration, reconcile_interval: Duration, debounce: Duration,
) where
    C: FnMut(bool) -> F,
    F: Future<Output = ()>,
{
    let mut last_reconcile = Instant::now();
    loop {
        let until_reconcile = reconcile_interval.saturating_sub(last_reconcile.elapsed());
        let until_reading = next_reading_deadline().map(|deadline| deadline.saturating_duration_since(Instant::now())).unwrap_or(retry_interval);
        match tokio::time::timeout(retry_interval.min(until_reconcile).min(until_reading), wake_rx.recv()).await {
            Ok(Ok(())) => {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                loop {
                    match tokio::time::timeout(debounce, wake_rx.recv()).await {
                        Ok(Ok(())) if stop.load(Ordering::Acquire) => return,
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => return,
                        Err(_) => break,
                    }
                }
            }
            Ok(Err(_)) => return,
            Err(_) => {}
        }
        if stop.load(Ordering::Acquire) {
            return;
        }
        let periodic = last_reconcile.elapsed() >= reconcile_interval;
        if periodic {
            last_reconcile = Instant::now();
        }
        cycle(periodic).await;
    }
}

impl AppBackend {
    pub(crate) fn downgrade(&self) -> Weak<AppState> {
        Arc::downgrade(&self.state)
    }

    pub(crate) fn from_weak(state: &Weak<AppState>) -> Result<Self, BackendError> {
        state.upgrade().map(|state| Self { state }).ok_or_else(|| BackendError::message("application backend has been closed"))
    }

    fn start_background_sync(&self, wake_rx: async_channel::Receiver<()>) -> Result<(), BackendError> {
        let state: Weak<AppState> = Arc::downgrade(&self.state);
        let stop = self.state.sync_stop.clone();
        let pending_sync = self.state.pending_sync.clone();
        let task = self.state.executor.spawn(run_sync_schedule(
            move |periodic| {
                let state = state.clone();
                async move {
                    let Some(state) = state.upgrade() else {
                        return;
                    };
                    let core = AppBackend { state };
                    if let Err(error) = core.background_sync_cycle(periodic).await {
                        log::warn!("background synchronization failed: {error}");
                    }
                }
            },
            move || pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).next_reading_deadline(),
            wake_rx,
            stop,
            BACKGROUND_SYNC_RETRY_INTERVAL,
            BACKGROUND_RECONCILE_INTERVAL,
            BACKGROUND_SYNC_DEBOUNCE,
        ));
        *self.lock(&self.state.sync_task)? = Some(task);
        Ok(())
    }

    fn request_background_sync(&self) {
        self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).full_reconcile = true;
        let _ = self.state.sync_wake_tx.try_send(());
    }

    pub(crate) fn request_library_sync(&self, library_id: LibraryId, assets: bool) {
        self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library(library_id, assets);
        let _ = self.state.sync_wake_tx.try_send(());
    }

    pub(crate) fn request_reading_position_sync(&self, library_id: LibraryId) {
        self.request_reading_position_sync_after(library_id, READING_POSITION_SYNC_DEBOUNCE);
    }

    fn request_reading_position_sync_after(&self, library_id: LibraryId, delay: Duration) {
        self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).reading_position(library_id, delay);
        let _ = self.state.sync_wake_tx.try_send(());
    }

    pub fn initialize(location: AppDataLocation) -> Result<Self, BackendError> {
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        Self::initialize_with_executor(location, executor)
    }

    fn initialize_with_executor(location: AppDataLocation, executor: BackendExecutor) -> Result<Self, BackendError> {
        Self::new_with_executor(
            BackendContext::initialize(location).map_err(BackendError::message)?,
            executor,
            #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
            true,
        )
    }

    pub fn new(backend: BackendContext) -> Result<Self, BackendError> {
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        Self::new_with_executor(
            backend,
            executor,
            #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
            true,
        )
    }

    /// Constructs a desktop backend whose library scanners remain parked until
    /// the graphical host has rendered its first frame.
    #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
    pub fn new_deferred_scanning(backend: BackendContext) -> Result<Self, BackendError> {
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        Self::new_with_executor(backend, executor, false)
    }

    fn new_with_executor(backend: BackendContext, executor: BackendExecutor, #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))] library_scanning_enabled: bool) -> Result<Self, BackendError> {
        let events = AppEventBus::default();
        let (sync_wake_tx, sync_wake_rx) = async_channel::bounded(1);
        let pending_sync = Arc::new(Mutex::new(PendingSyncWork::default()));
        let sync_stop = Arc::new(AtomicBool::new(false));
        let api_server = crate::api_server_url();
        let account = backend.load_account_session().map_err(BackendError::operation)?.filter(|session| session.server_url() == &api_server);
        let resume_sync = account.is_some();
        let browsing_preferences = backend.load_browsing_preferences().map_err(BackendError::operation)?;
        let catalogs = CatalogClient::with_persistent_cache(crate::catalog_server_url().as_str(), backend.app_data_dir()).map_err(BackendError::operation)?;
        let metadata = MetadataClient::new(crate::metadata_server_url().as_str()).map_err(BackendError::operation)?;
        let taxonomy = TaxonomyClient::new(crate::taxonomy_server_url().as_str()).map_err(BackendError::operation)?;
        let librarything = super::librarything::client_from_environment().map_err(BackendError::operation)?;
        catalogs.set_supported_languages(browsing_preferences.included_catalog_languages()).map_err(BackendError::operation)?;
        let core = Self {
            state: Arc::new(AppState {
                executor,
                backend,
                active_library: Mutex::new(None),
                open_libraries: Mutex::new(HashMap::new()),
                #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
                library_scanning_enabled: AtomicBool::new(library_scanning_enabled),
                library_list_updates: Mutex::new(()),
                preference_updates: Mutex::new(()),
                browser_session_updates: Mutex::new(()),
                account: Mutex::new(account),
                account_refresh: tokio::sync::Mutex::new(()),
                catalogs,
                metadata,
                taxonomy,
                taxonomy_updates: tokio::sync::Mutex::new(()),
                librarything,
                events,
                events_taken: AtomicBool::new(false),
                sync_wake_tx,
                pending_sync,
                sync_stop,
                sync_task: Mutex::new(None),
            }),
        };
        core.start_background_sync(sync_wake_rx)?;
        if resume_sync {
            let session = core.require_account()?;
            if let Err(error) = core.enroll_default_libraries(&session) {
                log::warn!("restored account could not synchronize every library: {error}");
            }
            core.request_background_sync();
        }
        Ok(core)
    }

    pub fn spawn<T, F, Fut>(&self, operation: F) -> BackendTask<T>
    where
        T: super::executor::BackendOutput,
        F: FnOnce(Self) -> Fut,
        Fut: super::executor::BackendFuture<T>,
    {
        self.state.executor.spawn(operation(self.clone()))
    }

    pub fn take_events(&self) -> Result<Receiver<AppEvent>, BackendError> {
        if self.state.events_taken.swap(true, Ordering::AcqRel) {
            return Err(BackendError::message("application event receiver has already been taken"));
        }
        let (sender, receiver) = unbounded();
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).all = Some(sender);
        Ok(receiver)
    }

    pub fn library_updates(&self, library_id: LibraryId) -> async_channel::Receiver<LibraryUpdate> {
        let (sender, receiver) = async_channel::unbounded();
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).libraries.push((library_id, sender));
        receiver
    }

    pub fn library_contents_changes(&self, library_id: LibraryId) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::bounded(1);
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library_contents.push((library_id, sender));
        receiver
    }

    pub fn library_list_changes(&self) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::unbounded();
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library_lists.push(sender);
        receiver
    }

    pub fn transfer_changes(&self) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::unbounded();
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).transfers.push(sender);
        receiver
    }

    pub fn sync_changes(&self) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::unbounded();
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).sync.push(sender);
        receiver
    }

    fn send_sync_changed(&self) {
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).sync.retain(|sender| sender.try_send(()).is_ok());
    }

    pub fn book_removals(&self) -> async_channel::Receiver<(LibraryId, ContentHash)> {
        let (sender, receiver) = async_channel::unbounded();
        self.state.events.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).book_removals.push(sender);
        receiver
    }

    pub fn libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.state.backend.try_get_library_entries().map_err(BackendError::operation)
    }

    pub fn create_library(&self, name: String, root: Option<PathBuf>) -> Result<LibraryEntry, BackendError> {
        let entry = {
            let _guard = self.lock(&self.state.library_list_updates)?;
            self.state.backend.create_library(name, root).map_err(BackendError::operation)?
        };
        self.finish_created_library(entry)
    }

    /// Registers folders selected by a native frontend.
    ///
    /// Paths remain inside the backend after this call. Frontends identify the
    /// resulting libraries only by `LibraryId`.
    pub fn create_libraries_from_paths(&self, paths: Vec<PathBuf>) -> Result<(Vec<LibraryId>, Vec<String>), BackendError> {
        let mut known_paths = self.libraries()?.into_iter().map(|entry| entry.library_path().to_path_buf()).collect::<Vec<_>>();
        let mut created_ids = Vec::new();
        let mut errors = Vec::new();
        for path in paths {
            let display_path = path.display().to_string();
            if !path.is_dir() {
                errors.push(format!("{display_path}: not a folder"));
                continue;
            }
            let canonical_path = match path.canonicalize() {
                Ok(path) => path,
                Err(error) => {
                    errors.push(format!("{display_path}: {error}"));
                    continue;
                }
            };
            if known_paths.contains(&canonical_path) {
                continue;
            }
            match self.create_library(library_name_for_folder(&path), Some(path)) {
                Ok(entry) => {
                    known_paths.push(entry.library_path().to_path_buf());
                    created_ids.push(*entry.library_id());
                }
                Err(error) => errors.push(format!("{display_path}: {error}")),
            }
        }
        Ok((created_ids, errors))
    }

    pub fn import_library_directory(&self, library_id: &LibraryId, directory: crate::DirectoryImport) -> Result<Vec<String>, BackendError> {
        self.library_handle(library_id)?.import_directory_contents(directory)
    }

    /// Ensures the application always has a local library to select.
    ///
    /// The check and creation share the library-list lock so concurrent startup
    /// or reconciliation work cannot create two defaults from the same empty
    /// snapshot.
    pub fn ensure_default_library(&self) -> Result<LibraryEntry, BackendError> {
        let entry = {
            let _guard = self.lock(&self.state.library_list_updates)?;
            if let Some(entry) = self.state.backend.try_get_library_entries().map_err(BackendError::operation)?.into_iter().next() {
                return Ok(entry);
            }
            self.state.backend.create_library("Library".to_owned(), None).map_err(BackendError::operation)?
        };
        self.finish_created_library(entry)
    }

    fn finish_created_library(&self, entry: LibraryEntry) -> Result<LibraryEntry, BackendError> {
        if self.lock(&self.state.active_library)?.as_ref().is_some_and(|library| library.id() == entry.library_id()) {
            *self.lock(&self.state.active_library)? = None;
        }
        self.state.events.send(AppEvent { library_id: *entry.library_id(), event: LibraryEvent::LibraryListChanged });
        let Some(session) = self.current_account()? else {
            return Ok(entry);
        };
        if let Err(error) = self.prepare_library_sync(entry.library_id(), &session) {
            return Err(error);
        }
        self.request_background_sync();
        self.library_entry(entry.library_id())
    }

    pub fn remove_library_from_device(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        {
            let _guard = self.lock(&self.state.library_list_updates)?;
            let entry = self.library_entry(library_id)?;
            if self.lock(&self.state.active_library)?.as_ref().is_some_and(|library| library.id() == library_id) {
                *self.lock(&self.state.active_library)? = None;
            }
            self.lock(&self.state.open_libraries)?.remove(library_id);
            self.state.backend.remove_library(&entry.library_path().to_string_lossy()).map_err(BackendError::operation)?;
        }
        let replacement = self.ensure_default_library();
        self.state.events.send(AppEvent { library_id: *library_id, event: LibraryEvent::LibraryListChanged });
        replacement.map(|_| ())
    }

    pub async fn delete_library(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        // Local actions never depend on network/account availability. Removal
        // records a durable server-deletion intent before discarding local state.
        self.remove_library_from_device(library_id)?;
        let remote_library_id = *library_id;
        match self.current_account() {
            Ok(Some(_)) => match self.authenticated_request(|session| Box::pin(async move { sync_client::delete_library(session, &remote_library_id).await })).await {
                Ok(_) => {
                    if let Err(error) = self.state.backend.complete_library_deletion(&remote_library_id) {
                        log::warn!("server deleted library {remote_library_id}, but its completed queue entry could not be cleared: {error}");
                    }
                }
                Err(error) => log::warn!("library {remote_library_id} was removed locally; server deletion remains queued: {error}"),
            },
            Ok(None) => {}
            Err(error) => log::warn!("library {remote_library_id} was removed locally; account state could not be read, so server deletion remains queued: {error}"),
        }
        Ok(())
    }

    pub fn library(&self, library_id: &LibraryId) -> Result<LibraryBackend, BackendError> {
        let library = self.library_handle(library_id)?;
        *self.lock(&self.state.active_library)? = Some(library.clone());
        Ok(library)
    }

    #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
    pub fn request_library_scan(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        self.library_handle(library_id)?.request_scan();
        Ok(())
    }

    fn library_handle(&self, library_id: &LibraryId) -> Result<LibraryBackend, BackendError> {
        if let Some(library) = self.lock(&self.state.open_libraries)?.get(library_id).cloned() {
            #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
            if self.state.library_scanning_enabled.load(Ordering::Acquire) {
                library.start_scanner();
            }
            return Ok(library);
        }
        let account = self.current_account()?;
        let entry = self.library_entry(library_id)?;
        let mut open_libraries = self.lock(&self.state.open_libraries)?;
        // Opening constructs a pool, scanner threads, and a sync manager. Recheck
        // after taking the insertion lock so concurrent first users cannot each
        // create a live handle for the same library.
        if let Some(library) = open_libraries.get(library_id).cloned() {
            drop(open_libraries);
            #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
            if self.state.library_scanning_enabled.load(Ordering::Acquire) {
                library.start_scanner();
            }
            return Ok(library);
        }
        let path = entry.library_path().to_string_lossy();
        let (library, events) = LibraryBackend::open_deferred_scan(self, &self.state.backend, &path, *library_id).map_err(BackendError::operation)?;
        #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
        if self.state.library_scanning_enabled.load(Ordering::Acquire) {
            library.start_scanner();
        }
        if let Some(session) = account {
            library.adopt_session(session).map_err(BackendError::message)?;
        }
        open_libraries.insert(*library_id, library.clone());
        drop(open_libraries);
        self.forward_library_events(library.clone(), events);
        self.request_library_sync(*library_id, true);
        Ok(library)
    }

    /// Enables scanning after a deferred desktop startup and starts every
    /// library that background synchronization or the UI opened meanwhile.
    #[cfg(all(feature = "scanner", not(target_os = "android"), not(target_arch = "wasm32")))]
    pub fn enable_library_scanning(&self) -> Result<(), BackendError> {
        if self.state.library_scanning_enabled.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let libraries = self.lock(&self.state.open_libraries)?.values().cloned().collect::<Vec<_>>();
        for library in libraries {
            library.start_scanner();
        }
        Ok(())
    }

    pub fn catalog_sources(&self) -> Vec<CatalogSource> {
        self.state.catalogs.sources()
    }

    pub(crate) async fn catalog_page_in(&self, library_id: &LibraryId, source_id: &CatalogSourceId, locator: Option<&str>) -> Result<CatalogPageView, BackendError> {
        let page = self.state.catalogs.page(source_id, locator).await.map_err(BackendError::operation)?;
        self.catalog_page_view(library_id, source_id, page)
    }

    pub(crate) async fn cached_search_catalog_in(&self, library_id: &LibraryId, source_id: &CatalogSourceId, query: &str) -> Result<CatalogPageView, BackendError> {
        let page = self.state.catalogs.cached_search(source_id, query).await.map_err(BackendError::operation)?;
        self.catalog_page_view(library_id, source_id, page)
    }

    pub(crate) async fn search_catalog_in(&self, library_id: &LibraryId, source_id: &CatalogSourceId, search: &CatalogSearch, query: CatalogSearchQuery) -> Result<CatalogPageView, BackendError> {
        let page = self.state.catalogs.search(source_id, search, query).await.map_err(BackendError::operation)?;
        self.catalog_page_view(library_id, source_id, page)
    }

    fn catalog_page_view(&self, library_id: &LibraryId, source_id: &CatalogSourceId, page: CatalogPage) -> Result<CatalogPageView, BackendError> {
        let owned_publications = self.owned_catalog_publications_for_page(library_id, source_id, &page)?;
        Ok(CatalogPageView { page, owned_publications })
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, BackendError> {
        self.state.catalogs.cover(source_id, image_url).await.map_err(BackendError::operation)
    }

    pub(crate) async fn metadata_enrichment(&self, request: &metadata_contract::MetadataEnrichmentRequest) -> Result<metadata_contract::RichMetadataEnrichmentResponse, BackendError> {
        self.state.metadata.enrichment(request).await.map_err(BackendError::operation)
    }

    pub(crate) async fn rich_metadata_enrichment(&self, request: &metadata_contract::MetadataEnrichmentRequest) -> Result<metadata_contract::RichMetadataEnrichmentResponse, BackendError> {
        self.state.metadata.rich_enrichment(request).await.map_err(BackendError::operation)
    }

    pub(crate) fn has_librarything(&self) -> bool {
        self.state.librarything.is_some()
    }

    pub(crate) async fn librarything_sibling_isbns(&self, isbn: &str) -> Result<Vec<String>, BackendError> {
        let client = self.state.librarything.as_ref().ok_or_else(|| BackendError::message("LibraryThing is not configured"))?;
        client.sibling_isbns(isbn).await.map_err(BackendError::operation)
    }

    pub(crate) async fn resolve_edition_identities(&self, request: &metadata_contract::EditionIdentityRequest) -> Result<metadata_contract::EditionIdentityResponse, BackendError> {
        self.state.metadata.resolve_edition_identities(request).await.map_err(BackendError::operation)
    }

    pub(crate) async fn refresh_catalog_subjects_in(&self, library_id: &LibraryId, request: CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseView, BackendError> {
        let response = self.state.catalogs.refresh_subjects(request).await.map_err(BackendError::operation)?;
        self.catalog_subject_browse_view(library_id, response)
    }

    fn catalog_subject_browse_view(&self, library_id: &LibraryId, response: CatalogSubjectBrowseResponse) -> Result<CatalogSubjectBrowseView, BackendError> {
        let owned_publications = self.catalog_ownership(library_id, response.publications.iter().map(|item| (item.source_id.clone(), item.publication.identity.clone())))?;
        Ok(CatalogSubjectBrowseView { response, owned_publications })
    }

    fn catalog_ownership(&self, library_id: &LibraryId, publications: impl IntoIterator<Item = (CatalogSourceId, CatalogPublicationIdentity)>) -> Result<HashSet<CatalogPublicationKey>, BackendError> {
        let mut by_source = HashMap::<CatalogSourceId, Vec<CatalogPublicationIdentity>>::new();
        for (source_id, identity) in publications {
            by_source.entry(source_id).or_default().push(identity);
        }
        let mut owned_publications = HashSet::new();
        for (source_id, identities) in by_source {
            owned_publications.extend(self.owned_catalog_publications_in(library_id, &source_id, &identities)?.into_iter().map(|identity| CatalogPublicationKey::new(source_id.clone(), identity)));
        }
        Ok(owned_publications)
    }

    fn owned_catalog_publications_in(&self, library_id: &LibraryId, source_id: &CatalogSourceId, identities: &[CatalogPublicationIdentity]) -> Result<Vec<CatalogPublicationIdentity>, BackendError> {
        self.library(library_id)?.owned_catalog_publications(source_id, identities)
    }

    fn owned_catalog_publications_for_page(&self, library_id: &LibraryId, source_id: &CatalogSourceId, page: &CatalogPage) -> Result<HashSet<CatalogPublicationKey>, BackendError> {
        self.catalog_ownership(library_id, page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).map(|publication| (source_id.clone(), publication.identity.clone())))
    }

    pub(crate) async fn import_catalog_book_in(&self, destination: &BookImportDestination, request: &CatalogImportRequest) -> Result<ContentHash, BackendError> {
        let library = self.library(destination.library_id())?;
        let download = self.state.catalogs.download_import(request).await.map_err(BackendError::operation)?;
        let directory_id = destination.directory_id().clone();
        let source_id = request.source_id.clone();
        let publication = request.publication.clone();
        let importing_library = library.clone();
        let content_hash = super::executor::run_blocking(move || {
            let mut source = download.open().map_err(|error| error.to_string())?;
            importing_library.import_downloaded_catalog_book(&directory_id, source.as_mut(), download.file_name(), &source_id, &publication).map_err(|error| error.to_string())
        })
        .await
        .map_err(BackendError::operation)?
        .map_err(BackendError::message)?;
        if let Err(error) = self.hydrate_taxonomy_for_book(&library, &content_hash).await {
            log::warn!("catalog book was imported, but missing taxonomy codes could not be hydrated: {error}");
        }
        Ok(content_hash)
    }

    /// Collects locally unsupported codes recorded during subject assignment,
    /// resolves them in one public batch request, persists the returned
    /// ancestor slice, and then recalculates affected local assignments.
    pub(crate) async fn hydrate_taxonomy_for_book(&self, library: &LibraryBackend, content_hash: &ContentHash) -> Result<(), BackendError> {
        self.hydrate_taxonomy_for_books(library, std::slice::from_ref(content_hash)).await
    }

    pub(crate) async fn hydrate_taxonomy_for_books(&self, library: &LibraryBackend, content_hashes: &[ContentHash]) -> Result<(), BackendError> {
        let _update = self.state.taxonomy_updates.lock().await;
        match self.state.taxonomy.current_release_id().await {
            Ok(server_release_id) => {
                self.state.backend.invalidate_unified_taxonomy_if_obsolete(&server_release_id).map_err(BackendError::operation)?;
            }
            Err(error) => log::warn!("continuing with cached taxonomy after release check failed: {error}"),
        }
        // Repair a prior partial reprojection before consulting the resolution
        // ledger. Otherwise an already-fetched code could suppress the retry.
        self.reload_taxonomy_assignments(library)?;
        // Recheck after serializing updates: another import may have fetched
        // the same slice while this operation was waiting.
        let mut codes = Vec::new();
        for content_hash in content_hashes {
            codes.extend(library.recorded_taxonomy_codes(content_hash)?);
        }
        let codes = self.state.backend.unresolved_taxonomy_codes(codes).map_err(BackendError::operation)?;
        let Some(response) = self.state.taxonomy.resolve_codes(codes).await.map_err(BackendError::operation)? else {
            return Ok(());
        };
        let definitions = taxonomy_client::concept_definitions(&response.concepts);
        let resolutions = taxonomy_client::code_resolutions(&response);
        let release_id = response.release_id;
        let backend = self.state.backend.clone();
        super::executor::run_blocking(move || backend.merge_unified_taxonomy_slice(&release_id, &definitions, &resolutions)).await.map_err(BackendError::operation)?.map_err(BackendError::message)?;

        // The taxonomy runtime is shared by every open library. Reseed each
        // library so previously recorded codes benefit from the fetched slice.
        self.reload_taxonomy_assignments(library)
    }

    fn reload_taxonomy_assignments(&self, required: &LibraryBackend) -> Result<(), BackendError> {
        let mut libraries = self.lock(&self.state.open_libraries)?.values().cloned().collect::<Vec<_>>();
        if !libraries.iter().any(|open| open.id() == required.id()) {
            libraries.push(required.clone());
        }
        let mut failures = Vec::new();
        for open in libraries {
            if let Err(error) = open.reload_unified_subject_assignments() {
                failures.push(format!("{}: {error}", open.id()));
            }
        }
        if !failures.is_empty() {
            return Err(BackendError::message(format!("taxonomy was stored, but {} library projection(s) could not be refreshed: {}", failures.len(), failures.join("; "))));
        }
        Ok(())
    }

    pub(crate) async fn hydrate_taxonomy_for_library(&self, library: &LibraryBackend) -> Result<(), BackendError> {
        let _update = self.state.taxonomy_updates.lock().await;
        match self.state.taxonomy.current_release_id().await {
            Ok(server_release_id) => {
                self.state.backend.invalidate_unified_taxonomy_if_obsolete(&server_release_id).map_err(BackendError::operation)?;
            }
            Err(error) => log::warn!("continuing with cached taxonomy after release check failed: {error}"),
        }
        self.reload_taxonomy_assignments(library)?;
        let codes = self.state.backend.unresolved_taxonomy_codes(library.recorded_taxonomy_codes_for_library()?).map_err(BackendError::operation)?;
        let Some(response) = self.state.taxonomy.resolve_codes(codes).await.map_err(BackendError::operation)? else {
            return Ok(());
        };
        let definitions = taxonomy_client::concept_definitions(&response.concepts);
        let resolutions = taxonomy_client::code_resolutions(&response);
        let release_id = response.release_id;
        let backend = self.state.backend.clone();
        super::executor::run_blocking(move || backend.merge_unified_taxonomy_slice(&release_id, &definitions, &resolutions)).await.map_err(BackendError::operation)?.map_err(BackendError::message)?;
        self.reload_taxonomy_assignments(library)
    }

    pub async fn refresh_catalog_sources(&self) -> Result<Vec<CatalogSource>, BackendError> {
        self.state.catalogs.refresh_sources().await.map_err(BackendError::operation)
    }

    /// Central policy for normal browsing queries. A usable remote account
    /// makes synchronized books browseable even when their publication bytes
    /// have not been downloaded to this device. Signed-out and offline views
    /// remain limited to books that can actually be opened locally.
    pub(crate) fn downloaded_books_only(&self) -> Result<bool, BackendError> {
        if self.current_account()?.is_none() {
            return Ok(true);
        }
        let network = self.network_status();
        Ok(!network.connected || network.validated == Some(false))
    }

    pub(crate) async fn download_book_in(&self, library_id: &LibraryId, content_hash: ContentHash) -> Result<crate::DownloadState, BackendError> {
        let library = self.library(library_id)?;
        if matches!(library.download_state(content_hash.clone())?, crate::DownloadState::Downloaded) {
            return Ok(crate::DownloadState::Downloaded);
        }
        library.request_book_download(content_hash.clone()).map_err(BackendError::operation)?;
        self.sync_library(&library).await?;
        library.download_state(content_hash)
    }

    pub(crate) async fn download_folder_in(&self, library_id: &LibraryId, dir_id: crate::DirId) -> Result<FolderDownloadStatus, BackendError> {
        let library = self.library(library_id)?;
        let requested = library.request_folder_download(&dir_id).map_err(BackendError::operation)?;
        if requested > 0 {
            self.sync_library(&library).await?;
        }
        library.folder_download_status(&dir_id)
    }

    async fn sync_library(&self, library: &LibraryBackend) -> Result<(), BackendError> {
        self.synchronize_library_with_retry(library, AssetSync::Transfer(crate::TransferOrigin::Background)).await
    }

    pub fn account_status(&self) -> Result<AccountStatus, BackendError> {
        self.restore_persisted_account_if_needed()?;
        let account = self.current_account()?;
        Ok(account_status_from_session(account.as_ref()))
    }

    fn restore_persisted_account_if_needed(&self) -> Result<(), BackendError> {
        if self.lock(&self.state.account)?.is_some() {
            return Ok(());
        }

        let server = crate::api_server_url();
        let restored = self.state.backend.load_account_session().map_err(BackendError::operation)?.filter(|session| session_is_usable(session, &server));
        let Some(session) = restored else { return Ok(()) };

        self.state.backend.save_account_session(&session).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(session.clone());
        if let Err(error) = self.enroll_default_libraries(&session) {
            log::warn!("persisted account could not synchronize every library: {error}");
        }
        self.request_background_sync();
        Ok(())
    }

    pub async fn account_storage_usage(&self) -> Result<AccountStorageUsage, BackendError> {
        self.authenticated_request(|session| Box::pin(sync_client::account_storage_usage(session))).await.map(Into::into)
    }

    /// Returns no usage when connected to a server that does not expose the
    /// optional account-storage endpoint yet.
    pub async fn account_storage_usage_if_supported(&self) -> Result<Option<AccountStorageUsage>, BackendError> {
        self.authenticated_request(|session| {
            Box::pin(async move {
                match sync_client::account_storage_usage(session).await {
                    Ok(usage) => Ok(Some(usage.into())),
                    Err(error) if error.is_not_found() => Ok(None),
                    Err(error) => Err(error),
                }
            })
        })
        .await
    }

    /// Returns no breakdown when connected to a server that does not expose the
    /// optional per-library storage endpoint yet. An empty list is a different
    /// answer — an account that is charged for nothing.
    pub async fn account_library_storage_usage_if_supported(&self) -> Result<Option<Vec<ServerLibraryStorageUsage>>, BackendError> {
        self.authenticated_request(|session| {
            Box::pin(async move {
                match sync_client::account_library_storage_usage(session).await {
                    Ok(usage) => Ok(Some(usage.libraries.into_iter().filter_map(|entry| entry.library_id.parse().ok().map(|library_id| ServerLibraryStorageUsage { library_id, used_bytes: entry.used_bytes })).collect())),
                    Err(error) if error.is_not_found() => Ok(None),
                    Err(error) => Err(error),
                }
            })
        })
        .await
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<AccountStatus, BackendError> {
        let session = sync_client::login(&crate::api_server_url(), email, password).await.map_err(BackendError::operation)?;
        self.accept_authenticated_session(session).await
    }

    async fn accept_authenticated_session(&self, session: sync_client::Session) -> Result<AccountStatus, BackendError> {
        self.state.backend.save_account_session(&session).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(session.clone());
        *self.lock(&self.state.active_library)? = None;
        if let Err(error) = self.enroll_default_libraries(&session) {
            log::warn!("signed-in account could not synchronize every library: {error}");
        }
        self.request_background_sync();
        Ok(account_status_from_session(Some(&session)))
    }

    pub async fn request_public_registration(&self, email: &str, password: &str) -> Result<(), BackendError> {
        sync_client::request_public_registration(&crate::api_server_url(), email, password).await.map_err(BackendError::operation)
    }

    pub async fn resend_verification(&self, email: &str) -> Result<(), BackendError> {
        sync_client::resend_verification(&crate::api_server_url(), email).await.map_err(BackendError::operation)
    }

    pub async fn verify_email(&self, email: &str, pin: &str) -> Result<AccountStatus, BackendError> {
        let session = sync_client::verify_email(&crate::api_server_url(), email, pin).await.map_err(BackendError::operation)?;
        self.accept_authenticated_session(session).await
    }

    pub async fn request_password_reset(&self, email: &str) -> Result<(), BackendError> {
        sync_client::request_password_reset(&crate::api_server_url(), email).await.map_err(BackendError::operation)
    }

    pub async fn reset_password(&self, token: &str, new_password: &str) -> Result<(), BackendError> {
        sync_client::reset_password(&crate::api_server_url(), token, new_password).await.map_err(BackendError::operation)
    }

    pub async fn logout(&self) -> Result<AccountStatus, BackendError> {
        let session = self.lock(&self.state.account)?.clone();
        if let Some(session) = session.as_ref() {
            if let Err(error) = sync_client::logout(session).await {
                log::warn!("Failed to notify server about logout: {error}");
            }
        }
        self.state.backend.clear_account_session().map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = None;
        *self.lock(&self.state.active_library)? = None;
        Ok(account_status_from_session(None))
    }

    pub async fn account_sessions(&self) -> Result<Vec<AccountSessionSummary>, BackendError> {
        let sessions = self.authenticated_request(|session| Box::pin(sync_client::list_account_sessions(session))).await?;
        Ok(sessions.into_iter().map(|session| AccountSessionSummary { session_id: session.session_id, created_at: session.created_at as f64, expires_at: session.expires_at as f64, current: session.current }).collect())
    }

    pub async fn revoke_account_session(&self, session_id: &str) -> Result<AccountStatus, BackendError> {
        let session_id = session_id.to_owned();
        let revocation = self
            .authenticated_request(|session| {
                let session_id = session_id.clone();
                Box::pin(async move { sync_client::revoke_account_session(session, &session_id).await })
            })
            .await?;
        if revocation.signed_out {
            self.clear_account_locally()?;
            return Ok(account_status_from_session(None));
        }
        self.account_status()
    }

    pub async fn revoke_other_account_sessions(&self) -> Result<u32, BackendError> {
        u32::try_from(self.authenticated_request(|session| Box::pin(sync_client::revoke_other_account_sessions(session))).await?.revoked).map_err(BackendError::operation)
    }

    pub async fn bootstrap_remote_libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.reconcile_libraries().await?;
        let session = self.require_account()?;
        self.enroll_default_libraries(&session)?;
        let mut attached = Vec::new();
        for entry in self.libraries()? {
            let library = self.library_handle(entry.library_id())?;
            self.publish_library(&library).await?;
            self.synchronize_library_with_retry(&library, AssetSync::Transfer(crate::TransferOrigin::UserInitiated)).await?;
            attached.push(entry);
        }
        Ok(attached)
    }

    pub async fn reconcile_libraries(&self) -> Result<LibraryListState, BackendError> {
        let Some(_) = self.current_account()? else {
            return self.library_list_state(None, None);
        };
        self.flush_pending_library_deletions().await?;
        let deleted = self.authenticated_request(|session| Box::pin(sync_client::list_deleted_libraries(session))).await?;
        self.apply_remote_library_deletions(&deleted.library_ids)?;
        let remote = self.authenticated_request(|session| Box::pin(sync_client::list_libraries(session))).await?;
        let session = self.require_account()?;
        let remote_ids = self.attach_remote_libraries(&remote)?;
        self.library_list_state(Some(&session), Some(&remote_ids))
    }

    async fn flush_pending_library_deletions(&self) -> Result<(), BackendError> {
        for library_id in self.state.backend.pending_library_deletions().map_err(BackendError::operation)? {
            self.authenticated_request(|session| Box::pin(async move { sync_client::delete_library(session, &library_id).await })).await?;
            self.state.backend.complete_library_deletion(&library_id).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    fn apply_remote_library_deletions(&self, deleted: &[LibraryId]) -> Result<(), BackendError> {
        let local_ids = self.state.backend.try_get_library_entries().map_err(BackendError::operation)?.into_iter().map(|entry| *entry.library_id()).collect::<std::collections::HashSet<_>>();
        for library_id in deleted.iter().filter(|library_id| local_ids.contains(library_id)) {
            self.remove_library_from_device(library_id)?;
            self.state.backend.complete_library_deletion(library_id).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    fn attach_remote_libraries(&self, remote: &[sync_common::api::libraries::LibrarySummary]) -> Result<std::collections::HashSet<LibraryId>, BackendError> {
        let existing = self.state.backend.try_get_library_entries().map_err(BackendError::operation)?.into_iter().map(|entry| (*entry.library_id(), entry.library_name().to_owned())).collect::<HashMap<_, _>>();
        let _guard = self.lock(&self.state.library_list_updates)?;
        let mut remote_ids = std::collections::HashSet::with_capacity(remote.len());
        let mut changed_library_id = None;
        for library in remote {
            remote_ids.insert(library.library_id.clone());
            if self.state.backend.is_library_detached(&library.library_id).map_err(BackendError::operation)? {
                continue;
            }
            if existing.get(&library.library_id).map(String::as_str) != Some(library.library_name.as_str()) {
                changed_library_id.get_or_insert(library.library_id);
            }
            // Library-list reconciliation updates registry metadata only. A live
            // Library owns expensive shared resources (pool, scanner, sync manager,
            // event forwarding) and must survive metadata refreshes unchanged.
            self.state.backend.attach_remote_library(&library.library_id, &library.library_name).map_err(BackendError::operation)?;
        }
        drop(_guard);
        if let Some(library_id) = changed_library_id {
            self.state.events.send(AppEvent { library_id, event: LibraryEvent::LibraryListChanged });
        }
        Ok(remote_ids)
    }

    fn library_list_state(&self, session: Option<&sync_client::Session>, remote: Option<&std::collections::HashSet<LibraryId>>) -> Result<LibraryListState, BackendError> {
        let libraries = self.libraries()?;
        let sync_statuses = libraries
            .iter()
            .map(|library| {
                let backend = self.library_handle(library.library_id())?;
                let bound_user_id = backend.account_state().map_err(BackendError::message)?.bound_user_id;
                let connection = backend.db.get().map_err(BackendError::operation)?;
                let sync_details = db::get_library_sync_details(&connection).map_err(BackendError::operation)?;
                let (state, server_url, account_id) = match session {
                    None if bound_user_id.is_none() => (LibrarySyncState::LocalOnly, None, None),
                    None => (LibrarySyncState::SignInRequired, Some(crate::API_SERVER_ORIGIN.to_owned()), bound_user_id),
                    Some(session) if bound_user_id.as_deref().is_some_and(|bound| bound != session.user_id()) => (LibrarySyncState::DifferentAccount, Some(crate::API_SERVER_ORIGIN.to_owned()), bound_user_id),
                    Some(session) => {
                        let state = if remote.is_some_and(|remote| remote.contains(library.library_id())) { LibrarySyncState::Synced } else { LibrarySyncState::RemoteMissing };
                        (state, Some(crate::API_SERVER_ORIGIN.to_owned()), Some(session.user_id().to_owned()))
                    }
                };
                Ok(LibrarySyncStatus {
                    library_id: *library.library_id(),
                    library_name: library.library_name().to_owned(),
                    state,
                    server_url,
                    account_id,
                    has_unsynced_changes: sync_details.has_unsynced_changes,
                    last_synced_at: sync_details.last_synced_at,
                })
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        Ok(LibraryListState { libraries, sync_statuses })
    }

    pub async fn sync_all_libraries(&self) -> Result<(), BackendError> {
        let session = self.require_account()?;
        self.enroll_default_libraries(&session)?;
        for entry in self.libraries()? {
            let library = self.library_handle(entry.library_id())?;
            self.publish_library(&library).await?;
            self.synchronize_library_with_retry(&library, AssetSync::Transfer(crate::TransferOrigin::UserInitiated)).await?;
        }
        Ok(())
    }

    fn prepare_library_sync(&self, library_id: &LibraryId, session: &sync_client::Session) -> Result<LibraryBackend, BackendError> {
        let library = self.library_handle(library_id)?;
        let account = library.account_state().map_err(BackendError::message)?;
        if account.user_id.as_deref() != Some(session.user_id()) {
            library.adopt_session(session.clone()).map_err(BackendError::message)?;
        }
        Ok(library)
    }

    /// Binds every local library to the account being adopted.
    fn enroll_default_libraries(&self, session: &sync_client::Session) -> Result<(), BackendError> {
        for entry in self.libraries()? {
            self.prepare_library_sync(entry.library_id(), session)?;
        }
        Ok(())
    }

    pub async fn library_sync_statuses(&self) -> Result<Vec<LibrarySyncStatus>, BackendError> {
        let remote = if self.current_account()?.is_some() {
            Some(self.authenticated_request(|session| Box::pin(sync_client::list_libraries(session))).await?.into_iter().map(|library| library.library_id).collect::<std::collections::HashSet<_>>())
        } else {
            None
        };
        let session = self.current_account()?;
        Ok(self.library_list_state(session.as_ref(), remote.as_ref())?.sync_statuses)
    }

    pub fn transfer_statuses(&self) -> Result<Vec<LibraryTransfers>, BackendError> {
        let libraries = self.lock(&self.state.open_libraries)?.values().cloned().collect::<Vec<_>>();
        let mut groups = libraries
            .into_iter()
            .map(|library| {
                let entry = self.library_entry(library.id())?;
                Ok(LibraryTransfers { library_id: *library.id(), library_name: entry.library_name().to_owned(), scanner_running: library.scanner_running(), transfers: library.transfer_statuses(), history: library.transfer_history() })
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        groups.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        Ok(groups)
    }

    pub fn local_library_storage_usage(&self) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        let mut usage = self
            .libraries()?
            .into_iter()
            .map(|entry| {
                let backend = self.library_handle(entry.library_id())?;
                let connection = backend.db.get().map_err(BackendError::operation)?;
                let used_bytes = db::local_blob_total_bytes(&connection, crate::BlobKind::Book).map_err(BackendError::operation)?;
                #[cfg(not(target_arch = "wasm32"))]
                let used_bytes = {
                    let downloaded_paths =
                        db::get_book_assets(&connection).map_err(BackendError::operation)?.into_iter().filter(|asset| asset.downloaded).map(|asset| asset.rel_path.resolve_under(entry.library_path())).collect::<HashSet<_>>();
                    used_bytes.saturating_add(regular_files_apparent_size(downloaded_paths)?)
                };
                Ok(LocalLibraryStorageUsage { library_id: *entry.library_id(), library_name: entry.library_name().to_owned(), used_bytes })
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        usage.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        Ok(usage)
    }

    pub async fn scan_local_library_storage_usage(&self) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        let core = self.clone();
        super::executor::run_blocking(move || core.local_library_storage_usage()).await.map_err(BackendError::operation)?
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.state.backend.transfer_runtime().network_status()
    }

    pub async fn update_network_status(&self, status: NetworkStatus) -> Result<NetworkStatus, BackendError> {
        let became_allowed = self.state.backend.transfer_runtime().set_network_status(status);
        if became_allowed {
            // Rebuild desired work from durable library facts even when an old
            // in-memory queue cannot resume (for example because its access
            // token has expired). The queue is only an execution cache.
            self.request_background_sync();
            self.resume_transfers().await?;
        }
        Ok(self.network_status())
    }

    async fn resume_transfers(&self) -> Result<(), BackendError> {
        if self.lock(&self.state.account)?.is_none() {
            return Ok(());
        }
        let libraries = self.lock(&self.state.open_libraries)?.values().cloned().collect::<Vec<_>>();
        for library in libraries {
            library.resume_transfers().await.map_err(BackendError::operation)?;
        }
        Ok(())
    }

    async fn background_sync_cycle(&self, periodic: bool) -> Result<(), BackendError> {
        if !self.network_status().connected {
            return Ok(());
        }
        let mut work = {
            let mut pending = self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.take_ready(Instant::now())
        };
        work.full_reconcile |= periodic;
        if !work.full_reconcile && !work.library_list && work.libraries.is_empty() {
            return Ok(());
        }
        let retry = work.clone();
        if let Err(error) = self.execute_background_sync(&work).await {
            self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).merge(retry);
            return Err(error);
        }
        Ok(())
    }

    async fn execute_background_sync(&self, work: &PendingSyncWork) -> Result<(), BackendError> {
        let Some(_session) = self.refresh_account_if_needed().await? else { return Ok(()) };
        if work.full_reconcile || work.library_list {
            self.reconcile_libraries().await?;
        }
        let entries = self.libraries()?;
        let mut publish_failed = HashSet::new();
        if work.full_reconcile {
            // Advertise every library before starting potentially long-running
            // asset transfers, so another device never waits behind uploads to
            // discover libraries added later.
            for entry in &entries {
                let library = self.library_handle(entry.library_id())?;
                if let Err(error) = self.publish_library(&library).await {
                    log::warn!("failed to publish {} during background synchronization: {error}", entry.library_name());
                    publish_failed.insert(*entry.library_id());
                    let mut pending = self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    pending.library_list = true;
                    pending.library(*entry.library_id(), true);
                }
            }
        }
        for entry in entries {
            let Some(scope) = work.libraries.get(entry.library_id()).copied().or_else(|| work.full_reconcile.then_some(LibrarySyncWork { assets: true })) else { continue };
            if publish_failed.contains(entry.library_id()) {
                continue;
            }
            let library = self.library_handle(entry.library_id())?;
            let assets = if scope.assets { AssetSync::Transfer(crate::TransferOrigin::Background) } else { AssetSync::Skip };
            let result = self.synchronize_library_with_retry(&library, assets).await;
            if let Err(error) = result {
                log::warn!("failed to synchronize {} in the background: {error}", entry.library_name());
                if !library.account_state().map_err(BackendError::message)?.signed_in {
                    self.clear_account_locally()?;
                    return Ok(());
                }
                self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library(entry.library_id().clone(), scope.assets);
            }
        }
        Ok(())
    }

    async fn publish_library(&self, library: &LibraryBackend) -> Result<(), BackendError> {
        let session = self.require_account()?;
        let entry = self.library_entry(library.id())?;
        if library.account_state().map_err(BackendError::message)?.bound_user_id.as_deref() != Some(session.user_id()) {
            return Err(BackendError::message("library cannot synchronize with the signed-in account"));
        }
        let library_id = library.id().clone();
        let library_name = entry.library_name().to_owned();
        self.authenticated_request(|session| Box::pin(sync_client::upsert_library(session, library_id.clone(), library_name.clone()))).await?;
        Ok(())
    }

    fn require_account(&self) -> Result<sync_client::Session, BackendError> {
        self.current_account()?.ok_or_else(|| BackendError::message("not signed in"))
    }

    fn current_account(&self) -> Result<Option<sync_client::Session>, BackendError> {
        let account = self.lock(&self.state.account)?.clone();
        let expired = account.as_ref().is_some_and(|session| if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) });
        if expired {
            self.clear_account_locally()?;
            return Ok(None);
        }
        Ok(account)
    }

    async fn refresh_account_if_needed(&self) -> Result<Option<sync_client::Session>, BackendError> {
        self.refresh_account(false, None).await
    }

    async fn refresh_account_after_unauthorized(&self, rejected_token: &str) -> Result<Option<sync_client::Session>, BackendError> {
        self.refresh_account(true, Some(rejected_token)).await
    }

    async fn refresh_account(&self, force: bool, rejected_token: Option<&str>) -> Result<Option<sync_client::Session>, BackendError> {
        let _refresh_guard = self.state.account_refresh.lock().await;
        let Some(session) = self.current_account()? else { return Ok(None) };
        if rejected_token.is_some_and(|token| token != session.token()) {
            return Ok(Some(session));
        }
        if !force && !session.expires_within(AUTH_REFRESH_WINDOW_SECONDS) {
            return Ok(Some(session));
        }
        if session.refresh_token().is_none() {
            if force || session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) {
                self.clear_account_locally()?;
                return Ok(None);
            }
            return Ok(Some(session));
        }
        if session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) {
            self.clear_account_locally()?;
            return Ok(None);
        }
        let refreshed = match sync_client::refresh_session(&session).await {
            Ok(refreshed) => refreshed,
            Err(error) if error.is_unauthorized() || matches!(error, sync_client::AuthError::InvalidData(_)) => {
                self.clear_account_locally()?;
                return Err(BackendError::operation(error));
            }
            Err(error) => return Err(BackendError::operation(error)),
        };
        self.state.backend.save_account_session(&refreshed).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(refreshed.clone());
        let libraries = self.lock(&self.state.open_libraries)?.values().cloned().collect::<Vec<_>>();
        for library in libraries {
            library.adopt_session(refreshed.clone()).map_err(BackendError::message)?;
        }
        // A refreshed credential is a recovery boundary: derive work again
        // instead of depending on authentication-paused queue entries.
        self.request_background_sync();
        Ok(Some(refreshed))
    }

    async fn authenticated_request<T>(&self, mut request: impl for<'a> FnMut(&'a sync_client::Session) -> AuthFuture<'a, T>) -> Result<T, BackendError> {
        let session = self.refresh_account_if_needed().await?.ok_or_else(|| BackendError::message("not signed in"))?;
        match request(&session).await {
            Ok(value) => Ok(value),
            Err(error) if error.is_unauthorized() => {
                let refreshed = self.refresh_account_after_unauthorized(session.token()).await?.ok_or_else(|| BackendError::message("authentication expired; sign in again"))?;
                request(&refreshed).await.map_err(BackendError::operation)
            }
            Err(error) => Err(BackendError::operation(error)),
        }
    }

    async fn synchronize_library_with_retry(&self, library: &LibraryBackend, assets: AssetSync) -> Result<(), BackendError> {
        let session = self.refresh_account_if_needed().await?.ok_or_else(|| BackendError::message("not signed in"))?;
        // Every synchronization entry point passes here. Persist batch
        // classifications and resolved entities before publishing the metadata
        // outbox, while ensuring catalogue downtime can never prevent sync.
        match tokio::time::timeout(Duration::from_secs(20), library.infer_open_library_isbns(usize::MAX, false)).await {
            Ok(Ok(updated)) if updated > 0 => log::info!("Open Library edition matching inferred ISBNs for {updated} books before synchronizing library {}", library.id()),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => log::warn!("Open Library edition matching unavailable before synchronizing library {}: {error}", library.id()),
            Err(_) => log::warn!("Open Library edition matching timed out before synchronizing library {}; continuing with local metadata", library.id()),
        }
        match tokio::time::timeout(Duration::from_secs(30), library.enrich_rich_metadata(usize::MAX, true)).await {
            Ok(Ok(updated)) if updated > 0 => log::info!("metadata enrichment updated {updated} books before synchronizing library {}", library.id()),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => log::warn!("metadata enrichment unavailable before synchronizing library {}: {error}", library.id()),
            Err(_) => log::warn!("metadata enrichment timed out before synchronizing library {}; continuing with local metadata", library.id()),
        }
        // Resolve stable author identities through the same non-blocking gate.
        match tokio::time::timeout(Duration::from_secs(20), library.enrich_open_library_authors(usize::MAX, true)).await {
            Ok(Ok(updated)) if updated > 0 => log::info!("Open Library enrichment resolved {updated} author credits before synchronizing library {}", library.id()),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => log::warn!("Open Library author enrichment unavailable before synchronizing library {}: {error}", library.id()),
            Err(_) => log::warn!("Open Library author enrichment timed out before synchronizing library {}; continuing with provisional identities", library.id()),
        }
        let policy = SyncPolicy { assets, reading_positions: ReadingPositionSync::AfterQuietPeriod(READING_POSITION_SYNC_DEBOUNCE) };
        let outcome = match library.synchronize(policy).await {
            Ok(outcome) => outcome,
            Err(SyncError::AuthenticationRequired) => {
                self.refresh_account_after_unauthorized(session.token()).await?.ok_or_else(|| BackendError::message("authentication expired; sign in again"))?;
                library.synchronize(policy).await.map_err(BackendError::operation)?
            }
            Err(error) => return Err(BackendError::operation(error)),
        };
        // Metadata-only and empty libraries do not necessarily create transfer
        // events. Notify account surfaces explicitly so their last-sync state
        // advances as soon as this successful checkpoint is persisted.
        self.send_sync_changed();
        if let Err(error) = self.hydrate_taxonomy_for_library(library).await {
            log::warn!("library synchronization completed, but missing taxonomy codes could not be hydrated: {error}");
        }
        if let Some(delay) = outcome.next_reading_upload_in {
            self.request_reading_position_sync_after(library.id().clone(), delay);
        }
        if outcome.mutation_issues.iter().any(|issue| matches!(issue, sync_common::MutationIssue::Retryable(rejection) if rejection.reason == sync_common::MutationRejectionReason::MissingDependency)) {
            self.request_reading_position_sync_after(library.id().clone(), BACKGROUND_SYNC_RETRY_INTERVAL);
        }
        let clock_skewed = outcome.mutation_issues.iter().filter(|issue| matches!(issue, sync_common::MutationIssue::Retryable(rejection) if rejection.reason == sync_common::MutationRejectionReason::TimestampTooFarFuture)).count();
        if clock_skewed > 0 {
            log::warn!("{clock_skewed} mutation(s) for library {} were deferred because this device's clock is ahead of the server", library.id());
        }
        let rejected = outcome.mutation_issues.iter().filter(|issue| matches!(issue, sync_common::MutationIssue::Rejected(_))).count();
        if rejected > 0 {
            log::error!("{rejected} mutation(s) for library {} were permanently rejected by the synchronization service and removed from the local outbox", library.id());
        }
        let untransmittable = outcome.mutation_issues.iter().filter(|issue| matches!(issue, sync_common::MutationIssue::Untransmittable(_))).count();
        if untransmittable > 0 {
            log::error!("{untransmittable} mutation(s) for library {} exceed the synchronization batch limit and remain in the local outbox", library.id());
        }
        Ok(())
    }

    fn clear_account_locally(&self) -> Result<(), BackendError> {
        self.state.backend.clear_account_session().map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = None;
        let mut clear_error = None;
        for library in self.lock(&self.state.open_libraries)?.values() {
            if let Err(error) = library.clear_session() {
                clear_error.get_or_insert_with(|| BackendError::message(error));
            }
        }
        *self.lock(&self.state.active_library)? = None;
        clear_error.map_or(Ok(()), Err)
    }

    pub fn reader_preferences(&self) -> Result<ReaderPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_reader_preferences().map_err(BackendError::operation)
    }

    pub fn browser_session(&self) -> Result<crate::BrowserSession, BackendError> {
        let _guard = self.lock(&self.state.browser_session_updates)?;
        self.state.backend.load_browser_session().map_err(BackendError::operation)
    }

    fn update_browser_session(&self, update: impl FnOnce(&mut crate::BrowserSession)) -> Result<(), BackendError> {
        let _guard = self.lock(&self.state.browser_session_updates)?;
        let mut session = self.state.backend.load_browser_session().map_err(BackendError::operation)?;
        update(&mut session);
        self.state.backend.save_browser_session(&session).map_err(BackendError::operation)
    }

    pub fn save_active_library_id(&self, library_id: Option<crate::LibraryId>) -> Result<(), BackendError> {
        self.update_browser_session(|session| session.active_library_id = library_id)
    }

    pub fn save_library_folder_path(&self, library_id: crate::LibraryId, path: Vec<crate::NavigationCrumb<crate::DirId>>) -> Result<(), BackendError> {
        self.update_browser_session(|session| session.places.entry(library_id).or_default().folder_path = path)
    }

    pub fn save_library_subject_path(&self, library_id: crate::LibraryId, path: Vec<crate::NavigationCrumb<library_model::SubjectPath>>) -> Result<(), BackendError> {
        self.update_browser_session(|session| session.places.entry(library_id).or_default().subject_path = path)
    }

    pub fn retain_browser_session_libraries(&self, library_ids: &[crate::LibraryId]) -> Result<(), BackendError> {
        self.update_browser_session(|session| session.places.retain(|library_id, _| library_ids.contains(library_id)))
    }

    pub fn browsing_preferences(&self) -> Result<BrowsingPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_browsing_preferences().map_err(BackendError::operation)
    }

    pub fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        let preferences = self.state.backend.apply_browsing_preference_patch(patch).map_err(BackendError::operation)?;
        self.state.catalogs.set_supported_languages(preferences.included_catalog_languages()).map_err(BackendError::operation)?;
        Ok(preferences)
    }

    pub fn update_reader_preference(&self, patch: ReaderPreferencePatch) -> Result<ReaderPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.apply_reader_preference_patch(patch).map_err(BackendError::operation)
    }

    pub fn adopt_session(&self, session: sync_client::Session) -> Result<(), BackendError> {
        if session.server_url() != &crate::api_server_url() {
            return Err(BackendError::message(format!("application sessions must belong to {}", crate::API_SERVER_ORIGIN)));
        }
        let credentials_expired = if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) };
        if credentials_expired {
            return Err(BackendError::message("cannot adopt an expired session"));
        }
        self.state.backend.save_account_session(&session).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(session.clone());
        *self.lock(&self.state.active_library)? = None;
        if let Err(error) = self.enroll_default_libraries(&session) {
            log::warn!("adopted account could not synchronize every library: {error}");
        }
        self.request_background_sync();
        Ok(())
    }

    fn library_entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        self.libraries()?.into_iter().find(|entry| entry.library_id() == library_id).ok_or_else(|| BackendError::message(format!("library not found: {library_id}")))
    }

    fn forward_library_events(&self, library: LibraryBackend, events: LibraryEventReceiver) {
        let library_id = *library.id();
        let event_tx = self.state.events.clone();
        let sync_wake_tx = self.state.sync_wake_tx.clone();
        let pending_sync = self.state.pending_sync.clone();
        let executor = self.state.executor.clone();
        let enrichment_running = Arc::new(AtomicBool::new(false));
        self.state.executor.spawn_detached(async move {
            while let Ok(event) = events.recv().await {
                if matches!(&event, LibraryEvent::ScanComplete) && enrichment_running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    eprintln!("METADATA_ENRICHMENT scheduled library={library_id}");
                    let library = library.clone();
                    let enrichment_running = enrichment_running.clone();
                    let enrichment_pending_sync = pending_sync.clone();
                    let enrichment_sync_wake_tx = sync_wake_tx.clone();
                    executor.spawn_detached(async move {
                        let result = async {
                            let mut updated = match library.infer_open_library_isbns(usize::MAX, false).await {
                                Ok(updated) => updated,
                                Err(error) => {
                                    log::warn!("Open Library edition matching unavailable after scan: {error}");
                                    0
                                }
                            };
                            updated += match library.enrich_rich_metadata(usize::MAX, true).await {
                                Ok(updated) => updated,
                                Err(error) => {
                                    log::warn!("metadata enrichment unavailable after scan: {error}");
                                    0
                                }
                            };
                            if updated > 0 {
                                enrichment_pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library(library_id, false);
                                let _ = enrichment_sync_wake_tx.try_send(());
                            }
                            Ok::<_, BackendError>(updated)
                        }
                        .await;
                        match result {
                            Ok(updated) if updated > 0 => log::info!("metadata enrichment updated {updated} books"),
                            Ok(_) => {}
                            Err(error) => log::warn!("metadata enrichment paused: {error}"),
                        }
                        enrichment_running.store(false, Ordering::Release);
                    });
                }
                forward_library_event(&pending_sync, &sync_wake_tx, &event_tx, library_id, event);
            }
        });
    }

    fn lock<'a, T>(&self, mutex: &'a Mutex<T>) -> Result<MutexGuard<'a, T>, BackendError> {
        mutex.lock().map_err(|_| BackendError::message("application state lock is poisoned"))
    }
}

fn forward_library_event(pending_sync: &Mutex<PendingSyncWork>, sync_wake_tx: &async_channel::Sender<()>, event_tx: &AppEventBus, library_id: LibraryId, event: LibraryEvent) {
    let sync_assets = match &event {
        LibraryEvent::BookAdded { .. } | LibraryEvent::BookRemoved { .. } | LibraryEvent::ThumbnailBatchProcessed { finished: true, .. } | LibraryEvent::DownloadStatusChanged { state: crate::DownloadState::Queued, .. } => Some(true),
        LibraryEvent::DirectoryAdded { .. } | LibraryEvent::DirectoryRemoved { .. } | LibraryEvent::ScanComplete => Some(false),
        LibraryEvent::ThumbnailGenerated { .. }
        | LibraryEvent::ThumbnailBatchProcessed { .. }
        | LibraryEvent::DownloadStatusChanged { .. }
        | LibraryEvent::FolderDownloadStatusChanged { .. }
        | LibraryEvent::SubjectEnrichmentBatchProcessed { .. }
        | LibraryEvent::TransferStatusChanged
        | LibraryEvent::LibraryListChanged
        | LibraryEvent::LibraryChanged
        | LibraryEvent::ScanStarted => None,
    };
    if let Some(assets) = sync_assets {
        pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library(library_id, assets);
        let _ = sync_wake_tx.try_send(());
    }
    event_tx.send(AppEvent { library_id, event });
}

fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::preferences::{ApplicationTheme, BrowsingPreferencePatch, UiFontFamily};
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;
    use test_support::{fixture_content_hash, fixture_library_id};

    fn write_test_pdf(path: &Path) {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let contents_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 300.into(), 500.into()], "Resources" => dictionary! {}, "Contents" => contents_id });
        document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.save(path).unwrap();
    }

    #[test]
    fn local_library_size_counts_only_selected_regular_book_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("book.epub"), vec![0_u8; 40]).unwrap();
        std::fs::write(root.path().join("not-downloaded.epub"), vec![0_u8; 20]).unwrap();
        std::fs::write(root.path().join("nested/library.db"), vec![0_u8; 2]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("book.epub"), root.path().join("duplicate-link")).unwrap();

        let mut selected = vec![root.path().join("book.epub")];
        #[cfg(unix)]
        selected.push(root.path().join("duplicate-link"));
        assert_eq!(regular_files_apparent_size(selected).unwrap(), 40);
    }

    #[test]
    fn scanner_activity_notifies_batch_job_subscribers() {
        let events = AppEventBus::default();
        let (sender, receiver) = async_channel::unbounded();
        events.0.lock().unwrap().transfers.push(sender);
        let library_id = fixture_library_id("scanner-activity");

        events.send(AppEvent { library_id, event: LibraryEvent::ScanStarted });
        assert!(receiver.try_recv().is_ok());

        events.send(AppEvent { library_id, event: LibraryEvent::ScanComplete });
        assert!(receiver.try_recv().is_ok());
    }

    #[tokio::test]
    async fn background_sync_schedule_debounces_wake_bursts() {
        let (wake_tx, wake_rx) = async_channel::bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let cycles = Arc::new(AtomicUsize::new(0));
        let worker_stop = stop.clone();
        let worker_cycles = cycles.clone();
        let worker = tokio::spawn(async move {
            run_sync_schedule(
                |_periodic| async {
                    worker_cycles.fetch_add(1, Ordering::AcqRel);
                },
                || None,
                wake_rx,
                worker_stop,
                Duration::from_secs(60),
                Duration::from_secs(60),
                Duration::from_millis(20),
            )
            .await;
        });
        wake_tx.try_send(()).unwrap();
        let _ = wake_tx.try_send(());
        for _ in 0..100 {
            if cycles.load(Ordering::Acquire) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(cycles.load(Ordering::Acquire), 1);
        stop.store(true, Ordering::Release);
        let _ = wake_tx.try_send(());
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn background_sync_schedule_wakes_at_a_reading_deadline_not_the_retry_interval() {
        let (_wake_tx, wake_rx) = async_channel::bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let cycle_stop = worker_stop.clone();
        let deadline = Instant::now() + Duration::from_millis(20);
        let started = Instant::now();
        let worker = tokio::spawn(async move {
            run_sync_schedule(|_periodic| async { cycle_stop.store(true, Ordering::Release) }, || Some(deadline), wake_rx, worker_stop, Duration::from_secs(60), Duration::from_secs(60), Duration::from_millis(5)).await;
        });

        worker.await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn notification_work_coalesces_per_library_and_only_escalates_scope() {
        let library = fixture_library_id("library-1");
        let mut pending = PendingSyncWork::default();
        pending.library(library.clone(), false);
        pending.library(library.clone(), false);
        assert_eq!(pending.libraries.len(), 1);
        assert!(!pending.libraries[&library].assets);
        pending.library(library.clone(), true);
        pending.library(library.clone(), false);
        assert_eq!(pending.libraries.len(), 1);
        assert!(pending.libraries[&library].assets);
        assert!(!pending.full_reconcile);
        assert!(!pending.library_list);
        assert!(pending.reading_positions.is_empty());
    }

    #[test]
    fn queued_download_event_schedules_asset_reconciliation() {
        let library_id = fixture_library_id("library-download");
        let pending = Mutex::new(PendingSyncWork::default());
        let (wake_tx, wake_rx) = async_channel::bounded(1);
        forward_library_event(&pending, &wake_tx, &AppEventBus::default(), library_id, LibraryEvent::DownloadStatusChanged { content_hash: fixture_content_hash(17), state: crate::DownloadState::Queued });

        assert!(pending.lock().unwrap().libraries[&library_id].assets);
        assert!(wake_rx.try_recv().is_ok());
    }

    #[test]
    fn a_new_reading_update_resets_the_trailing_deadline() {
        let library = fixture_library_id("library-1");
        let start = Instant::now();
        let mut pending = PendingSyncWork::default();
        pending.reading_positions.insert(library.clone(), start + Duration::from_secs(30));

        assert!(pending.take_ready(start + Duration::from_secs(29)).libraries.is_empty());
        pending.reading_positions.insert(library.clone(), start + Duration::from_secs(50));
        assert!(pending.take_ready(start + Duration::from_secs(31)).libraries.is_empty());

        let ready = pending.take_ready(start + Duration::from_secs(50));
        assert!(ready.libraries.contains_key(&library));
        assert!(pending.reading_positions.is_empty());
    }

    #[test]
    fn app_backend_opens_library_scoped_feature_handles() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Test".to_string(), Some(root.path().join("library"))).unwrap();
        let library = core.library(entry.library_id()).unwrap();
        assert_eq!(library.id(), entry.library_id());
        let root_id = sync_common::ROOT_DIR_ID;
        let created = library.create_directory(&root_id, "Shelf").unwrap();
        assert_eq!(library.folder(&root_id).unwrap().children[0].id, *created.id());
    }

    #[test]
    fn ensure_default_library_creates_only_one_managed_library() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();

        let first = core.ensure_default_library().unwrap();
        let second = core.ensure_default_library().unwrap();
        let libraries = core.libraries().unwrap();

        assert_eq!(first.library_id(), second.library_id());
        assert_eq!(libraries.len(), 1);
        assert_eq!(libraries[0].library_name(), "Library");
    }

    #[test]
    fn removing_the_only_library_creates_a_new_default() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let removed = core.ensure_default_library().unwrap();

        core.remove_library_from_device(removed.library_id()).unwrap();
        let libraries = core.libraries().unwrap();

        assert_eq!(libraries.len(), 1);
        assert_ne!(libraries[0].library_id(), removed.library_id());
        assert_eq!(libraries[0].library_name(), "Library");
    }

    #[test]
    fn deferred_desktop_backend_keeps_scanners_parked_until_enabled() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let core = AppBackend::new_deferred_scanning(context).unwrap();
        let entry = core.create_library("Test".to_owned(), Some(root.path().join("library"))).unwrap();
        let library = core.library(entry.library_id()).unwrap();

        assert!(!library.scanner_running());
        core.enable_library_scanning().unwrap();
        assert!(core.state.library_scanning_enabled.load(Ordering::Acquire));
    }

    #[test]
    fn library_handle_reopens_after_the_active_library_is_closed() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Test".to_owned(), Some(root.path().join("library"))).unwrap();
        core.library(entry.library_id()).unwrap();
        let closed = core.lock(&core.state.active_library).unwrap().take().unwrap();
        drop(closed);

        let reopened = core.library(entry.library_id()).unwrap();
        assert_eq!(reopened.id(), entry.library_id());
        assert_eq!(core.lock(&core.state.active_library).unwrap().as_ref().unwrap().id(), entry.library_id());
    }

    #[test]
    fn library_handle_keeps_imports_scoped_to_its_library() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let active = core.create_library("Active".to_owned(), Some(root.path().join("active"))).unwrap();
        let destination_library = core.create_library("Destination".to_owned(), Some(root.path().join("destination"))).unwrap();
        let active = core.library(active.library_id()).unwrap();
        let destination_library = core.library(destination_library.library_id()).unwrap();
        let root_id = sync_common::ROOT_DIR_ID;
        let folder = destination_library.create_directory(&root_id, "Shelf").unwrap();
        let source = root.path().join("book.pdf");
        write_test_pdf(&source);
        assert!(active.import_book(folder.id(), &source).is_err());
        destination_library.import_book(folder.id(), &source).unwrap();

        assert!(destination_library.folder(folder.id()).unwrap().books.iter().any(|book| book.title == "book"));
        assert!(active.folder(&root_id).unwrap().books.is_empty());
    }

    #[test]
    fn app_backend_persists_canonical_preferences() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().to_path_buf())).unwrap();
        let preferences = core.update_reader_preference(ReaderPreferencePatch::LineHeight(1.8)).unwrap();
        assert_eq!(preferences.line_height(), 1.8);
        let browsing = core.update_browsing_preference(BrowsingPreferencePatch::Theme(ApplicationTheme::LibrarySerif)).unwrap();
        assert_eq!(browsing.theme(), ApplicationTheme::LibrarySerif);
        let browsing = core.update_browsing_preference(BrowsingPreferencePatch::InterfaceFont(UiFontFamily::Serif)).unwrap();
        assert_eq!(browsing.interface_font(), UiFontFamily::Serif);
    }

    #[test]
    fn concurrent_first_open_reuses_one_library_handle() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library_id = *core.create_library("Test".to_string(), Some(root.path().join("library"))).unwrap().library_id();
        let barrier = Arc::new(Barrier::new(8));
        let threads = (0..8)
            .map(|_| {
                let core = core.clone();
                let library_id = library_id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    core.library(&library_id).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let libraries = threads.into_iter().map(|thread| thread.join().unwrap()).collect::<Vec<_>>();
        assert!(libraries.iter().all(|library| library.id() == &library_id));
        assert!(libraries.iter().all(|library| library.shares_scanner_with(&libraries[0])));
    }

    #[test]
    fn adopting_an_account_enables_default_sync_for_local_libraries() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Local".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(session).unwrap();

        let library = core.library(entry.library_id()).unwrap();
        let account = library.account_state().unwrap();
        assert!(account.signed_in);
        assert_eq!(account.bound_user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn libraries_created_while_signed_in_are_synchronized_by_default() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let session = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(session).unwrap();

        let entry = core.create_library("Default sync".to_owned(), Some(root.path().join("library"))).unwrap();
        assert_eq!(core.library(entry.library_id()).unwrap().account_state().unwrap().bound_user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn restored_accounts_enable_default_sync_before_libraries_are_listed() {
        let root = tempfile::tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let library_path = root.path().join("library");
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        let entry = core.create_library("Restored".to_owned(), Some(library_path)).unwrap();
        let library_id = entry.library_id().clone();
        let session = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.state.backend.save_account_session(&session).unwrap();
        drop(core);

        let restored = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data)).unwrap();
        assert_eq!(restored.library(&library_id).unwrap().account_state().unwrap().bound_user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn account_status_rehydrates_a_persisted_session_when_memory_is_empty() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let session = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader@example.com".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(session).unwrap();
        *core.lock(&core.state.account).unwrap() = None;

        let status = core.account_status().unwrap();

        assert!(status.signed_in());
        assert_eq!(status.email(), Some("reader@example.com"));
    }

    #[test]
    fn expired_account_is_cleared_without_changing_protocol_identity() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Linked".to_owned(), Some(root.path().join("library"))).unwrap();
        let live = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(live).unwrap();
        let expired = sync_client::Session::try_new_with_expiry(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43), Some(1)).unwrap();
        core.state.backend.save_account_session(&expired).unwrap();
        *core.lock(&core.state.account).unwrap() = Some(expired);

        assert!(!core.account_status().unwrap().signed_in());
        assert_eq!(core.library(entry.library_id()).unwrap().account_state().unwrap().bound_user_id.as_deref(), Some("user-1"));
        assert!(core.state.backend.load_account_session().unwrap().is_none());
    }

    #[test]
    fn expired_access_token_remains_recoverable_while_its_refresh_token_is_valid() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let session = sync_client::Session::try_new_with_tokens(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43), Some(1), Some("b".repeat(43)), Some(now + 60 * 60)).unwrap();

        core.adopt_session(session).unwrap();

        assert!(core.account_status().unwrap().signed_in());
        assert!(core.state.backend.app_data_dir().join("session.json").exists());
    }

    #[test]
    fn remote_library_list_updates_metadata_without_invalidating_live_handles() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let local = core.create_library("Local name".to_owned(), Some(root.path().join("library"))).unwrap();
        let open_library = core.library(local.library_id()).unwrap();
        let remote_only_id = fixture_library_id("remote-only");
        let session = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        let remote = vec![
            sync_common::api::libraries::LibrarySummary { library_id: local.library_id().clone(), library_name: "Server name".to_owned() },
            sync_common::api::libraries::LibrarySummary { library_id: remote_only_id.clone(), library_name: "Remote only".to_owned() },
        ];

        let remote_ids = core.attach_remote_libraries(&remote).unwrap();
        let state = core.library_list_state(Some(&session), Some(&remote_ids)).unwrap();
        assert_eq!(state.libraries.len(), 2);
        assert!(state.sync_statuses.iter().all(|status| status.state == LibrarySyncState::Synced));
        assert_eq!(core.library_entry(local.library_id()).unwrap().library_name(), "Server name");
        assert_eq!(core.library_entry(&remote_only_id).unwrap().library_name(), "Remote only");
        assert_eq!(open_library.id(), core.library(local.library_id()).unwrap().id(), "unchanged library metadata must not rebuild the active library handle");
    }

    #[test]
    fn advertised_library_reaches_second_client_registry_and_survives_restart() {
        let root = tempfile::tempdir().unwrap();
        let source = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("source"))).unwrap();
        let created = source.create_library("Shared library".to_owned(), None).unwrap();
        let advertisement = sync_common::api::libraries::LibrarySummary { library_id: *created.library_id(), library_name: created.library_name().to_owned() };

        let destination_path = root.path().join("destination");
        let destination = AppBackend::initialize(AppDataLocation::PlatformLocal(destination_path.clone())).unwrap();
        let events = destination.take_events().unwrap();
        destination.attach_remote_libraries(std::slice::from_ref(&advertisement)).unwrap();
        let event = events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(event.library_id, advertisement.library_id);
        assert!(matches!(event.event, LibraryEvent::LibraryListChanged));
        assert_eq!(destination.libraries().unwrap().iter().map(LibraryEntry::library_id).copied().collect::<Vec<_>>(), vec![advertisement.library_id]);

        destination.attach_remote_libraries(std::slice::from_ref(&advertisement)).unwrap();
        assert!(matches!(events.try_recv(), Err(crossbeam_channel::TryRecvError::Empty)));
        drop(destination);

        let restarted = AppBackend::initialize(AppDataLocation::PlatformLocal(destination_path)).unwrap();
        let visible = restarted.libraries().unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].library_id(), &advertisement.library_id);
        assert_eq!(visible[0].library_name(), "Shared library");
    }

    #[test]
    fn preparing_a_library_persists_only_its_protocol_account_identity() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Local".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = sync_client::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();

        let library = core.prepare_library_sync(entry.library_id(), &session).unwrap();
        let account = library.account_state().unwrap();
        assert!(account.signed_in);
        assert_eq!(account.bound_user_id.as_deref(), Some("user-1"));
        let registry = std::fs::read_to_string(core.state.backend.app_data_dir().join("libraries.csv")).unwrap();
        assert!(!registry.contains("sync_"));
    }

    #[test]
    fn concurrent_preference_patches_cannot_lose_each_other() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let threads = [ReaderPreferencePatch::LineHeight(1.8), ReaderPreferencePatch::Zoom(1.7)]
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
        assert_eq!(preferences.line_height(), 1.8);
        assert_eq!(preferences.zoom(), 1.7);
    }

    #[test]
    fn account_switching_rebinds_existing_local_libraries() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library_id = core.create_library("Test".to_string(), None).unwrap().library_id().clone();
        let first = sync_client::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-a".to_string(), "reader-a".to_string(), "a".repeat(43)).unwrap();
        core.adopt_session(first).unwrap();

        let second = sync_client::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-b".to_string(), "reader-b".to_string(), "b".repeat(43)).unwrap();
        core.adopt_session(second).unwrap();

        let library = core.library(&library_id).unwrap();
        let account = library.account_state().unwrap();
        assert!(account.signed_in);
        assert_eq!(account.bound_user_id.as_deref(), Some("user-b"));
    }

    #[test]
    fn account_enrollment_keeps_every_library_available_to_sync_and_report() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        core.create_library("Biografi".to_string(), None).unwrap();
        core.create_library("Humaniora".to_string(), None).unwrap();
        let session = sync_client::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-a".to_string(), "reader-a".to_string(), "a".repeat(43)).unwrap();

        core.adopt_session(session).unwrap();

        assert_eq!(core.lock(&core.state.open_libraries).unwrap().len(), 2);
        assert_eq!(core.transfer_statuses().unwrap().len(), 2);
    }
}

use super::data::{AppDataLocation, BackendContext, LibraryEntry};
use super::executor::{BackendExecutor, BackendTask};
use super::preferences::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences, TransferPreferencePatch, TransferPreferences};
use crate::catalog::opds::{
    built_in_opds_endpoints, custom_endpoints_from_registry, endpoints_from_registry, load_custom_registry_cache, load_registry_cache, registry_from_endpoints, save_registry_cache, OpdsCatalog, OpdsClient, OpdsCredentials,
    OpdsDirectAcquisition, OpdsEndpoint, OpdsEndpointId, OpdsPublicationIdentity, OpdsSearch, OpdsSearchQuery,
};
use crate::catalog::{
    catalog_page_import_request, catalog_provider_route, catalog_sources, discover_author_books_locally, opds_import_request, opds_page, opds_search_definition, opds_search_query, opds_source, overdrive_page, overdrive_search_page,
    search_opds_catalogs, validate_opds_catalog_locator, CatalogAuthorDiscoveryResult, CatalogCoverCache, CatalogImportRequest, CatalogOwnershipStatus, CatalogPage, CatalogProviderRoute, CatalogPublicationIdentity,
    CatalogPublicationOwnership, CatalogSearch, CatalogSearchQuery, CatalogServerClient, CatalogSource, CatalogSourceId, CatalogWideSearchResult,
};
use crate::library::database as db;
use crate::library::BookImportDestination;
use crate::platform::scanner::LibraryEvent;
use crate::sync::{AssetSync, ReadingPositionSync, SyncPolicy};
use crate::{BookId, FolderDownloadStatus, LibraryId, LibraryTransfers, NetworkStatus};
use bokheim_app_contract::frontend::FrontendContract;
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use serde::Serialize;
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use sync_common::api::notifications::NotificationMessage;

use crate::library::LibraryBackend;

const BACKGROUND_SYNC_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const BACKGROUND_RECONCILE_INTERVAL: Duration = Duration::from_secs(10 * 60);
const BACKGROUND_SYNC_DEBOUNCE: Duration = Duration::from_millis(750);
const READING_POSITION_SYNC_DEBOUNCE: Duration = Duration::from_secs(30);
const AUTH_EXPIRY_SAFETY_WINDOW_SECONDS: i64 = 60;
const MAX_CUSTOM_OPDS_ENDPOINTS: usize = 64;
const AUTH_REFRESH_WINDOW_SECONDS: i64 = 15 * 60;

fn catalog_ownership_key(source_id: &CatalogSourceId, edition_id: &str) -> String {
    format!("{}:{edition_id}", source_id.as_str())
}

fn account_status_from_session(session: Option<&crate::auth::Session>) -> AccountStatus {
    session.map(|session| AccountStatus::authenticated(session.server_url().as_str(), session.email())).unwrap_or_else(AccountStatus::signed_out)
}

pub use bokheim_app_contract::account::{AccountStatus, LibrarySyncState, LibrarySyncStatus};

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct AccountSessionSummary {
    pub session_id: String,
    pub created_at: f64,
    pub expires_at: f64,
    pub current: bool,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct LibraryCatalogueState {
    pub libraries: Vec<LibraryEntry>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
}

#[derive(Clone, Debug)]
pub struct CatalogPageView {
    pub page: CatalogPage,
    pub owned_editions: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct CatalogWideSearchView {
    pub result: CatalogWideSearchResult,
    pub owned_editions: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct CatalogAuthorDiscoveryView {
    pub result: CatalogAuthorDiscoveryResult,
    pub owned_editions: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct BackendError(String);

impl BackendError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
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
    catalogue_updates: Mutex<()>,
    preference_updates: Mutex<()>,
    account: Mutex<Option<crate::auth::Session>>,
    account_refresh: tokio::sync::Mutex<()>,
    opds_client: OpdsClient,
    catalog_server: CatalogServerClient,
    catalog_covers: CatalogCoverCache,
    opds_endpoints: Mutex<Vec<OpdsEndpoint>>,
    opds_registry_revision: Mutex<u64>,
    custom_opds_endpoints: Mutex<Vec<OpdsEndpoint>>,
    event_tx: Sender<AppEvent>,
    event_rx: Mutex<Option<Receiver<AppEvent>>>,
    sync_wake_tx: Sender<()>,
    pending_sync: Arc<Mutex<PendingSyncWork>>,
    sync_stop: Arc<AtomicBool>,
    sync_thread: Mutex<Option<JoinHandle<()>>>,
    notification_task: Mutex<Option<BackendTask<()>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LibrarySyncWork {
    assets: bool,
}

#[derive(Clone, Debug, Default)]
struct PendingSyncWork {
    full_reconcile: bool,
    catalogue: bool,
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
        let mut ready = PendingSyncWork { full_reconcile: std::mem::take(&mut self.full_reconcile), catalogue: std::mem::take(&mut self.catalogue), libraries: std::mem::take(&mut self.libraries), reading_positions: HashMap::new() };
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
        self.catalogue |= other.catalogue;
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
        self.notification_task.get_mut().ok().and_then(Option::take);
        let thread = self.sync_thread.get_mut().ok().and_then(Option::take);
        if let Some(thread) = thread {
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

#[derive(Clone)]
pub struct AppBackend {
    state: Arc<AppState>,
}

fn run_sync_schedule(mut cycle: impl FnMut(bool), mut next_reading_deadline: impl FnMut() -> Option<Instant>, wake_rx: Receiver<()>, stop: &AtomicBool, retry_interval: Duration, reconcile_interval: Duration, debounce: Duration) {
    let mut last_reconcile = Instant::now();
    loop {
        let until_reconcile = reconcile_interval.saturating_sub(last_reconcile.elapsed());
        let until_reading = next_reading_deadline().map(|deadline| deadline.saturating_duration_since(Instant::now())).unwrap_or(retry_interval);
        match wake_rx.recv_timeout(retry_interval.min(until_reconcile).min(until_reading)) {
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
        let periodic = last_reconcile.elapsed() >= reconcile_interval;
        if periodic {
            last_reconcile = Instant::now();
        }
        cycle(periodic);
    }
}

async fn notification_retry_delay(stop: &AtomicBool, duration: Duration) {
    let deadline = tokio::time::Instant::now() + duration;
    while !stop.load(Ordering::Acquire) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250).min(deadline.saturating_duration_since(tokio::time::Instant::now()))).await;
    }
}

async fn run_notification_stream(state: Weak<AppState>, stop: Arc<AtomicBool>) {
    let mut retry_seconds = 1_u64;
    while !stop.load(Ordering::Acquire) {
        let session = state.upgrade().and_then(|state| {
            if !state.backend.transfer_runtime().network_status().connected {
                return None;
            }
            state.account.lock().ok()?.clone()
        });
        let Some(session) = session else {
            notification_retry_delay(&stop, Duration::from_secs(1)).await;
            continue;
        };

        let mut stream = match bokheim_sync_client::connect_notifications(&session).await {
            Ok(stream) => stream,
            Err(error) => {
                log::debug!("notification connection failed: {error}");
                let jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| u64::from(duration.subsec_millis()) % 500).unwrap_or(0);
                notification_retry_delay(&stop, Duration::from_millis(retry_seconds * 1000 + jitter)).await;
                retry_seconds = (retry_seconds * 2).min(60);
                continue;
            }
        };
        retry_seconds = 1;
        if let Some(state) = state.upgrade() {
            let core = AppBackend { state };
            core.request_background_sync();
        }

        loop {
            let notification = tokio::select! {
                notification = stream.next() => notification,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if stop.load(Ordering::Acquire) {
                        stream.close().await;
                        return;
                    }
                    let current = state.upgrade().and_then(|state| {
                        let connected = state.backend.transfer_runtime().network_status().connected;
                        let token = state.account.lock().ok()?.as_ref().map(|session| session.token().to_owned());
                        Some((connected, token))
                    });
                    if !matches!(current, Some((true, Some(ref token))) if token == session.token()) {
                        stream.close().await;
                        break;
                    }
                    continue;
                }
            };
            let notification = match notification {
                Ok(Some(notification)) => notification,
                Ok(None) => break,
                Err(error) => {
                    log::debug!("notification connection ended: {error}");
                    break;
                }
            };
            let Some(state) = state.upgrade() else { return };
            let core = AppBackend { state };
            match notification {
                NotificationMessage::Hello { .. } => {}
                NotificationMessage::LibraryChanged { library_id } => core.request_library_sync(library_id, false),
                NotificationMessage::CatalogueChanged => core.request_catalogue_sync(),
                NotificationMessage::Reconcile => core.request_background_sync(),
            }
        }
    }
}

impl AppBackend {
    pub(crate) fn downgrade(&self) -> Weak<AppState> {
        Arc::downgrade(&self.state)
    }

    pub(crate) fn from_weak(state: &Weak<AppState>) -> Result<Self, BackendError> {
        state.upgrade().map(|state| Self { state }).ok_or_else(|| BackendError::message("application backend has been closed"))
    }

    fn start_background_sync(&self, wake_rx: Receiver<()>) -> Result<(), BackendError> {
        let state: Weak<AppState> = Arc::downgrade(&self.state);
        let stop = self.state.sync_stop.clone();
        let executor = self.state.executor.clone();
        let pending_sync = self.state.pending_sync.clone();
        let thread = std::thread::Builder::new()
            .name(format!("{}-sync", crate::APP_NAME_LOWER))
            .spawn(move || {
                run_sync_schedule(
                    |periodic| {
                        let Some(state) = state.upgrade() else {
                            return;
                        };
                        let core = AppBackend { state };
                        if let Err(error) = executor.block_on(core.background_sync_cycle(periodic)) {
                            log::warn!("background synchronization failed: {error}");
                        }
                    },
                    || pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).next_reading_deadline(),
                    wake_rx,
                    &stop,
                    BACKGROUND_SYNC_RETRY_INTERVAL,
                    BACKGROUND_RECONCILE_INTERVAL,
                    BACKGROUND_SYNC_DEBOUNCE,
                );
            })
            .map_err(BackendError::operation)?;
        *self.lock(&self.state.sync_thread)? = Some(thread);
        Ok(())
    }

    fn start_notification_stream(&self) -> Result<(), BackendError> {
        let task = self.state.executor.spawn(run_notification_stream(Arc::downgrade(&self.state), self.state.sync_stop.clone()));
        *self.lock(&self.state.notification_task)? = Some(task);
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

    fn request_catalogue_sync(&self) {
        self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).catalogue = true;
        let _ = self.state.sync_wake_tx.try_send(());
    }

    pub fn initialize(location: AppDataLocation) -> Result<Self, BackendError> {
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        Self::initialize_with_executor(location, executor)
    }

    pub fn initialize_with_executor(location: AppDataLocation, executor: BackendExecutor) -> Result<Self, BackendError> {
        Self::new_with_executor(BackendContext::initialize(location).map_err(BackendError::message)?, executor)
    }

    /// Reuses a host-owned Tokio runtime even when the calling thread is not
    /// entered into that runtime, as is the case for Tauri's setup callback.
    pub fn initialize_on_tokio_runtime(location: AppDataLocation, handle: tokio::runtime::Handle) -> Result<Self, BackendError> {
        let executor = BackendExecutor::from_tokio_handle(handle).map_err(BackendError::operation)?;
        Self::initialize_with_executor(location, executor)
    }

    /// Reuses the Tokio runtime currently entered on this thread.
    pub fn initialize_on_current_runtime(location: AppDataLocation) -> Result<Self, BackendError> {
        let executor = BackendExecutor::from_current_tokio().map_err(BackendError::operation)?;
        Self::initialize_with_executor(location, executor)
    }

    pub fn new(backend: BackendContext) -> Result<Self, BackendError> {
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        Self::new_with_executor(backend, executor)
    }

    pub fn new_with_executor(backend: BackendContext, executor: BackendExecutor) -> Result<Self, BackendError> {
        let (event_tx, event_rx) = unbounded();
        let (sync_wake_tx, sync_wake_rx) = bounded(1);
        let pending_sync = Arc::new(Mutex::new(PendingSyncWork::default()));
        let sync_stop = Arc::new(AtomicBool::new(false));
        let api_server = crate::api_server_url();
        let account = crate::auth::load_session_from(&backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?.filter(|session| session.server_url() == &api_server);
        let resume_sync = account.is_some();
        let transfer_preferences = backend.load_transfer_preferences().map_err(BackendError::operation)?;
        let opds_client = OpdsClient::with_persistent_cache(backend.app_data_dir().join("opds-pages.json")).map_err(BackendError::operation)?;
        let catalog_server = CatalogServerClient::with_persistent_cache(crate::catalog_server_url().as_str(), backend.app_data_dir().join("catalog-server-pages.json")).map_err(BackendError::operation)?;
        let catalog_covers = CatalogCoverCache::new(backend.app_data_dir().join("catalog-covers")).map_err(BackendError::operation)?;
        let cached_opds = load_registry_cache(&backend.app_data_dir().join("opds-catalogs.json")).ok().flatten().filter(|(_, endpoints)| !endpoints.is_empty());
        let opds_registry_revision = cached_opds.as_ref().map(|(registry, _)| registry.revision).unwrap_or(0);
        let opds_endpoints = cached_opds.map(|(_, endpoints)| endpoints).unwrap_or_else(built_in_opds_endpoints);
        let custom_opds_endpoints = load_custom_registry_cache(&backend.app_data_dir().join("custom-opds-catalogs.json")).ok().flatten().map(|(_, endpoints)| endpoints).unwrap_or_default();
        backend.transfer_runtime().set_allow_background_on_metered(transfer_preferences.allow_background_on_metered());
        backend.transfer_runtime().set_auto_download_limit_mb(transfer_preferences.auto_download_limit_mb());
        let core = Self {
            state: Arc::new(AppState {
                executor,
                backend,
                active_library: Mutex::new(None),
                catalogue_updates: Mutex::new(()),
                preference_updates: Mutex::new(()),
                account: Mutex::new(account),
                account_refresh: tokio::sync::Mutex::new(()),
                opds_client,
                catalog_server,
                catalog_covers,
                opds_endpoints: Mutex::new(opds_endpoints),
                opds_registry_revision: Mutex::new(opds_registry_revision),
                custom_opds_endpoints: Mutex::new(custom_opds_endpoints),
                event_tx,
                event_rx: Mutex::new(Some(event_rx)),
                sync_wake_tx,
                pending_sync,
                sync_stop,
                sync_thread: Mutex::new(None),
                notification_task: Mutex::new(None),
            }),
        };
        core.start_background_sync(sync_wake_rx)?;
        core.start_notification_stream()?;
        if resume_sync {
            let session = core.require_account()?;
            if core.ensure_libraries_can_sync(&session).is_ok() {
                if let Err(error) = core.enroll_default_libraries(&session) {
                    log::warn!("restored account could not synchronize every library: {error}");
                    core.clear_account_locally()?;
                } else {
                    core.request_background_sync();
                }
            } else {
                core.clear_account_locally()?;
            }
        }
        Ok(core)
    }

    pub fn spawn<T, F, Fut>(&self, operation: F) -> BackendTask<T>
    where
        T: Send + 'static,
        F: FnOnce(Self) -> Fut,
        Fut: Future<Output = T> + Send + 'static,
    {
        self.state.executor.spawn(operation(self.clone()))
    }

    pub(crate) fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.state.executor.block_on(future)
    }

    pub fn app_data_dir(&self) -> &Path {
        self.state.backend.app_data_dir()
    }

    pub fn take_events(&self) -> Result<Receiver<AppEvent>, BackendError> {
        self.lock(&self.state.event_rx)?.take().ok_or_else(|| BackendError::message("application event receiver has already been taken"))
    }

    pub fn libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.state.backend.try_get_library_entries().map_err(BackendError::operation)
    }

    pub fn create_library(&self, name: String, root: Option<PathBuf>) -> Result<LibraryEntry, BackendError> {
        let entry = {
            let _guard = self.lock(&self.state.catalogue_updates)?;
            self.state.backend.create_library(name, root).map_err(BackendError::operation)?
        };
        if self.lock(&self.state.active_library)?.as_ref().is_some_and(|library| library.id() == entry.library_id()) {
            *self.lock(&self.state.active_library)? = None;
        }
        let Some(session) = self.current_account()? else {
            return Ok(entry);
        };
        if let Err(error) = self.prepare_library_sync(entry.library_id().as_str(), &session) {
            self.clear_account_locally()?;
            return Err(error);
        }
        self.request_background_sync();
        self.library_entry(entry.library_id())
    }

    pub fn remove_library(&self, library_id: &str) -> Result<(), BackendError> {
        let _guard = self.lock(&self.state.catalogue_updates)?;
        let library_id = LibraryId::parse(library_id).map_err(BackendError::operation)?;
        let entry = self.library_entry(&library_id)?;
        if self.lock(&self.state.active_library)?.as_ref().is_some_and(|library| library.id() == &library_id) {
            *self.lock(&self.state.active_library)? = None;
        }
        self.state.backend.remove_library(&entry.library_path().to_string_lossy()).map_err(BackendError::operation)?;
        Ok(())
    }

    pub fn library(&self, library_id: &str) -> Result<LibraryBackend, BackendError> {
        let library_id = LibraryId::parse(library_id).map_err(BackendError::operation)?;
        let account = self.current_account()?;
        let mut active = self.lock(&self.state.active_library)?;
        if let Some(library) = active.as_ref().filter(|library| library.id() == &library_id).cloned() {
            return Ok(library);
        }
        let entry = self.library_entry(&library_id)?;
        let path = entry.library_path().to_string_lossy();
        let (library, events) = LibraryBackend::open(self, &self.state.backend, &path, library_id.as_str()).map_err(BackendError::operation)?;
        if let Some(session) = account {
            library.adopt_session(session).map_err(BackendError::message)?;
        }
        *active = Some(library.clone());
        drop(active);
        self.forward_library_events(library_id.clone(), events);
        self.request_library_sync(library_id, true);
        Ok(library)
    }

    pub fn opds_endpoints(&self) -> Vec<OpdsEndpoint> {
        let registered = self.lock(&self.state.opds_endpoints).map(|endpoints| endpoints.clone()).unwrap_or_default();
        let mut endpoints = built_in_opds_endpoints();
        for endpoint in registered {
            if let Some(existing) = endpoints.iter_mut().find(|existing| existing.id == endpoint.id) {
                *existing = endpoint;
            } else {
                endpoints.push(endpoint);
            }
        }
        if let Ok(custom) = self.lock(&self.state.custom_opds_endpoints) {
            for endpoint in custom.iter().cloned() {
                if let Some(existing) = endpoints.iter_mut().find(|existing| existing.id == endpoint.id) {
                    *existing = endpoint;
                } else {
                    endpoints.push(endpoint);
                }
            }
        }
        endpoints
    }

    pub fn catalog_sources(&self) -> Vec<CatalogSource> {
        catalog_sources(self.opds_endpoints())
    }

    pub fn custom_catalog_sources(&self) -> Result<Vec<CatalogSource>, BackendError> {
        Ok(self.lock(&self.state.custom_opds_endpoints)?.iter().cloned().map(opds_source).collect())
    }

    pub async fn search_catalogs(&self, query: &str) -> Result<CatalogWideSearchResult, BackendError> {
        let custom = self.lock(&self.state.custom_opds_endpoints)?.clone();
        let (shared, device_local) = tokio::join!(self.state.catalog_server.search_catalogs(query), search_opds_catalogs(&self.state.opds_client, custom, query));
        let device_local = device_local.map_err(BackendError::operation)?;
        let mut result = shared.map_err(BackendError::operation)?;
        result.groups.extend(device_local);
        self.state.catalog_covers.remember_wide_search(&result).await;
        Ok(result)
    }

    pub(crate) async fn search_catalogs_in(&self, library_id: &str, query: &str) -> Result<CatalogWideSearchView, BackendError> {
        let result = self.search_catalogs(query).await?;
        let mut owned_editions = HashSet::new();
        for group in &result.groups {
            if let crate::catalog::CatalogWideSearchOutcome::Results { page } = &group.outcome {
                owned_editions.extend(self.owned_catalog_editions_for_page(library_id, &group.source.id, page));
            }
        }
        Ok(CatalogWideSearchView { result, owned_editions })
    }

    pub fn opds_endpoint_for_catalog_source(&self, source_id: &CatalogSourceId) -> Result<Option<OpdsEndpoint>, BackendError> {
        match catalog_provider_route(source_id).map_err(BackendError::operation)? {
            CatalogProviderRoute::Overdrive => Ok(None),
            CatalogProviderRoute::Opds(endpoint_id) => self.opds_endpoints().into_iter().find(|endpoint| endpoint.id == endpoint_id).map(Some).ok_or_else(|| BackendError::message("catalog source is no longer available")),
        }
    }

    pub async fn catalog_page(&self, source_id: &CatalogSourceId, locator: Option<&str>, credentials: Option<&OpdsCredentials>) -> Result<CatalogPage, BackendError> {
        let Some(endpoint) = self.opds_endpoint_for_catalog_source(source_id)? else {
            return overdrive_page(locator).map_err(BackendError::operation);
        };
        let locator = locator.unwrap_or(&endpoint.url);
        validate_opds_catalog_locator(&endpoint, locator).map_err(BackendError::operation)?;
        if credentials.is_none() && self.is_server_cached_endpoint(&endpoint) {
            match self.state.catalog_server.catalog_page(source_id, Some(locator), false).await {
                Ok(page) => {
                    self.state.catalog_covers.remember_page(&page).await;
                    return Ok(page);
                }
                Err(error) => log::warn!(target: "catalog", "catalog server page unavailable; using direct OPDS: {error}"),
            }
        }
        let catalog = self.opds_catalog(locator, credentials).await?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(BackendError::operation)?;
        let page = opds_page(source_id.clone(), catalog);
        self.state.catalog_covers.remember_page(&page).await;
        Ok(page)
    }

    pub(crate) async fn catalog_page_in(&self, library_id: &str, source_id: &CatalogSourceId, locator: Option<&str>, credentials: Option<&OpdsCredentials>) -> Result<CatalogPageView, BackendError> {
        let page = self.catalog_page(source_id, locator, credentials).await?;
        let owned_editions = self.owned_catalog_editions_for_page(library_id, source_id, &page);
        Ok(CatalogPageView { page, owned_editions })
    }

    pub async fn search_catalog(&self, source_id: &CatalogSourceId, search: &CatalogSearch, query: CatalogSearchQuery, credentials: Option<&OpdsCredentials>) -> Result<CatalogPage, BackendError> {
        if matches!(catalog_provider_route(source_id).map_err(BackendError::operation)?, CatalogProviderRoute::Overdrive) {
            return overdrive_search_page(search, &query).map_err(BackendError::operation);
        }
        let endpoint = self.opds_endpoint_for_catalog_source(source_id)?.ok_or_else(|| BackendError::message("catalog source is not an OPDS provider"))?;
        if credentials.is_none() && self.is_server_cached_endpoint(&endpoint) {
            match self.state.catalog_server.search_catalog(source_id, search.clone(), query.clone()).await {
                Ok(page) => {
                    self.state.catalog_covers.remember_page(&page).await;
                    return Ok(page);
                }
                Err(error) => log::warn!(target: "catalog", "catalog server search unavailable; using direct OPDS: {error}"),
            }
        }
        let search = opds_search_definition(search).map_err(BackendError::operation)?;
        let search_origin = match &search {
            OpdsSearch::UriTemplate { base_url, .. } => base_url,
            OpdsSearch::OpenSearchDescription { url } => url,
        };
        validate_opds_catalog_locator(&endpoint, search_origin).map_err(BackendError::operation)?;
        let catalog = self.search_opds_advanced(&search, &opds_search_query(query), credentials).await?;
        validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(BackendError::operation)?;
        let page = opds_page(source_id.clone(), catalog);
        self.state.catalog_covers.remember_page(&page).await;
        Ok(page)
    }

    pub(crate) async fn search_catalog_in(&self, library_id: &str, source_id: &CatalogSourceId, search: &CatalogSearch, query: CatalogSearchQuery, credentials: Option<&OpdsCredentials>) -> Result<CatalogPageView, BackendError> {
        let page = self.search_catalog(source_id, search, query, credentials).await?;
        let owned_editions = self.owned_catalog_editions_for_page(library_id, source_id, &page);
        Ok(CatalogPageView { page, owned_editions })
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, BackendError> {
        if let Some(endpoint) = self.opds_endpoint_for_catalog_source(source_id)? {
            if self.is_server_cached_endpoint(&endpoint) {
                match self.state.catalog_covers.cover_from_server(&self.state.catalog_server, source_id, image_url).await {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) => log::warn!(target: "catalog", "catalog server cover unavailable; fetching the remembered OPDS cover directly: {error}"),
                }
            }
        }
        self.state.catalog_covers.cover(source_id, image_url).await.map_err(BackendError::operation)
    }

    fn is_server_cached_endpoint(&self, endpoint: &OpdsEndpoint) -> bool {
        matches!(endpoint.authentication, crate::catalog::opds::OpdsEndpointAuthentication::None) && self.lock(&self.state.opds_endpoints).is_ok_and(|endpoints| endpoints.iter().any(|candidate| candidate.id == endpoint.id))
    }

    pub(crate) fn catalog_ownership_in(&self, _library_id: &str, _source_id: &CatalogSourceId, identities: &[CatalogPublicationIdentity]) -> Result<Vec<CatalogPublicationOwnership>, BackendError> {
        Ok(identities.iter().map(|identity| CatalogPublicationOwnership { edition_id: identity.edition_id.clone(), status: CatalogOwnershipStatus::NotOwned }).collect())
    }

    fn owned_catalog_editions_for_page(&self, library_id: &str, source_id: &CatalogSourceId, page: &CatalogPage) -> HashSet<String> {
        let identities = page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).map(|publication| publication.identity.clone()).collect::<Vec<_>>();
        self.catalog_ownership_in(library_id, source_id, &identities)
            .unwrap_or_default()
            .into_iter()
            .filter(|ownership| ownership.status == CatalogOwnershipStatus::ExactEditionOwned)
            .map(|ownership| catalog_ownership_key(source_id, &ownership.edition_id))
            .collect()
    }

    pub(crate) async fn import_catalog_book_in(&self, destination: &BookImportDestination, request: &CatalogImportRequest, credentials: Option<&OpdsCredentials>) -> Result<BookId, BackendError> {
        let endpoint = self.opds_endpoint_for_catalog_source(&request.source_id)?.ok_or_else(|| BackendError::message("catalog source does not support imports"))?;
        validate_opds_catalog_locator(&endpoint, &request.catalog_locator).map_err(BackendError::operation)?;
        let server_page = if credentials.is_none() && self.is_server_cached_endpoint(&endpoint) {
            match self.state.catalog_server.catalog_page(&request.source_id, Some(&request.catalog_locator), true).await {
                Ok(page) => Some(page),
                Err(error) => {
                    log::warn!(target: "catalog", "catalog server import lookup unavailable; using direct OPDS: {error}");
                    None
                }
            }
        } else {
            None
        };
        let (source_id, identity, acquisition) = if let Some(page) = server_page {
            catalog_page_import_request(request, &page).map_err(BackendError::operation)?
        } else {
            let catalog = self.opds_catalog_fresh(&request.catalog_locator, credentials).await?;
            validate_opds_catalog_locator(&endpoint, &catalog.source_url).map_err(BackendError::operation)?;
            opds_import_request(request, &catalog).map_err(BackendError::operation)?
        };
        self.import_opds_book(destination, &source_id, &identity, &acquisition, credentials).await
    }

    pub fn add_custom_opds_endpoint(&self, title: &str, url: &str, username_label: Option<&str>, password_required: bool) -> Result<OpdsEndpoint, BackendError> {
        let _guard = self.lock(&self.state.catalogue_updates)?;
        let id = format!("custom-{}", uuid::Uuid::new_v4().simple());
        let authentication = match username_label.map(str::trim).filter(|label| !label.is_empty()) {
            Some(label) => bokheim_opds::CatalogAuthentication::Basic { username_label: label.to_owned(), password_required },
            None => bokheim_opds::CatalogAuthentication::None,
        };
        let access = if matches!(authentication, bokheim_opds::CatalogAuthentication::None) { bokheim_opds::CatalogAccess::Open } else { bokheim_opds::CatalogAccess::Membership };
        let registry = bokheim_opds::CatalogRegistry {
            schema_version: bokheim_opds::CATALOG_REGISTRY_SCHEMA_VERSION,
            revision: 1,
            catalogs: vec![bokheim_opds::CatalogSource { id, title: title.to_owned(), description: "User-supplied OPDS catalog".to_owned(), languages: vec!["Unspecified".to_owned()], url: url.to_owned(), access, authentication }],
        };
        let endpoint = custom_endpoints_from_registry(registry).map_err(BackendError::operation)?.into_iter().next().ok_or_else(|| BackendError::message("custom OPDS endpoint was not created"))?;
        let mut custom = self.lock(&self.state.custom_opds_endpoints)?.clone();
        if custom.len() >= MAX_CUSTOM_OPDS_ENDPOINTS {
            return Err(BackendError::message(format!("at most {MAX_CUSTOM_OPDS_ENDPOINTS} custom OPDS catalogs can be configured")));
        }
        if custom.iter().any(|candidate| candidate.url == endpoint.url) {
            return Err(BackendError::message("this custom OPDS catalog is already configured"));
        }
        custom.push(endpoint.clone());
        save_registry_cache(&self.app_data_dir().join("custom-opds-catalogs.json"), &registry_from_endpoints(1, &custom)).map_err(BackendError::operation)?;
        *self.lock(&self.state.custom_opds_endpoints)? = custom;
        Ok(endpoint)
    }

    pub fn remove_custom_opds_endpoint(&self, endpoint_id: &OpdsEndpointId) -> Result<(), BackendError> {
        let _guard = self.lock(&self.state.catalogue_updates)?;
        let mut custom = self.lock(&self.state.custom_opds_endpoints)?.clone();
        let previous_len = custom.len();
        custom.retain(|endpoint| &endpoint.id != endpoint_id);
        if custom.len() == previous_len {
            return Err(BackendError::message("custom OPDS endpoint was not found"));
        }
        save_registry_cache(&self.app_data_dir().join("custom-opds-catalogs.json"), &registry_from_endpoints(1, &custom)).map_err(BackendError::operation)?;
        *self.lock(&self.state.custom_opds_endpoints)? = custom;
        Ok(())
    }

    pub async fn refresh_opds_endpoints(&self) -> Result<Vec<OpdsEndpoint>, BackendError> {
        let registry = self.state.catalog_server.registry().await.map_err(BackendError::operation)?;
        let endpoints = endpoints_from_registry(registry.clone()).map_err(BackendError::operation)?;
        if endpoints.is_empty() {
            return Err(BackendError::message("the curated OPDS registry is empty"));
        }
        let mut current_revision = self.lock(&self.state.opds_registry_revision)?;
        if registry.revision < *current_revision {
            return Err(BackendError::message(format!("refusing stale OPDS registry revision {} because revision {} is cached", registry.revision, *current_revision)));
        }
        let mut current_endpoints = self.lock(&self.state.opds_endpoints)?;
        if registry.revision == *current_revision && *current_revision != 0 && endpoints != *current_endpoints {
            return Err(BackendError::message(format!("OPDS registry revision {} changed without a revision increment", registry.revision)));
        }
        save_registry_cache(&self.app_data_dir().join("opds-catalogs.json"), &registry).map_err(BackendError::operation)?;
        *current_endpoints = endpoints;
        *current_revision = registry.revision;
        drop(current_endpoints);
        drop(current_revision);
        Ok(self.opds_endpoints())
    }

    pub fn refresh_opds_endpoints_blocking(&self) -> Result<Vec<OpdsEndpoint>, BackendError> {
        self.state.executor.block_on(self.refresh_opds_endpoints())
    }

    pub async fn opds_catalog(&self, url: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, BackendError> {
        self.state.opds_client.fetch_catalog(url, credentials).await.map_err(BackendError::operation)
    }

    pub async fn opds_catalog_fresh(&self, url: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, BackendError> {
        self.state.opds_client.fetch_catalog_fresh(url, credentials).await.map_err(BackendError::operation)
    }

    pub fn opds_catalog_blocking(&self, url: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, BackendError> {
        self.state.executor.block_on(self.opds_catalog(url, credentials))
    }

    pub async fn prefetch_opds_catalog_metadata(&self, catalog: &OpdsCatalog) {
        self.state.opds_client.prefetch_catalog_metadata(catalog).await;
    }

    pub fn prefetch_opds_catalog_metadata_blocking(&self, catalog: &OpdsCatalog) -> Result<(), BackendError> {
        self.state.executor.block_on(self.prefetch_opds_catalog_metadata(catalog));
        Ok(())
    }

    pub async fn search_opds_advanced(&self, search: &OpdsSearch, query: &OpdsSearchQuery, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, BackendError> {
        self.state.opds_client.search_definition(search, query, credentials).await.map_err(BackendError::operation)
    }

    pub fn search_opds_advanced_blocking(&self, search: &OpdsSearch, query: &OpdsSearchQuery, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, BackendError> {
        self.state.executor.block_on(self.search_opds_advanced(search, query, credentials))
    }

    #[rustfmt::skip]
    pub(crate) async fn import_opds_book(&self, destination: &BookImportDestination, source_id: &OpdsEndpointId, identity: &OpdsPublicationIdentity, acquisition: &OpdsDirectAcquisition, credentials: Option<&OpdsCredentials>) -> Result<BookId, BackendError> {
        identity.validate().map_err(BackendError::operation)?;
        let _ = source_id;
        let library = self.library(destination.library_id().as_str())?;
        let temporary_directory = self.app_data_dir().join("opds-downloads");
        let download = self.state.opds_client.download_book(acquisition, credentials, &temporary_directory).await.map_err(BackendError::operation)?;
        let directory_id = destination.directory_id().clone();
        tokio::task::spawn_blocking(move || library.import_book(directory_id.as_str(), download.path(), download.file_name()).map_err(|error| error.to_string()))
            .await
            .map_err(BackendError::operation)?
            .map_err(BackendError::message)
    }

    /// Central policy for normal browsing queries. Unavailable books are only
    /// useful when the user asked for them and the remote account is usable.
    /// Other non-browsing queries intentionally bypass this.
    pub(crate) fn downloaded_books_only(&self) -> Result<bool, BackendError> {
        if !self.browsing_preferences()?.show_unavailable_books() || self.current_account()?.is_none() {
            return Ok(true);
        }
        let network = self.network_status();
        Ok(!network.connected || network.validated == Some(false))
    }

    async fn refresh_catalog_author_books_in(&self, library_id: &str, author_id: &str) -> Result<CatalogAuthorDiscoveryResult, BackendError> {
        let author_id = sync_common::AuthorId::parse(author_id).map_err(BackendError::operation)?;
        let request = self.library(library_id)?.catalog_author_discovery_request(author_id)?;
        self.refresh_catalog_author_request(request).await
    }

    pub(crate) async fn refresh_catalog_author_view_in(&self, library_id: &str, author_id: &str) -> Result<CatalogAuthorDiscoveryView, BackendError> {
        let result = self.refresh_catalog_author_books_in(library_id, author_id).await?;
        let mut by_source = HashMap::<CatalogSourceId, Vec<CatalogPublicationIdentity>>::new();
        for matched in &result.matches {
            by_source.entry(matched.source.id.clone()).or_default().push(matched.publication.identity.clone());
        }
        let owned_editions = by_source
            .into_iter()
            .flat_map(|(source_id, identities)| {
                self.catalog_ownership_in(library_id, &source_id, &identities)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|ownership| ownership.status == CatalogOwnershipStatus::ExactEditionOwned)
                    .map(move |ownership| catalog_ownership_key(&source_id, &ownership.edition_id))
            })
            .collect();
        Ok(CatalogAuthorDiscoveryView { result, owned_editions })
    }

    pub(crate) async fn populate_catalog_author_books_in(&self, library_id: &str, author_id: &str) -> Result<CatalogAuthorDiscoveryResult, BackendError> {
        let author_id = sync_common::AuthorId::parse(author_id).map_err(BackendError::operation)?;
        let request = self.library(library_id)?.catalog_author_discovery_request(author_id)?;
        if let Some((result, _)) = self.state.catalog_server.cached_author_discovery(&request).map_err(BackendError::operation)? {
            return Ok(result);
        }
        if request.book_identifiers.is_empty() {
            return self.refresh_catalog_author_request(request).await;
        }
        let mut name_request = request.clone();
        name_request.book_identifiers.clear();
        let result = self.refresh_catalog_author_request(name_request).await?;
        self.state.catalog_server.cache_author_discovery(&request, &result).await.map_err(BackendError::operation)?;
        Ok(result)
    }

    async fn refresh_catalog_author_request(&self, request: crate::catalog::CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, BackendError> {
        let (result, cache_local_result) = match self.state.catalog_server.refresh_author_books(&request).await {
            Ok(result) => (result, false),
            Err(server_error) => {
                log::warn!(target: "catalog", "catalog server author discovery unavailable; searching OPDS catalogues locally: {server_error}");
                let result = discover_author_books_locally(&self.state.opds_client, self.opds_endpoints(), &request)
                    .await
                    .map_err(|local_error| BackendError::message(format!("catalog server failed ({server_error}); local OPDS fallback failed ({local_error})")))?;
                (result, true)
            }
        };
        if cache_local_result {
            self.state.catalog_server.cache_author_discovery(&request, &result).await.map_err(BackendError::operation)?;
        }
        self.state.catalog_covers.remember_author_discovery(&result).await;
        Ok(result)
    }

    pub(crate) async fn download_book_in(&self, library_id: &str, book_id: BookId) -> Result<crate::DownloadState, BackendError> {
        let library = self.library(library_id)?;
        if matches!(library.download_state(book_id)?, crate::DownloadState::Downloaded) {
            return Ok(crate::DownloadState::Downloaded);
        }
        library.request_book_download(book_id).map_err(BackendError::operation)?;
        self.sync_library(&library).await?;
        library.download_state(book_id)
    }

    pub(crate) async fn download_folder_in(&self, library_id: &str, dir_id: crate::DirId) -> Result<FolderDownloadStatus, BackendError> {
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
        let account = self.current_account()?;
        Ok(account_status_from_session(account.as_ref()))
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<AccountStatus, BackendError> {
        let session = crate::auth::login(email, password).await.map_err(BackendError::operation)?;
        self.accept_authenticated_session(session).await
    }

    async fn accept_authenticated_session(&self, session: crate::auth::Session) -> Result<AccountStatus, BackendError> {
        if let Err(error) = self.ensure_libraries_can_sync(&session) {
            let _ = crate::auth::logout(&session).await;
            return Err(error);
        }
        crate::auth::save_session_to(&session, &self.state.backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(session.clone());
        *self.lock(&self.state.active_library)? = None;
        if let Err(error) = self.enroll_default_libraries(&session) {
            self.clear_account_locally()?;
            let _ = crate::auth::logout(&session).await;
            return Err(error);
        }
        self.request_background_sync();
        Ok(account_status_from_session(Some(&session)))
    }

    pub async fn request_public_registration(&self, email: &str, password: &str) -> Result<(), BackendError> {
        crate::auth::request_public_registration(email, password).await.map_err(BackendError::operation)
    }

    pub async fn resend_verification(&self, email: &str) -> Result<(), BackendError> {
        crate::auth::resend_verification(email).await.map_err(BackendError::operation)
    }

    pub async fn verify_email(&self, token: &str) -> Result<AccountStatus, BackendError> {
        let session = crate::auth::verify_email(token).await.map_err(BackendError::operation)?;
        self.accept_authenticated_session(session).await
    }

    pub async fn request_password_reset(&self, email: &str) -> Result<(), BackendError> {
        crate::auth::request_password_reset(email).await.map_err(BackendError::operation)
    }

    pub async fn reset_password(&self, token: &str, new_password: &str) -> Result<(), BackendError> {
        crate::auth::reset_password(token, new_password).await.map_err(BackendError::operation)
    }

    pub async fn logout(&self) -> Result<AccountStatus, BackendError> {
        let session = self.lock(&self.state.account)?.clone();
        if let Some(session) = session.as_ref() {
            if let Err(error) = crate::auth::logout(session).await {
                log::warn!("Failed to notify server about logout: {error}");
            }
            let cache = self.state.backend.app_data_dir().join("remote-book-cache").join(session.user_id());
            if let Err(error) = std::fs::remove_dir_all(&cache) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("Failed to clear remote book cache on logout: {error}");
                }
            }
        }
        crate::auth::clear_session_from(&self.state.backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = None;
        *self.lock(&self.state.active_library)? = None;
        Ok(account_status_from_session(None))
    }

    pub async fn account_sessions(&self) -> Result<Vec<AccountSessionSummary>, BackendError> {
        let sessions = self.authenticated_request(|session| Box::pin(crate::auth::list_account_sessions(session))).await?;
        Ok(sessions.into_iter().map(|session| AccountSessionSummary { session_id: session.session_id, created_at: session.created_at as f64, expires_at: session.expires_at as f64, current: session.current }).collect())
    }

    pub async fn revoke_account_session(&self, session_id: &str) -> Result<AccountStatus, BackendError> {
        let session_id = session_id.to_owned();
        let revocation = self
            .authenticated_request(|session| {
                let session_id = session_id.clone();
                Box::pin(async move { crate::auth::revoke_account_session(session, &session_id).await })
            })
            .await?;
        if revocation.signed_out {
            self.clear_account_locally()?;
            return Ok(account_status_from_session(None));
        }
        self.account_status()
    }

    pub async fn revoke_other_account_sessions(&self) -> Result<u32, BackendError> {
        u32::try_from(self.authenticated_request(|session| Box::pin(crate::auth::revoke_other_account_sessions(session))).await?.revoked).map_err(BackendError::operation)
    }

    pub async fn bootstrap_remote_libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.reconcile_library_catalogue().await?;
        let session = self.require_account()?;
        self.enroll_default_libraries(&session)?;
        let mut attached = Vec::new();
        for entry in self.libraries()? {
            let library = self.library(entry.library_id().as_str())?;
            self.publish_library(&library).await?;
            self.synchronize_library_with_retry(&library, AssetSync::Transfer(crate::TransferOrigin::UserInitiated)).await?;
            attached.push(entry);
        }
        Ok(attached)
    }

    pub async fn reconcile_library_catalogue(&self) -> Result<LibraryCatalogueState, BackendError> {
        let Some(_) = self.current_account()? else {
            return self.catalogue_state(None, None);
        };
        let remote = self.authenticated_request(|session| Box::pin(crate::auth::list_libraries(session))).await?;
        let session = self.require_account()?;
        let remote_ids = self.attach_remote_catalogue(&remote)?;
        self.catalogue_state(Some(&session), Some(&remote_ids))
    }

    fn attach_remote_catalogue(&self, remote: &[sync_common::api::libraries::LibrarySummary]) -> Result<std::collections::HashSet<LibraryId>, BackendError> {
        let materialize_remote = self.state.backend.materializes_remote_libraries();
        let attached_ids = if materialize_remote { std::collections::HashSet::new() } else { self.state.backend.try_get_library_entries().map_err(BackendError::operation)?.into_iter().map(|entry| entry.library_id().clone()).collect() };
        let _guard = self.lock(&self.state.catalogue_updates)?;
        let mut remote_ids = std::collections::HashSet::with_capacity(remote.len());
        for library in remote {
            remote_ids.insert(library.library_id.clone());
            // Catalogue reconciliation updates registry metadata only. A live
            // Library owns expensive shared resources (pool, scanner, sync manager,
            // event forwarding) and must survive metadata refreshes unchanged.
            if materialize_remote || attached_ids.contains(&library.library_id) {
                self.state.backend.attach_remote_library(&library.library_id, &library.library_name).map_err(BackendError::operation)?;
            }
        }
        Ok(remote_ids)
    }

    fn catalogue_state(&self, session: Option<&crate::auth::Session>, remote: Option<&std::collections::HashSet<LibraryId>>) -> Result<LibraryCatalogueState, BackendError> {
        let libraries = self.libraries()?;
        let sync_statuses = libraries
            .iter()
            .map(|library| {
                let bound_user_id = self.library_bound_user_id(library)?;
                let (state, server_url, account_id) = match session {
                    None if bound_user_id.is_none() => (LibrarySyncState::LocalOnly, None, None),
                    None => (LibrarySyncState::SignInRequired, Some(crate::API_SERVER_ORIGIN.to_owned()), bound_user_id),
                    Some(session) if bound_user_id.as_deref().is_some_and(|bound| bound != session.user_id()) => (LibrarySyncState::DifferentAccount, Some(crate::API_SERVER_ORIGIN.to_owned()), bound_user_id),
                    Some(session) => {
                        let state = if remote.is_some_and(|remote| remote.contains(library.library_id())) { LibrarySyncState::Synced } else { LibrarySyncState::RemoteMissing };
                        (state, Some(crate::API_SERVER_ORIGIN.to_owned()), Some(session.user_id().to_owned()))
                    }
                };
                Ok(LibrarySyncStatus { library_id: library.library_id().as_str().to_owned(), library_name: library.library_name().to_owned(), state, server_url, account_id })
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        Ok(LibraryCatalogueState { libraries, sync_statuses })
    }

    pub async fn sync_all_libraries(&self) -> Result<(), BackendError> {
        let session = self.require_account()?;
        self.enroll_default_libraries(&session)?;
        for entry in self.libraries()? {
            let library = self.library(entry.library_id().as_str())?;
            self.publish_library(&library).await?;
            self.synchronize_library_with_retry(&library, AssetSync::Transfer(crate::TransferOrigin::UserInitiated)).await?;
        }
        Ok(())
    }

    fn prepare_library_sync(&self, library_id: &str, session: &crate::auth::Session) -> Result<LibraryBackend, BackendError> {
        let library_id = LibraryId::parse(library_id).map_err(BackendError::operation)?;
        let library = self.library(library_id.as_str())?;
        let account = library.account_state().map_err(BackendError::message)?;
        if account.bound_user_id.is_some_and(|bound| bound != session.user_id()) {
            return Err(BackendError::message("library history is bound to a different account and requires an explicit migration"));
        }
        if account.user_id.as_deref() != Some(session.user_id()) {
            library.adopt_session(session.clone()).map_err(BackendError::message)?;
        }
        Ok(library)
    }

    /// Binds every local library to the account being adopted. This must cover
    /// the same set `ensure_libraries_can_sync` validates: a library that is
    /// never bound is a library that cannot later refuse a different account.
    /// Reading the active-library handle instead enrolled nothing at all, since
    /// that handle is only populated once something opens a library.
    fn enroll_default_libraries(&self, session: &crate::auth::Session) -> Result<(), BackendError> {
        for entry in self.libraries()? {
            self.prepare_library_sync(entry.library_id().as_str(), session)?;
        }
        Ok(())
    }

    fn ensure_libraries_can_sync(&self, session: &crate::auth::Session) -> Result<(), BackendError> {
        for entry in self.libraries()? {
            if self.library_bound_user_id(&entry)?.as_deref().is_some_and(|bound| bound != session.user_id()) {
                return Err(BackendError::message(format!("cannot sign in as this account because library '{}' belongs to another account", entry.library_name())));
            }
        }
        Ok(())
    }

    fn library_bound_user_id(&self, entry: &LibraryEntry) -> Result<Option<String>, BackendError> {
        if let Some(library) = self.lock(&self.state.active_library)?.as_ref().filter(|library| library.id() == entry.library_id()).cloned() {
            return Ok(library.account_state().map_err(BackendError::message)?.bound_user_id);
        }
        let path = entry.library_path().join(crate::APP_HIDDEN_LIBRARY_DB_PATH);
        if !path.exists() {
            return Ok(None);
        }
        let connection = rusqlite::Connection::open(path).map_err(BackendError::operation)?;
        db::get_sync_metadata(&connection, "bound_user_id").map_err(BackendError::operation)
    }

    pub async fn library_sync_statuses(&self) -> Result<Vec<LibrarySyncStatus>, BackendError> {
        let remote = if self.current_account()?.is_some() {
            Some(self.authenticated_request(|session| Box::pin(crate::auth::list_libraries(session))).await?.into_iter().map(|library| library.library_id).collect::<std::collections::HashSet<_>>())
        } else {
            None
        };
        let session = self.current_account()?;
        Ok(self.catalogue_state(session.as_ref(), remote.as_ref())?.sync_statuses)
    }

    pub fn transfer_statuses(&self) -> Result<Vec<LibraryTransfers>, BackendError> {
        let Some(library) = self.lock(&self.state.active_library)?.clone() else { return Ok(Vec::new()) };
        let entry = self.library_entry(library.id())?;
        Ok(vec![LibraryTransfers { library_id: library.id().as_str().to_owned(), library_name: entry.library_name().to_owned(), transfers: library.transfer_statuses() }])
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.state.backend.transfer_runtime().network_status()
    }

    pub async fn update_network_status(&self, status: NetworkStatus) -> Result<NetworkStatus, BackendError> {
        let became_allowed = self.state.backend.transfer_runtime().set_network_status(status);
        if became_allowed {
            self.resume_transfers().await?;
            self.request_background_sync();
        }
        Ok(self.network_status())
    }

    pub fn transfer_preferences(&self) -> Result<TransferPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_transfer_preferences().map_err(BackendError::operation)
    }

    pub async fn update_transfer_preference(&self, patch: TransferPreferencePatch) -> Result<TransferPreferences, BackendError> {
        let preferences = {
            let _guard = self.lock(&self.state.preference_updates)?;
            self.state.backend.apply_transfer_preference_patch(patch).map_err(BackendError::operation)?
        };
        let became_allowed = self.state.backend.transfer_runtime().set_allow_background_on_metered(preferences.allow_background_on_metered());
        let auto_download_changed = self.state.backend.transfer_runtime().set_auto_download_limit_mb(preferences.auto_download_limit_mb());
        if became_allowed {
            self.resume_transfers().await?;
        }
        if auto_download_changed {
            self.request_background_sync();
        }
        Ok(preferences)
    }

    async fn resume_transfers(&self) -> Result<(), BackendError> {
        if self.lock(&self.state.account)?.is_none() {
            return Ok(());
        }
        let active = { self.lock(&self.state.active_library)?.clone() };
        if let Some(library) = active {
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
        if !work.full_reconcile && !work.catalogue && work.libraries.is_empty() {
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
        if work.full_reconcile || work.catalogue {
            self.reconcile_library_catalogue().await?;
        }
        let Some(library) = ({ self.lock(&self.state.active_library)?.clone() }) else { return Ok(()) };
        let Some(scope) = work.libraries.get(library.id()).copied().or_else(|| work.full_reconcile.then_some(LibrarySyncWork { assets: true })) else { return Ok(()) };
        let entry = self.library_entry(library.id())?;
        {
            if work.full_reconcile {
                if let Err(error) = self.publish_library(&library).await {
                    log::warn!("failed to publish {} during background synchronization: {error}", entry.library_name());
                    let mut pending = self.state.pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    pending.catalogue = true;
                    pending.library(entry.library_id().clone(), true);
                    return Ok(());
                }
            }
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
        self.authenticated_request(|session| Box::pin(crate::auth::upsert_library(session, library_id.clone(), library_name.clone()))).await?;
        Ok(())
    }

    fn require_account(&self) -> Result<crate::auth::Session, BackendError> {
        self.current_account()?.ok_or_else(|| BackendError::message("not signed in"))
    }

    fn current_account(&self) -> Result<Option<crate::auth::Session>, BackendError> {
        let account = self.lock(&self.state.account)?.clone();
        let expired = account.as_ref().is_some_and(|session| if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) });
        if expired {
            self.clear_account_locally()?;
            return Ok(None);
        }
        Ok(account)
    }

    async fn refresh_account_if_needed(&self) -> Result<Option<crate::auth::Session>, BackendError> {
        self.refresh_account(false, None).await
    }

    async fn refresh_account_after_unauthorized(&self, rejected_token: &str) -> Result<Option<crate::auth::Session>, BackendError> {
        self.refresh_account(true, Some(rejected_token)).await
    }

    async fn refresh_account(&self, force: bool, rejected_token: Option<&str>) -> Result<Option<crate::auth::Session>, BackendError> {
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
        let refreshed = match bokheim_sync_client::refresh_session(&session).await {
            Ok(refreshed) => refreshed,
            Err(error) if error.is_unauthorized() || matches!(error, crate::auth::AuthError::InvalidData(_)) => {
                self.clear_account_locally()?;
                return Err(BackendError::operation(error));
            }
            Err(error) => return Err(BackendError::operation(error)),
        };
        crate::auth::save_session_to(&refreshed, &self.state.backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(refreshed.clone());
        if let Some(library) = self.lock(&self.state.active_library)?.clone() {
            library.adopt_session(refreshed.clone()).map_err(BackendError::message)?;
        }
        Ok(Some(refreshed))
    }

    async fn authenticated_request<T>(&self, mut request: impl for<'a> FnMut(&'a crate::auth::Session) -> Pin<Box<dyn Future<Output = Result<T, crate::auth::AuthError>> + Send + 'a>>) -> Result<T, BackendError> {
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
        let policy = SyncPolicy { assets, reading_positions: ReadingPositionSync::AfterQuietPeriod(READING_POSITION_SYNC_DEBOUNCE) };
        let first = library.synchronize(policy).await.map_err(|error| (Self::sync_error_is_unauthorized(error.as_ref()), error.to_string()));
        let outcome = match first {
            Ok(outcome) => outcome,
            Err((true, _)) => {
                self.refresh_account_after_unauthorized(session.token()).await?.ok_or_else(|| BackendError::message("authentication expired; sign in again"))?;
                library.synchronize(policy).await.map_err(BackendError::operation)?
            }
            Err((false, message)) => return Err(BackendError::message(message)),
        };
        if let Some(delay) = outcome.next_reading_upload_in {
            self.request_reading_position_sync_after(library.id().clone(), delay);
        }
        Ok(())
    }

    fn sync_error_is_unauthorized(error: &(dyn std::error::Error + 'static)) -> bool {
        error.downcast_ref::<crate::auth::AuthError>().is_some_and(crate::auth::AuthError::is_unauthorized) || error.downcast_ref::<crate::sync::transfer::AuthenticationRequired>().is_some()
    }

    fn clear_account_locally(&self) -> Result<(), BackendError> {
        crate::auth::clear_session_from(&self.state.backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = None;
        *self.lock(&self.state.active_library)? = None;
        Ok(())
    }

    pub fn reader_preferences(&self) -> Result<ReaderPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_reader_preferences().map_err(BackendError::operation)
    }

    pub fn browsing_preferences(&self) -> Result<BrowsingPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.load_browsing_preferences().map_err(BackendError::operation)
    }

    pub fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.apply_browsing_preference_patch(patch).map_err(BackendError::operation)
    }

    pub fn has_persisted_reader_preferences(&self) -> bool {
        self.state.backend.reader_preferences_path().exists()
    }

    pub fn update_reader_preference(&self, patch: ReaderPreferencePatch) -> Result<ReaderPreferences, BackendError> {
        let _guard = self.lock(&self.state.preference_updates)?;
        self.state.backend.apply_reader_preference_patch(patch).map_err(BackendError::operation)
    }

    pub fn frontend_contract(&self) -> FrontendContract {
        FrontendContract::default()
    }

    pub fn adopt_session(&self, session: crate::auth::Session) -> Result<(), BackendError> {
        if session.server_url() != &crate::api_server_url() {
            return Err(BackendError::message(format!("application sessions must belong to {}", crate::API_SERVER_ORIGIN)));
        }
        let credentials_expired = if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) };
        if credentials_expired {
            return Err(BackendError::message("cannot adopt an expired session"));
        }
        self.ensure_libraries_can_sync(&session)?;
        crate::auth::save_session_to(&session, &self.state.backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = Some(session.clone());
        *self.lock(&self.state.active_library)? = None;
        if let Err(error) = self.enroll_default_libraries(&session) {
            self.clear_account_locally()?;
            return Err(error);
        }
        self.request_background_sync();
        Ok(())
    }

    pub fn clear_library_sessions(&self) -> Result<(), BackendError> {
        crate::auth::clear_session_from(&self.state.backend.app_data_dir().join("session.json")).map_err(BackendError::operation)?;
        *self.lock(&self.state.account)? = None;
        *self.lock(&self.state.active_library)? = None;
        Ok(())
    }

    fn library_entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        self.libraries()?.into_iter().find(|entry| entry.library_id() == library_id).ok_or_else(|| BackendError::message(format!("library not found: {library_id}")))
    }

    fn forward_library_events(&self, library_id: LibraryId, events: Receiver<LibraryEvent>) {
        let event_tx = self.state.event_tx.clone();
        let sync_wake_tx = self.state.sync_wake_tx.clone();
        let pending_sync = self.state.pending_sync.clone();
        std::thread::spawn(move || {
            while let Ok(event) = events.recv() {
                let sync_assets = match &event {
                    LibraryEvent::BookAdded { .. } | LibraryEvent::BookRemoved { .. } | LibraryEvent::ScanComplete => Some(true),
                    LibraryEvent::DirectoryAdded { .. } | LibraryEvent::DirectoryRemoved { .. } => Some(false),
                    LibraryEvent::ThumbnailGenerated { .. } | LibraryEvent::DownloadStatusChanged { .. } | LibraryEvent::FolderDownloadStatusChanged { .. } | LibraryEvent::TransferStatusChanged => None,
                };
                if let Some(assets) = sync_assets {
                    pending_sync.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).library(library_id.clone(), assets);
                    let _ = sync_wake_tx.try_send(());
                }
                if event_tx.send(AppEvent { library_id: library_id.clone(), event }).is_err() {
                    break;
                }
            }
        });
    }

    fn lock<'a, T>(&self, mutex: &'a Mutex<T>) -> Result<MutexGuard<'a, T>, BackendError> {
        mutex.lock().map_err(|_| BackendError::message("application state lock is poisoned"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::preferences::{ApplicationTheme, BrowsingPreferencePatch, ReaderPreferenceKey, ReaderTheme, UiFontFamily};
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;

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
    fn background_sync_schedule_debounces_wake_bursts() {
        let (wake_tx, wake_rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let cycles = Arc::new(AtomicUsize::new(0));
        let worker_stop = stop.clone();
        let worker_cycles = cycles.clone();
        let worker = std::thread::spawn(move || {
            run_sync_schedule(
                |_periodic| {
                    worker_cycles.fetch_add(1, Ordering::AcqRel);
                },
                || None,
                wake_rx,
                &worker_stop,
                Duration::from_secs(60),
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
    fn background_sync_schedule_wakes_at_a_reading_deadline_not_the_retry_interval() {
        let (_wake_tx, wake_rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let deadline = Instant::now() + Duration::from_millis(20);
        let started = Instant::now();
        let worker = std::thread::spawn(move || {
            run_sync_schedule(|_periodic| worker_stop.store(true, Ordering::Release), || Some(deadline), wake_rx, &worker_stop, Duration::from_secs(60), Duration::from_secs(60), Duration::from_millis(5));
        });

        worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn notification_work_coalesces_per_library_and_only_escalates_scope() {
        let library = LibraryId::parse("library-1").unwrap();
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
        assert!(!pending.catalogue);
        assert!(pending.reading_positions.is_empty());
    }

    #[test]
    fn a_new_reading_update_resets_the_trailing_deadline() {
        let library = LibraryId::parse("library-1").unwrap();
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
        assert!(core.state.executor.owns_runtime());
        let entry = core.create_library("Test".to_string(), Some(root.path().join("library"))).unwrap();
        let library = core.library(entry.library_id().as_str()).unwrap();
        assert_eq!(library.id(), entry.library_id());
        let created = library.create_directory("__root__", "Shelf").unwrap();
        assert_eq!(library.folder("__root__").unwrap().directories[0].id, created.id().as_str());
    }

    #[test]
    fn library_handle_reopens_after_the_active_library_is_closed() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Test".to_owned(), Some(root.path().join("library"))).unwrap();
        core.library(entry.library_id().as_str()).unwrap();
        let closed = core.lock(&core.state.active_library).unwrap().take().unwrap();
        drop(closed);

        let reopened = core.library(entry.library_id().as_str()).unwrap();
        assert_eq!(reopened.id(), entry.library_id());
        assert_eq!(core.lock(&core.state.active_library).unwrap().as_ref().unwrap().id(), entry.library_id());
    }

    #[test]
    fn tokio_host_constructors_borrow_the_host_runtime() {
        let root = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let explicit = AppBackend::initialize_on_tokio_runtime(AppDataLocation::PlatformLocal(root.path().join("explicit")), runtime.handle().clone()).unwrap();
        assert!(!explicit.state.executor.owns_runtime());
        drop(explicit);
        let current = runtime.block_on(async { AppBackend::initialize_on_current_runtime(AppDataLocation::PlatformLocal(root.path().join("current"))).unwrap() });
        assert!(!current.state.executor.owns_runtime());
    }

    #[test]
    fn library_handle_keeps_imports_scoped_to_its_library() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let active = core.create_library("Active".to_owned(), Some(root.path().join("active"))).unwrap();
        let destination_library = core.create_library("Destination".to_owned(), Some(root.path().join("destination"))).unwrap();
        let active = core.library(active.library_id().as_str()).unwrap();
        let destination_library = core.library(destination_library.library_id().as_str()).unwrap();
        let folder = destination_library.create_directory("__root__", "Shelf").unwrap();
        let source = root.path().join("book.pdf");
        write_test_pdf(&source);
        assert!(active.import_book(folder.id().as_str(), &source, "book.pdf").is_err());
        destination_library.import_book(folder.id().as_str(), &source, "book.pdf").unwrap();

        assert!(destination_library.folder(folder.id().as_str()).unwrap().books.iter().any(|book| book.title == "book"));
        assert!(active.folder("__root__").unwrap().books.is_empty());
    }

    #[test]
    fn app_backend_exposes_canonical_preferences_and_capabilities() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().to_path_buf())).unwrap();
        let preferences = core.update_reader_preference(ReaderPreferencePatch::Theme(ReaderTheme::Dark)).unwrap();
        assert_eq!(preferences.theme(), ReaderTheme::Dark);
        let browsing = core.update_browsing_preference(BrowsingPreferencePatch::Theme(ApplicationTheme::LibrarySerif)).unwrap();
        assert_eq!(browsing.theme(), ApplicationTheme::LibrarySerif);
        let browsing = core.update_browsing_preference(BrowsingPreferencePatch::InterfaceFont(UiFontFamily::Serif)).unwrap();
        assert_eq!(browsing.interface_font(), UiFontFamily::Serif);
        assert_eq!(browsing.heading_font(), UiFontFamily::Serif);
        let contract = core.frontend_contract().reader_preferences;
        assert!(contract.foliate_capabilities.supports(ReaderPreferenceKey::Theme));
        assert!(contract.native_capabilities.supports(ReaderPreferenceKey::Theme));
    }

    #[test]
    fn concurrent_first_open_reuses_one_library_handle() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library_id = core.create_library("Test".to_string(), Some(root.path().join("library"))).unwrap().library_id().as_str().to_owned();
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
        assert!(libraries.iter().all(|library| library.id().as_str() == library_id));
    }

    #[test]
    fn adopting_an_account_enables_default_sync_for_local_libraries() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Local".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(session).unwrap();

        let library = core.library(entry.library_id().as_str()).unwrap();
        let account = library.account_state().unwrap();
        assert!(account.signed_in);
        assert_eq!(account.bound_user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn libraries_created_while_signed_in_are_synchronized_by_default() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(session).unwrap();

        let entry = core.create_library("Default sync".to_owned(), Some(root.path().join("library"))).unwrap();
        assert_eq!(core.library(entry.library_id().as_str()).unwrap().account_state().unwrap().bound_user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn restored_accounts_enable_default_sync_before_libraries_are_listed() {
        let root = tempfile::tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let library_path = root.path().join("library");
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        let entry = core.create_library("Restored".to_owned(), Some(library_path)).unwrap();
        let library_id = entry.library_id().clone();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        crate::auth::save_session_to(&session, &app_data.join("session.json")).unwrap();
        drop(core);

        let restored = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data)).unwrap();
        assert_eq!(restored.library(library_id.as_str()).unwrap().account_state().unwrap().bound_user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn expired_account_is_cleared_without_changing_protocol_identity() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Linked".to_owned(), Some(root.path().join("library"))).unwrap();
        let live = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        core.adopt_session(live).unwrap();
        let expired = crate::auth::Session::try_new_with_expiry(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43), Some(1)).unwrap();
        crate::auth::save_session_to(&expired, &core.app_data_dir().join("session.json")).unwrap();
        *core.lock(&core.state.account).unwrap() = Some(expired);

        assert!(!core.account_status().unwrap().signed_in());
        assert_eq!(core.library(entry.library_id().as_str()).unwrap().account_state().unwrap().bound_user_id.as_deref(), Some("user-1"));
        assert!(!core.app_data_dir().join("session.json").exists());
    }

    #[test]
    fn expired_access_token_remains_recoverable_while_its_refresh_token_is_valid() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let session = crate::auth::Session::try_new_with_tokens(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43), Some(1), Some("b".repeat(43)), Some(now + 60 * 60)).unwrap();

        core.adopt_session(session).unwrap();

        assert!(core.account_status().unwrap().signed_in());
        assert!(core.app_data_dir().join("session.json").exists());
    }

    #[test]
    fn remote_catalogue_updates_metadata_without_invalidating_live_handles() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let local = core.create_library("Local name".to_owned(), Some(root.path().join("library"))).unwrap();
        let open_library = core.library(local.library_id().as_str()).unwrap();
        let remote_only_id = LibraryId::parse("remote-only").unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();
        let remote = vec![
            sync_common::api::libraries::LibrarySummary { library_id: local.library_id().clone(), library_name: "Server name".to_owned() },
            sync_common::api::libraries::LibrarySummary { library_id: remote_only_id.clone(), library_name: "Remote only".to_owned() },
        ];

        let remote_ids = core.attach_remote_catalogue(&remote).unwrap();
        let state = core.catalogue_state(Some(&session), Some(&remote_ids)).unwrap();
        assert_eq!(state.libraries.len(), 2);
        assert!(state.sync_statuses.iter().all(|status| status.state == LibrarySyncState::Synced));
        assert_eq!(core.library_entry(local.library_id()).unwrap().library_name(), "Server name");
        assert_eq!(core.library_entry(&remote_only_id).unwrap().library_name(), "Remote only");
        assert_eq!(open_library.id(), core.library(local.library_id().as_str()).unwrap().id(), "unchanged catalogue metadata must not rebuild the active library handle");
    }

    #[test]
    fn preparing_a_library_persists_only_its_protocol_account_identity() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let entry = core.create_library("Local".to_owned(), Some(root.path().join("library"))).unwrap();
        let session = crate::auth::Session::try_new(crate::api_server_url(), "user-1".to_owned(), "reader".to_owned(), "a".repeat(43)).unwrap();

        let library = core.prepare_library_sync(entry.library_id().as_str(), &session).unwrap();
        let account = library.account_state().unwrap();
        assert!(account.signed_in);
        assert_eq!(account.bound_user_id.as_deref(), Some("user-1"));
        let registry = std::fs::read_to_string(core.app_data_dir().join("libraries.csv")).unwrap();
        assert!(!registry.contains("sync_"));
    }

    #[test]
    fn concurrent_preference_patches_cannot_lose_each_other() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
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
    fn account_switching_is_rejected_while_libraries_belong_to_the_current_account() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library_id = core.create_library("Test".to_string(), None).unwrap().library_id().clone();
        let first = crate::auth::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-a".to_string(), "reader-a".to_string(), "a".repeat(43)).unwrap();
        core.adopt_session(first).unwrap();

        let second = crate::auth::Session::try_new(crate::ServerUrl::parse(crate::API_SERVER_ORIGIN).unwrap(), "user-b".to_string(), "reader-b".to_string(), "b".repeat(43)).unwrap();
        let error = core.adopt_session(second).unwrap_err();
        assert!(error.to_string().contains("belongs to another account"));

        let library = core.library(library_id.as_str()).unwrap();
        let account = library.account_state().unwrap();
        assert!(account.signed_in);
        assert_eq!(account.bound_user_id.as_deref(), Some("user-a"));
    }

    #[test]
    fn custom_catalog_identity_is_generated_device_local_and_persistent() {
        let root = tempfile::tempdir().unwrap();
        let app_data = root.path().join("app-data");
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        let endpoint = core.add_custom_opds_endpoint("Local books", "http://127.0.0.1:8080/opds", Some("Member"), true).unwrap();
        assert!(endpoint.id.as_str().starts_with("custom-"));
        assert_eq!(endpoint.id.as_str().len(), "custom-".len() + 32);
        let source_id = crate::catalog::opds_catalog_source_id(&endpoint.id);
        assert!(core.custom_catalog_sources().unwrap().iter().any(|source| source.id == source_id));
        assert!(!core.is_server_cached_endpoint(&endpoint));
        assert!(core.add_custom_opds_endpoint("Duplicate", "http://127.0.0.1:8080/opds", None, false).unwrap_err().to_string().contains("already configured"));
        drop(core);
        let restored = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        assert_eq!(restored.custom_catalog_sources().unwrap().iter().map(|source| source.id.clone()).collect::<Vec<_>>(), vec![source_id.clone()]);
        restored.remove_custom_opds_endpoint(&endpoint.id).unwrap();
        drop(restored);
        let removed = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data)).unwrap();
        assert!(removed.custom_catalog_sources().unwrap().is_empty());
    }

    #[test]
    fn bundled_catalogues_remain_available_when_the_cached_registry_is_incomplete() {
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        core.state.opds_endpoints.lock().unwrap().retain(|endpoint| endpoint.id.as_str() == "project-gutenberg");

        let endpoints = core.opds_endpoints();

        assert!(endpoints.iter().any(|endpoint| endpoint.id.as_str() == "open-library"));
        assert_eq!(endpoints.iter().filter(|endpoint| endpoint.id.as_str() == "project-gutenberg").count(), 1);
    }

    #[tokio::test]
    async fn custom_catalog_rejects_a_redirect_to_another_origin() {
        let destination = axum::Router::new().route(
            "/catalog",
            axum::routing::get(|| async { ([(axum::http::header::CONTENT_TYPE, "application/opds+json")], r#"{"metadata":{"title":"Other origin"},"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#) }),
        );
        let destination_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination_address = destination_listener.local_addr().unwrap();
        let destination_task = tokio::spawn(async move { axum::serve(destination_listener, destination).await.unwrap() });
        let source = axum::Router::new().route("/root", axum::routing::get(move || async move { axum::response::Redirect::temporary(&format!("http://{destination_address}/catalog")) }));
        let source_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source_address = source_listener.local_addr().unwrap();
        let source_task = tokio::spawn(async move { axum::serve(source_listener, source).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let core = AppBackend::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let endpoint = core.add_custom_opds_endpoint("Redirecting", &format!("http://{source_address}/root"), None, false).unwrap();
        let source_id = crate::catalog::opds_catalog_source_id(&endpoint.id);
        let error = core.catalog_page(&source_id, None, None).await.unwrap_err();
        assert!(error.to_string().contains("registered OPDS origin"), "{error}");
        source_task.abort();
        destination_task.abort();
    }
}

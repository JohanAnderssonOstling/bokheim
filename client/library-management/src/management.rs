//! Library lifecycle and storage policy.
use crate::executor::{BackendExecutor, BackendOutput, BackendSend};
use crate::library::{LibraryHandle, LibraryOpenOptions};
use crate::{BackendError, LibraryId};
use account_client::{Session, Url};
use client_platform_runtime::storage_queue;
use client_protocol::Failure;
use cpu_host::CpuHost;
use library_registry::{CloudStorageReset, LibraryEntry, LibraryRegistry};
use library_services::account::LibraryAccount;
use library_services::interest::{InterestLease, NotificationInterest};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use sync_common::storage::LibraryStorage;
use tokio::sync::watch;

mod account;
mod client;
mod folder_error;
mod lifecycle;
mod locations;
#[cfg(not(target_arch = "wasm32"))]
mod root_recovery;
mod storage_copy;
pub use folder_error::{LibraryFolderError, LibraryFolderStep};
#[cfg(not(target_arch = "wasm32"))]
mod library_move;
mod quota;
mod registration;
pub use account::LibraryListState;
mod statistics;
pub use client::{ManagementClient, NotificationLease};
pub use statistics::{LocalLibraryStorageUsage, StorageCopyJournal};

/// Host capabilities; library creation does not depend on an application object.
pub struct LibraryServiceConfig {
    pub registry: LibraryRegistry,
    #[cfg(not(target_arch = "wasm32"))]
    pub preference_storage_root: std::path::PathBuf,
    pub executor: BackendExecutor,
    pub cpu: Arc<dyn CpuHost>,
    pub account: LibraryAccount,
    pub notification_interest: NotificationInterest,
    pub sync_server: Url,
    pub metadata_server: String,
    pub library_list_changed: watch::Sender<()>,
    pub account_wake: watch::Sender<()>,
    pub cloud_quota_lock: Arc<tokio::sync::Mutex<()>>,
    pub library_location_lock: Arc<tokio::sync::Mutex<()>>,
    pub storage_copy_lock: Arc<tokio::sync::Mutex<()>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub root_failures: async_channel::Sender<LibraryId>,
}

/// Shared library ownership; background workers retain weak references.
pub struct LibraryService {
    pub(crate) config: LibraryServiceConfig,
    // None means shutdown has started. Retained clients cannot reopen libraries.
    handles: Mutex<Option<HashMap<LibraryId, LibraryHandle>>>,
    opening: tokio::sync::Mutex<()>,
}

impl Drop for LibraryService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl LibraryService {
    pub fn new(config: LibraryServiceConfig) -> Arc<Self> {
        let service = Arc::new(Self { config, handles: Mutex::new(Some(HashMap::new())), opening: tokio::sync::Mutex::new(()) });
        #[cfg(not(target_arch = "wasm32"))]
        service.start_library_move_cleanups();
        service
    }

    /// Resolve an already-open library at a transport or workflow boundary.
    pub fn handle(&self, id: &LibraryId) -> Result<LibraryHandle, BackendError> {
        self.try_handle(id).ok_or_else(|| BackendError::message(format!("library {id} is not open")))
    }

    pub(crate) fn try_handle(&self, id: &LibraryId) -> Option<LibraryHandle> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner).as_ref()?.get(id).cloned()
    }

    pub(crate) fn open_handles(&self) -> Vec<LibraryHandle> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(|handles| handles.values().cloned().collect()).unwrap_or_default()
    }

    // Construction never holds the map lock or blocks handle lookups.
    pub(crate) async fn open_handle(self: &Arc<Self>, options: LibraryOpenOptions, reset_token: Option<CloudStorageReset>) -> Result<bool, BackendError> {
        let _opening = self.opening.lock().await;
        let id = options.id;
        {
            let handles = self.handles.lock().unwrap_or_else(PoisonError::into_inner);
            let handles = handles.as_ref().ok_or_else(|| BackendError::message("library service has shut down"))?;
            if handles.contains_key(&id) {
                return Ok(false);
            }
        }
        let handle = LibraryHandle::open(options, &self.config.executor).await?;
        self.run(move |service| {
            // Finalize the applied reset and publish in the same storage command
            // as the access check. A failed acknowledgement leaves no live handle.
            service.entry_on_storage(&id)?;
            let mut handles = service.handles.lock().unwrap_or_else(PoisonError::into_inner);
            let handles = handles.as_mut().ok_or_else(|| BackendError::message("library service has shut down"))?;
            // Shutdown cannot interleave with acknowledgement and publication.
            if let Some(token) = reset_token {
                service.config.registry.acknowledge_cloud_storage_reset(&id, token).map_err(BackendError::operation)?;
            }
            handles.insert(id, handle);
            Ok(true)
        })
        .await?;
        self.config.account_wake.send_replace(());
        Ok(true)
    }

    pub fn retire(&self, id: &LibraryId) {
        if let Some(handle) = self.handles.lock().unwrap_or_else(PoisonError::into_inner).as_mut().and_then(|handles| handles.remove(id)) {
            handle.shutdown();
        }
    }

    /// Stop libraries immediately even when clients retain the service.
    pub fn shutdown(&self) {
        let handles = self.handles.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(handles) = handles {
            for handle in handles.into_values() {
                handle.shutdown();
            }
        }
    }

    pub(crate) async fn run<T: BackendOutput + Send>(self: &Arc<Self>, operation: impl FnOnce(Arc<Self>) -> Result<T, BackendError> + BackendSend + Send + 'static) -> Result<T, BackendError> {
        let service = self.clone();
        storage_queue::run(move || operation(service)).await.map_err(BackendError::message)?
    }

    pub fn list_changes(&self) -> watch::Receiver<()> {
        self.config.library_list_changed.subscribe()
    }

    pub fn notification_interest(&self, browsing: bool) -> InterestLease {
        self.config.notification_interest.view(browsing)
    }

    pub async fn current_account(self: &Arc<Self>) -> Result<Option<Session>, BackendError> {
        self.run(move |service| service.current_account_on_storage()).await
    }

    pub(crate) fn current_account_on_storage(&self) -> Result<Option<Session>, BackendError> {
        self.config.account.current().map_err(crate::account::account_error)
    }

    pub async fn account_identity(self: &Arc<Self>) -> Result<Option<String>, BackendError> {
        self.run(move |service| service.account_identity_on_storage()).await
    }

    pub(crate) fn account_identity_on_storage(&self) -> Result<Option<String>, BackendError> {
        Ok(self.current_account_on_storage()?.as_ref().map(Session::identity))
    }

    pub(crate) fn bind_to_current_account_on_storage(&self, id: &LibraryId) -> Result<(), BackendError> {
        if self.config.registry.device_only(id).map_err(BackendError::operation)? {
            return Ok(());
        }
        if let Some(account) = self.account_identity_on_storage()? {
            self.config.registry.bind_library_account(id, &account).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    fn library_account(&self, id: LibraryId) -> LibraryAccount {
        let registry = self.config.registry.clone();
        self.config.account.clone().with_authorization(move |session| {
            if registry.device_only(&id).map_err(|error| std::io::Error::other(error.to_string()))? {
                return Ok(false);
            }
            let account = session.identity();
            if !registry.accessible_to_account(&id, Some(&account)).map_err(|error| std::io::Error::other(error.to_string()))? {
                return Ok(false);
            }
            registry.bind_library_account(&id, &account).map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(true)
        })
    }

    pub async fn list(self: &Arc<Self>) -> Result<Vec<LibraryEntry>, BackendError> {
        self.run(move |service| service.list_on_storage()).await
    }

    pub(crate) fn list_on_storage(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.config.registry.entries_for_account(self.account_identity_on_storage()?.as_deref()).map_err(BackendError::operation)
    }

    pub async fn entry(self: &Arc<Self>, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        let library_id = *library_id;
        self.run(move |service| service.entry_on_storage(&library_id)).await
    }

    pub(crate) fn entry_on_storage(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        if !self.config.registry.accessible_to_account(library_id, self.account_identity_on_storage()?.as_deref()).map_err(BackendError::operation)? {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in to the account that owns this library"));
        }
        self.config.registry.entry(library_id).map_err(BackendError::operation)?.ok_or_else(|| BackendError::message(format!("library not found: {library_id}")))
    }

    /// Startup isolates unavailable libraries so other libraries can still open.
    pub async fn open_registered(self: &Arc<Self>) -> Result<(), BackendError> {
        for entry in self.list().await? {
            if self.try_handle(entry.library_id()).is_some() {
                continue;
            }
            if let Err(error) = self.open(entry.library_id()).await {
                log::warn!("Could not open library {} during startup: {error}", entry.library_id());
            }
        }
        Ok(())
    }

    pub async fn create_with_asset_storage(self: &Arc<Self>, name: String, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        let entry = self.register_with_asset_storage(&name, None, asset_storage_enabled).await?;
        self.finish_created(entry).await
    }

    /// Register first so a caller adding folders can report registration and open failures separately.
    pub async fn register_with_asset_storage(self: &Arc<Self>, name: &str, locator: Option<String>, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        let storage = if asset_storage_enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local };
        let name = name.to_owned();
        self.run(move |this| {
            let previous = this.config.registry.entries().map_err(BackendError::operation)?;
            let device_only = !storage.uses_bokheim_assets();
            let library_root = if locator.is_none() { this.effective_library_root()? } else { this.config.registry.default_library_root().to_owned() };
            let entry = this.config.registry.create_with_storage_root(&name, locator, storage, &library_root).map_err(BackendError::operation)?;
            if device_only && !this.config.registry.has_account(entry.library_id()).map_err(BackendError::operation)? {
                this.config.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
            }
            this.bind_to_current_account_on_storage(entry.library_id())?;
            if previous.iter().any(|old| old.library_id() == entry.library_id() && old.storage_locator() != entry.storage_locator()) {
                this.retire(entry.library_id());
            }
            Ok(entry)
        })
        .await
    }

    /// The check and creation share one storage command to avoid duplicate import destinations.
    pub async fn ensure_import_library(self: &Arc<Self>) -> Result<LibraryEntry, BackendError> {
        let _location = self.config.library_location_lock.lock().await;
        for entry in self.list().await? {
            if self.try_handle(entry.library_id()).is_some() || self.open(entry.library_id()).await.is_ok() {
                return Ok(entry);
            }
        }
        let enabled = self.current_account().await?.is_some();
        let entry = self.register_with_asset_storage("Library", None, enabled).await?;
        self.finish_created(entry).await
    }

    pub async fn finish_created(self: &Arc<Self>, entry: LibraryEntry) -> Result<LibraryEntry, BackendError> {
        self.open(entry.library_id()).await?;
        self.config.library_list_changed.send_replace(());
        self.config.account_wake.send_replace(());
        Ok(entry)
    }

    pub async fn rename_library(self: &Arc<Self>, library_id: &LibraryId, name: &str) -> Result<(), BackendError> {
        let library_id = *library_id;
        let name = name.to_owned();
        self.run(move |this| {
            // Require an existing library; renaming must never recreate a deleted one.
            this.entry_on_storage(&library_id)?;
            this.config.registry.rename_library(&library_id, &name).map_err(BackendError::operation)?;
            this.config.library_list_changed.send_replace(());
            this.config.account_wake.send_replace(());
            Ok(())
        }).await
    }

    pub async fn open(self: &Arc<Self>, library_id: &LibraryId) -> Result<(), BackendError> {
        let library_id = *library_id;
        let result = async {
            let (options, reset_token) = self
                .run(move |this| {
                    this.entry_on_storage(&library_id)?;
                    #[cfg(not(target_arch = "wasm32"))]
                    let recovered = matches!(this.config.registry.recover_library_root(&library_id, &this.effective_library_root()?).map_err(BackendError::operation)?, library_registry::LibraryRootRecovery::Recovered(_));
                    let (options, reset_token) = this.open_options_on_storage(library_id)?;
                    #[cfg(not(target_arch = "wasm32"))]
                    let options = LibraryOpenOptions { reset_local_presence: recovered, ..options };
                    Ok((options, reset_token))
                })
                .await?;
            if self.open_handle(options, reset_token).await? {
                #[cfg(not(target_arch = "wasm32"))]
                self.config.registry.access(move |registry| registry.remember_library_root(&library_id)).await.map_err(BackendError::operation)?;
            }
            Ok(())
        }
        .await;
        #[cfg(not(target_arch = "wasm32"))]
        if result.is_err() {
            let _ = self.config.root_failures.try_send(library_id);
        }
        result
    }

    /// Resolve registry policy and host services for opening a library.
    pub async fn open_options(self: &Arc<Self>, library_id: LibraryId) -> Result<(LibraryOpenOptions, Option<CloudStorageReset>), BackendError> {
        self.run(move |service| service.open_options_on_storage(library_id)).await
    }

    pub(crate) fn open_options_on_storage(self: &Arc<Self>, library_id: LibraryId) -> Result<(LibraryOpenOptions, Option<CloudStorageReset>), BackendError> {
        let reset_token = self.config.registry.cloud_storage_reset(&library_id).map_err(BackendError::operation)?;
        let storage = self.config.registry.storage(&library_id).map_err(BackendError::operation)?;
        let entry = self.entry_on_storage(&library_id)?;
        let library_path = entry.storage_locator();
        Ok((
            LibraryOpenOptions {
                id: library_id,
                root: library_path.to_owned(),
                database_locator: self.config.registry.library_database_locator(&library_id, library_path),
                default_sync_server_url: self.current_account_on_storage()?.map(|session| session.server_url().clone()).unwrap_or_else(|| self.config.sync_server.clone()),
                metadata_server_url: self.config.metadata_server.clone(),
                storage,
                reset_cloud_presence: reset_token.is_some(),
                #[cfg(not(target_arch = "wasm32"))]
                reset_local_presence: false,
                account: self.library_account(library_id),
                cpu: self.config.cpu.clone(),
                notification_interest: self.config.notification_interest.clone(),
                #[cfg(not(target_arch = "wasm32"))]
                root_failures: Some(self.config.root_failures.clone()),
            },
            reset_token,
        ))
    }
}

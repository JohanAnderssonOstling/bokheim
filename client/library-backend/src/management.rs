//! Registration and opening lifecycle shared by ordinary and OPDS libraries.
use crate::executor::{BackendExecutor, BackendOutput, BackendSend};
use crate::integration::{LibraryAccount, NotificationInterest};
use crate::library::{LibraryDirectory, LibraryOpenConfig, LibraryRuntimeSpec};
use crate::{BackendError, LibraryId};
use account_client::{Session, Url};
use client_platform_runtime::startup_profile::StartupProfile;
use client_platform_runtime::storage_queue::{self, Priority};
use client_runtime::Failure;
use library_registry::{LibraryEntry, LibraryRegistry};
use std::sync::Arc;
use sync_common::storage::{LibraryKind, LibraryStorage};
use tokio::sync::watch;

mod account;
mod client;
#[cfg(not(target_arch = "wasm32"))]
mod library_move;
mod quota;
pub use account::LibraryListState;
#[cfg(not(target_arch = "wasm32"))]
mod opds;
mod statistics;
#[cfg(target_arch = "wasm32")]
pub use client::ManagementTransport;
pub use client::{ManagementClient, ManagementCommand, ManagementSubscription, NotificationLease};
pub use statistics::{LocalLibraryStorageUsage, StorageCopyJournal};

/// Host capabilities; library creation does not depend on an application object.
pub struct LibraryServiceConfig {
    pub registry: LibraryRegistry,
    pub library_directory: Arc<LibraryDirectory>,
    pub executor: BackendExecutor,
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
    pub catalog: crate::catalog_sources::CatalogService,
    #[cfg(not(target_arch = "wasm32"))]
    pub root_failures: async_channel::Sender<LibraryId>,
}

#[derive(Clone)]
pub struct LibraryService {
    pub(crate) state: Arc<LibraryServiceConfig>,
}

impl LibraryService {
    pub fn new(config: LibraryServiceConfig) -> Self {
        let service = Self { state: Arc::new(config) };
        #[cfg(not(target_arch = "wasm32"))]
        service.start_library_move_cleanups();
        service
    }

    pub(crate) async fn run<T: BackendOutput>(&self, operation: impl FnOnce(Self) -> Result<T, BackendError> + BackendSend + 'static) -> Result<T, BackendError> {
        let service = self.clone();
        storage_queue::run(Priority::Interactive, move || operation(service)).await.map_err(BackendError::message)?
    }

    pub fn list_changes(&self) -> watch::Receiver<()> {
        self.state.library_list_changed.subscribe()
    }

    pub fn notification_interest(&self, browsing: bool) -> crate::integration::InterestLease {
        self.state.notification_interest.view(browsing)
    }

    /// Browser subscription lifetime is tied to its receiver across the worker boundary.
    #[cfg(target_arch = "wasm32")]
    pub fn notification_subscription(&self, browsing: bool) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::bounded(1);
        let interest = self.notification_interest(browsing);
        self.state.executor.spawn_detached(async move {
            let _interest = interest;
            sender.closed().await;
        });
        receiver
    }

    pub(crate) fn with_storage<T: BackendOutput>(&self, operation: impl FnOnce(&Self) -> Result<T, BackendError> + BackendSend + 'static) -> Result<T, BackendError> {
        let service = self.clone();
        storage_queue::run_sync(Priority::Interactive, move || operation(&service)).map_err(BackendError::message)?
    }

    pub fn current_account(&self) -> Result<Option<Session>, BackendError> {
        self.state.account.current().map_err(crate::account::account_error)
    }

    pub fn account_identity(&self) -> Result<Option<String>, BackendError> {
        Ok(self.current_account()?.as_ref().map(session_identity))
    }

    pub fn bind_to_current_account(&self, id: &LibraryId) -> Result<(), BackendError> {
        if self.state.registry.device_only(id).map_err(BackendError::operation)? {
            return Ok(());
        }
        if let Some(account) = self.account_identity()? {
            self.state.registry.bind_library_account(id, &account).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    fn library_account(&self, id: LibraryId) -> LibraryAccount {
        let registry = self.state.registry.clone();
        self.state.account.clone().with_authorization(move |session| {
            if registry.device_only(&id).map_err(|error| std::io::Error::other(error.to_string()))? {
                return Ok(false);
            }
            let account = session_identity(session);
            if !registry.accessible_to_account(&id, Some(&account)).map_err(|error| std::io::Error::other(error.to_string()))? {
                return Ok(false);
            }
            registry.bind_library_account(&id, &account).map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(true)
        })
    }

    pub fn list(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.state.registry.entries_for_account(self.account_identity()?.as_deref()).map_err(BackendError::operation)
    }

    pub fn entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        if !self.state.registry.accessible_to_account(library_id, self.account_identity()?.as_deref()).map_err(BackendError::operation)? {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in to the account that owns this library"));
        }
        self.state.registry.entry(library_id).map_err(BackendError::operation)?.ok_or_else(|| BackendError::message(format!("library not found: {library_id}")))
    }

    /// Startup isolates unavailable libraries so other libraries can still open.
    pub fn open_registered(&self) -> Result<(), BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        self.state.catalog.open()?;
        for entry in self.list()? {
            if self.state.library_directory.try_get(entry.library_id())?.is_some() {
                continue;
            }
            let mut profile = StartupProfile::for_library("library_open", entry.library_id());
            if let Err(error) = self.open(entry.library_id()) {
                log::warn!("Could not open library {} during startup: {error}", entry.library_id());
            }
            profile.mark("open");
        }
        Ok(())
    }

    pub fn create_with_asset_storage(&self, name: String, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        let entry = self.register_with_asset_storage(&name, None, asset_storage_enabled)?;
        self.finish_created(entry)
    }

    /// Register first so a caller adding folders can report registration and open failures separately.
    pub fn register_with_asset_storage(&self, name: &str, locator: Option<String>, asset_storage_enabled: bool) -> Result<LibraryEntry, BackendError> {
        let storage = if asset_storage_enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local };
        self.register(name, locator, storage, LibraryKind::Standard)
    }

    fn register(&self, name: &str, locator: Option<String>, storage: LibraryStorage, kind: LibraryKind) -> Result<LibraryEntry, BackendError> {
        let name = name.to_owned();
        self.with_storage(move |this| {
            let previous = this.state.registry.entries().map_err(BackendError::operation)?;
            let device_only = !kind.is_opds() && !storage.uses_bokheim_assets();
            let entry = this.state.registry.create_with_kind(&name, locator, storage, kind).map_err(BackendError::operation)?;
            if device_only && !this.state.registry.has_account(entry.library_id()).map_err(BackendError::operation)? {
                this.state.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
            }
            this.bind_to_current_account(entry.library_id())?;
            if previous.iter().any(|old| old.library_id() == entry.library_id() && old.storage_locator() != entry.storage_locator()) {
                this.state.library_directory.retire(entry.library_id())?;
            }
            Ok(entry)
        })
    }

    /// The check and creation share one storage command to avoid duplicate import destinations.
    pub fn ensure_import_library(&self) -> Result<LibraryEntry, BackendError> {
        self.with_storage(move |this| {
            for entry in this.list()?.into_iter().filter(|entry| !entry.is_opds()) {
                if this.state.library_directory.try_get(entry.library_id())?.is_some() || this.open(entry.library_id()).is_ok() {
                    return Ok(entry);
                }
            }
            let enabled = this.current_account()?.is_some();
            let entry = this.register_with_asset_storage("Library", None, enabled)?;
            this.finish_created(entry)
        })
    }

    pub fn finish_created(&self, entry: LibraryEntry) -> Result<LibraryEntry, BackendError> {
        self.open(entry.library_id())?;
        self.state.library_list_changed.send_replace(());
        self.state.account_wake.send_replace(());
        Ok(entry)
    }

    /// Explicit eager open.  This is called only during startup and creation;
    /// ordinary lookups never open a library as a side effect.
    pub fn open(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        let mut profile = StartupProfile::for_library("library_dispatch", library_id);
        let library_id = *library_id;
        let result = self.with_storage(move |this| {
            profile.mark("storage_queue_wait");
            this.entry(&library_id)?;
            #[cfg(not(target_arch = "wasm32"))]
            let recovery = this.state.registry.recover_library_root(&library_id).map_err(BackendError::operation)?;
            let (spec, reset_token) = this.runtime_spec(library_id)?;
            profile.mark("registry_and_spec");
            #[cfg(not(target_arch = "wasm32"))]
            let opened = match recovery {
                library_registry::LibraryRootRecovery::Recovered(_) => this.state.library_directory.open_recovered(spec, &this.state.executor)?,
                library_registry::LibraryRootRecovery::Unchanged(_) => this.state.library_directory.open(spec, &this.state.executor)?,
            };
            #[cfg(target_arch = "wasm32")]
            let opened = this.state.library_directory.open(spec, &this.state.executor)?;
            if opened {
                profile.mark("directory_open");
                if let Some(token) = reset_token {
                    let reset_key = format!("cloud-storage-reset:{library_id}");
                    this.state.registry.clear_value_if_current(&reset_key, &token).map_err(BackendError::operation)?;
                }
                #[cfg(not(target_arch = "wasm32"))]
                this.state.registry.remember_library_root(&library_id).map_err(BackendError::operation)?;
            }
            profile.mark("registry_finalize");
            Ok(())
        });
        #[cfg(not(target_arch = "wasm32"))]
        if result.is_err() {
            let _ = self.state.root_failures.try_send(library_id);
        }
        result
    }

    /// Capture registry policy and the current account for a library-owned runtime.
    pub fn runtime_spec(&self, library_id: LibraryId) -> Result<(LibraryRuntimeSpec, Option<Vec<u8>>), BackendError> {
        let reset_key = format!("cloud-storage-reset:{library_id}");
        let reset_token = self.state.registry.read_value(&reset_key).map_err(BackendError::operation)?;
        let storage = self.state.registry.storage(&library_id).map_err(BackendError::operation)?;
        let entry = self.entry(&library_id)?;
        let library_path = entry.storage_locator();
        let display_name = entry.library_name().to_owned();
        Ok((
            LibraryRuntimeSpec {
                config: LibraryOpenConfig {
                    id: library_id,
                    display_name,
                    root: library_path.to_owned(),
                    database_locator: self.state.registry.library_database_locator(&library_id, library_path),
                    default_sync_server_url: self.current_account()?.map(|session| session.server_url().clone()).unwrap_or_else(|| self.state.sync_server.clone()),
                    metadata_server_url: self.state.metadata_server.clone(),
                    storage,
                    kind: entry.kind().clone(),
                    #[cfg(not(target_arch = "wasm32"))]
                    opds_credentials: if entry.is_opds() { self.state.catalog.credentials(library_id)? } else { None },
                    reset_cloud_presence: reset_token.is_some(),
                },
                account: self.library_account(library_id),
                notification_interest: self.state.notification_interest.clone(),
                #[cfg(not(target_arch = "wasm32"))]
                root_failures: Some(self.state.root_failures.clone()),
            },
            reset_token,
        ))
    }
}

fn session_identity(session: &Session) -> String {
    format!("{}|{}", session.server_url(), session.user_id())
}

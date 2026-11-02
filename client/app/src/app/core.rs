pub(super) use super::data::{BackendContext, LibraryEntry, LibraryRegistry};
#[cfg(not(target_arch = "wasm32"))]
use crate::executor::BackendTask;
use crate::executor::{BackendExecutor, BackendOutput, BackendSend};
use crate::host::startup_profile::StartupProfile;
use crate::AppStartupState;
use app_preferences::Service as PreferenceService;
use library_backend::account::{Service as AccountService, SessionStorage};
#[cfg(not(target_arch = "wasm32"))]
use library_backend::catalog_sources::{CatalogService, CatalogServiceConfig};
use library_backend::library::LibraryDirectory;
use library_backend::management::{LibraryService, LibraryServiceConfig};
pub(super) use std::path::{Path, PathBuf};
use std::sync::Arc;
pub(super) use sync_common::LibraryId;
use tokio::sync::watch;

pub(super) use crate::DeviceAccountSession;
pub(super) use crate::{BackendError, Failure};

pub(crate) struct AppState {
    pub(crate) app_storage_root: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) catalog: CatalogService,
    pub(crate) executor: BackendExecutor,
    pub(super) registry: LibraryRegistry,
    pub(crate) preferences: PreferenceService,
    pub(super) storage_copy_lock: Arc<tokio::sync::Mutex<()>>,
    pub(super) library_location_lock: Arc<tokio::sync::Mutex<()>>,
    /// Shared registration and opening policy for every library kind.
    pub(crate) library_service: LibraryService,
    /// Routing to the library-owned resource-bearing sessions.
    pub(super) library_directory: Arc<LibraryDirectory>,
    pub(super) account: DeviceAccountSession,
    pub(super) library_list_changed: watch::Sender<()>,
    /// Coalesces requests for a full account-maintenance pass.
    pub(super) account_wake: watch::Sender<()>,
    pub(crate) account_service: AccountService,
    #[cfg(not(target_arch = "wasm32"))]
    root_task: Option<BackendTask<()>>,
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.account_service.shutdown();
        #[cfg(not(target_arch = "wasm32"))]
        self.root_task.take();
        self.library_directory.shutdown();
    }
}

#[derive(Clone)]
pub(crate) struct AppBackend {
    pub(crate) state: Arc<AppState>,
}

impl AppBackend {
    /// The storage owner applies registry policy and the corresponding
    /// directory or account changes together, before publishing the result.
    pub(super) fn with_storage<T: BackendOutput>(&self, operation: impl FnOnce(&Self) -> Result<T, BackendError> + BackendSend + 'static) -> Result<T, BackendError> {
        use client_platform_runtime::storage_queue::{self, Priority};
        let this = self.clone();
        storage_queue::run_sync(Priority::Interactive, move || operation(&this)).map_err(BackendError::message)?
    }

    pub(crate) fn startup_state(&self) -> Result<AppStartupState, BackendError> {
        Ok(AppStartupState {
            default_library_root: self.state.registry.default_library_root().to_owned(),
            dictionary_storage_root: self.state.app_storage_root.join("dictionaries").to_string_lossy().into_owned(),
            preference_storage_root: self.state.preferences.storage_root().to_string_lossy().into_owned(),
            libraries: self.libraries().map_err(BackendError::operation)?,
            browsing_preferences: self.state.preferences.browsing(),
            account_status: self.state.account_service.account_status().map_err(BackendError::operation)?,
            reader_preferences: self.state.preferences.reader(),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn run_native_worker(self, worker: crate::runtime::native::AppWorker) {
        let executor = self.state.executor.clone();
        executor.block_on(worker.serve(self));
    }

    pub fn new(context: BackendContext) -> Result<Self, BackendError> {
        let core = Self::new_without_libraries(context)?;
        core.open_startup_libraries()?;
        Ok(core)
    }

    pub(crate) fn new_without_libraries(context: BackendContext) -> Result<Self, BackendError> {
        let account = DeviceAccountSession::load(SessionStorage::new(context.registry.clone()), crate::api_server_url()).map_err(BackendError::operation)?;
        Self::with_account(context, account)
    }

    pub(crate) fn with_account(context: BackendContext, account: DeviceAccountSession) -> Result<Self, BackendError> {
        let mut profile = StartupProfile::new("backend_construct");
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        profile.mark("executor");
        let notification_interest = crate::notification_interest::NotificationInterest::default();
        let library_directory = Arc::new(LibraryDirectory::default());
        let (account_wake, account_requests) = watch::channel(());
        profile.mark("account_load");
        #[cfg(not(target_arch = "wasm32"))]
        let (root_failures, root_requests) = async_channel::unbounded();
        let app_storage_root = context.app_data_dir.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let catalog = CatalogService::start(CatalogServiceConfig {
            registry: context.registry.clone(),
            libraries: library_directory.clone(),
            executor: executor.clone(),
            account: account.library_account().with_authorization(|_| Ok(false)),
            notification_interest: notification_interest.clone(),
            sync_server: crate::api_server_url(),
            metadata_server: crate::METADATA_SERVER_ORIGIN.to_owned(),
        });
        let library_list_changed = watch::channel(()).0;
        let storage_copy_lock = Arc::default();
        let library_location_lock = Arc::default();
        let library_service = LibraryService::new(LibraryServiceConfig {
            registry: context.registry.clone(),
            library_directory: library_directory.clone(),
            executor: executor.clone(),
            account: account.library_account(),
            notification_interest: notification_interest.clone(),
            sync_server: crate::api_server_url(),
            metadata_server: crate::METADATA_SERVER_ORIGIN.to_owned(),
            library_list_changed: library_list_changed.clone(),
            account_wake: account_wake.clone(),
            cloud_quota_lock: Arc::default(),
            library_location_lock: Arc::clone(&library_location_lock),
            storage_copy_lock: Arc::clone(&storage_copy_lock),
            #[cfg(not(target_arch = "wasm32"))]
            catalog: catalog.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            root_failures: root_failures.clone(),
        });
        let preferences = PreferenceService::start(context.preferences.clone(), &app_storage_root, &executor, Arc::new(library_service.clone())).map_err(BackendError::operation)?;
        profile.mark("preference_service");
        let account_service = AccountService::new(account.clone(), library_service.clone(), executor.clone(), account_requests);
        let state = Arc::new_cyclic(|_weak| AppState {
            app_storage_root,
            #[cfg(not(target_arch = "wasm32"))]
            catalog,
            account_service,
            #[cfg(not(target_arch = "wasm32"))]
            root_task: Some(super::root_recovery::start(&executor, _weak.clone(), root_requests)),
            executor: executor.clone(),
            registry: context.registry,
            preferences,
            storage_copy_lock,
            library_location_lock,
            library_service,
            library_directory,
            account: account.clone(),
            library_list_changed,
            account_wake,
        });
        let core = Self { state };
        if let Some(account) = core.state.library_service.account_identity()? {
            // Libraries created locally while signed out can now synchronize.
            // Legacy caches wait for discovery to establish their ownership.
            core.state.registry.bind_unowned_libraries(&account).map_err(BackendError::operation)?;
        }
        profile.mark("state_and_account_task");
        // Start the shared catalog service; libraries refresh when its snapshot is ready.
        library_backend::start_catalog(&context.app_data_dir, &executor, core.state.library_directory.clone());
        profile.mark("catalog_start");
        Ok(core)
    }

    pub(crate) fn open_startup_libraries(&self) -> Result<(), BackendError> {
        let mut profile = StartupProfile::new("library_startup");
        self.open_registered_libraries()?;
        profile.mark("libraries_open");
        self.start_pending_local_purges();
        self.state.account_wake.send_replace(());
        profile.mark("maintenance_schedule");
        Ok(())
    }

    pub(super) fn send_library_list_changed(&self) {
        self.state.library_list_changed.send_replace(());
    }

    /// Runtime routing may resolve an already-open actor here. It cannot open
    /// or configure libraries; that remains the explicit lifecycle path.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn library_directory(&self) -> &Arc<LibraryDirectory> {
        &self.state.library_directory
    }

    pub(super) fn library_entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        self.state.library_service.entry(library_id)
    }
}

pub(super) fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/core/library_client_lifecycle_tests.rs"]
mod library_client_lifecycle_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/account/storage_tests.rs"]
mod account_storage_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/quota/tests.rs"]
pub(super) mod quota_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/account/unavailable_library_tests.rs"]
mod unavailable_library_tests;

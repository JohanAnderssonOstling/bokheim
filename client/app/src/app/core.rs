use super::data::{BackendContext, LibraryRegistry};
use crate::executor::{BackendExecutor, BackendTask};
use crate::AppStartupState;
#[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
use app_preferences::BrowsingPreferences;
use app_preferences::PreferenceStore;
use client_platform_runtime::{text_search, transfers};
use cpu_host::CpuHost;
use library_management::account::{Service as AccountService, SessionStorage};
use library_management::management::{LibraryService, LibraryServiceConfig};
use library_services::interest::NotificationInterest;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::sync::{oneshot, watch};

use crate::BackendError;
pub(super) use crate::DeviceAccountSession;

/// Clients and accepted operations retain this owner through an `Arc`.
/// The final owner shuts down workers and releases the browser host.
pub(crate) struct AppBackend {
    pub(crate) app_storage_root: PathBuf,
    pub(crate) host_shutdown: OnceLock<oneshot::Sender<()>>,
    pub(super) registry: LibraryRegistry,
    pub(crate) preferences: Arc<PreferenceStore>,
    pub(crate) executor: BackendExecutor,
    _text_index_cleanup: BackendTask<()>,
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
    _transfer_preferences: BackendTask<()>,
    /// Shared registration and opening policy for every library kind.
    pub(crate) library_service: Arc<LibraryService>,
    /// Coalesces requests for a full account-maintenance pass.
    pub(super) account_wake: watch::Sender<()>,
    pub(crate) account_service: Arc<AccountService>,
    #[cfg(not(target_arch = "wasm32"))]
    root_task: Option<BackendTask<()>>,
}

impl Drop for AppBackend {
    fn drop(&mut self) {
        if let Some(stopped) = self.host_shutdown.take() {
            let _ = stopped.send(());
        }
        self.account_service.shutdown();
        #[cfg(not(target_arch = "wasm32"))]
        self.root_task.take();
        self.library_service.shutdown();
    }
}

impl AppBackend {
    pub(crate) async fn startup_state(&self) -> Result<AppStartupState, BackendError> {
        Ok(AppStartupState {
            default_library_root: self.registry.default_library_root().to_owned(),
            dictionary_storage_root: self.app_storage_root.join("dictionaries").to_string_lossy().into_owned(),
            preference_storage_root: self.preferences.storage_root().to_string_lossy().into_owned(),
            libraries: self.library_service.list().await.map_err(BackendError::operation)?,
            browsing_preferences: self.preferences.browsing(),
            account_status: self.account_service.account_status().await.map_err(BackendError::operation)?,
            reader_preferences: self.preferences.reader(),
        })
    }

    pub async fn new(context: BackendContext) -> Result<Arc<Self>, BackendError> {
        let account = DeviceAccountSession::load(SessionStorage::new(context.registry.clone()), crate::api_server_url()).map_err(BackendError::operation)?;
        let core = Self::with_account(context, account).await?;
        core.library_service.open_registered().await?;
        core.library_service.start_pending_local_purges();
        core.account_wake.send_replace(());
        Ok(core)
    }

    pub(crate) async fn with_account(context: BackendContext, account: DeviceAccountSession) -> Result<Arc<Self>, BackendError> {
        let executor = context.executor.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let cpu: Arc<dyn CpuHost> = Arc::new(cpu_host::NativeHost);
        #[cfg(target_arch = "wasm32")]
        let cpu: Arc<dyn CpuHost> = Arc::new(client_platform_web::cpu_host::WebHost);
        let (account_wake, account_requests) = watch::channel(());
        #[cfg(not(target_arch = "wasm32"))]
        let (root_failures, root_requests) = async_channel::unbounded();
        let app_storage_root = context.app_data_dir;
        let library_list_changed = watch::channel(()).0;
        let library_service = LibraryService::new(LibraryServiceConfig {
            registry: context.registry.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            preference_storage_root: context.preferences.storage_root().to_owned(),
            executor: executor.clone(),
            cpu,
            account: account.library_account(),
            notification_interest: NotificationInterest::default(),
            sync_server: crate::api_server_url(),
            metadata_server: crate::METADATA_SERVER_ORIGIN.to_owned(),
            library_list_changed,
            account_wake: account_wake.clone(),
            cloud_quota_lock: Arc::default(),
            library_location_lock: Arc::default(),
            storage_copy_lock: Arc::default(),
            #[cfg(not(target_arch = "wasm32"))]
            root_failures,
        });
        let preferences = context.preferences;
        text_search::set_enabled(preferences.browsing().text_search());
        transfers::POLICY.set_allow_metered(preferences.browsing().allow_metered_transfers());
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        let transfer_preferences = transfers::mirror_allow_metered(&app_storage_root, preferences.subscribe_browsing(), BrowsingPreferences::allow_metered_transfers, &executor).map_err(BackendError::operation)?;
        let mut text_search_changes = text_search::subscribe();
        let libraries = library_service.clone();
        let text_index_cleanup = executor.spawn(async move {
            loop {
                text_search_changes.borrow_and_update();
                if !text_search::enabled() {
                    if let Err(error) = libraries.cleanup_text_search_indexes().await {
                        log::warn!("could not finish removing text search indexes: {error}");
                    }
                }
                if text_search_changes.changed().await.is_err() {
                    return;
                }
            }
        });
        let account_service = AccountService::new(account, library_service.clone(), executor.clone(), account_requests);
        #[cfg(not(target_arch = "wasm32"))]
        let root_task = library_service.start_root_recovery(root_requests);
        let core = Arc::new(Self {
            app_storage_root,
            host_shutdown: OnceLock::new(),
            account_service,
            #[cfg(not(target_arch = "wasm32"))]
            root_task: Some(root_task),
            registry: context.registry,
            preferences,
            executor: executor.clone(),
            _text_index_cleanup: text_index_cleanup,
            #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
            _transfer_preferences: transfer_preferences,
            library_service,
            account_wake,
        });
        if let Some(account) = core.library_service.account_identity().await? {
            // Libraries created locally while signed out can now synchronize.
            // Legacy caches wait for discovery to establish their ownership.
            core.registry.access(move |registry| registry.bind_unowned_libraries(&account)).await.map_err(BackendError::operation)?;
        }
        // Start the shared catalog service; libraries refresh when its snapshot is ready.
        library_management::start_catalog(&core.app_storage_root, &executor, &core.library_service);
        Ok(core)
    }
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/libraries/folder_add_tests.rs"]
mod folder_add_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/root_recovery/tests.rs"]
mod root_recovery_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/storage_copy/tests.rs"]
mod storage_copy_tests;

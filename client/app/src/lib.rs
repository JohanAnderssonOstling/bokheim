//! Application clients, account state, and startup configuration.
//! Platform launchers and worker entry points use [`host`].
pub mod host;

mod api;
mod app;
use client_platform_runtime::executor;
pub use library_management::management::{LibraryFolderError, LibraryFolderStep, NotificationLease};
pub(crate) mod hosting;

pub use app::{API_SERVER_ORIGIN, METADATA_SERVER_ORIGIN, Url, api_server_url, metadata_server_url};
pub use app::{APP_NAME, APP_NAME_LOWER};
pub use app::{AccountSnapshot, AppClient, AppStartupState};
pub use app::{AccountStatus, AppDataLocation, LibraryEntry, ServerBookStorageUsage, ServerLibraryStorageUsage};
pub use client_protocol::{BackendError, Classified, Failure, WifiFailure};
pub(crate) use library_management::account::AccountSession as DeviceAccountSession;
pub use library_management::management::LocalLibraryStorageUsage;
pub use sync_common::storage::LibraryStorage;

/// Initialize the logging system. Call this once at the start of your application.
///
/// Example:
/// ```no_run
/// app::init_logging();
/// ```
pub fn init_logging() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Warn)
        .filter_module("wgpu", log::LevelFilter::Warn)
        .filter_module("wgpu_core", log::LevelFilter::Warn)
        .filter_module("wgpu_hal", log::LevelFilter::Warn)
        .filter_module("zbus", log::LevelFilter::Warn)
        .filter_module("ashpd", log::LevelFilter::Warn)
        .init();
    log::info!("Logging initialized");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use api::BackendLaunch;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use app::BackendContext;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(test)]
#[path = "../tests/library_folder_error/tests.rs"]
mod library_folder_error_tests;

//! Frontend API: application/library clients, view data, and reader capabilities.
//! Platform launchers and worker entry points use [`host`].
extern crate core;

pub mod host;

mod api;
mod app;
use client_platform_runtime::executor;
mod backend_error;
mod import_journal;
mod import_status;
mod import_workflow;
pub(crate) use library_runtime::interest as notification_interest;
mod staged_import;
#[cfg(target_arch = "wasm32")]
use publication_access::range_cache;
#[cfg(target_arch = "wasm32")]
mod browser_import;
#[cfg(test)]
pub(crate) use app::data as home;
#[cfg(all(feature = "storage-contract-tests", target_arch = "wasm32"))]
mod contracts;
mod library;
mod platform;
pub(crate) mod runtime;
mod storage;
mod sync;
mod worker_cpu;
#[cfg(target_arch = "wasm32")]
mod worker_sync;

#[cfg(test)]
#[path = "../tests/convergence.rs"]
mod convergence_tests;

pub use app::{API_SERVER_ORIGIN, METADATA_SERVER_ORIGIN, ServerUrl, ServerUrlError, api_server_url, metadata_server_url};
pub use app::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_NAME, APP_NAME_LOWER};
pub use app::{AccountSnapshot, AppClient, AppStartupState, LibraryClient, ThumbnailResolution};
pub use app::{AccountStatus, AccountStorageUsage, AppDataLocation, LibraryEntry, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
#[cfg(test)]
pub(crate) use app::{AppWorkerRequest, WorkerDispatchResult, WorkerPayload, decode_worker_message, encode_worker_message};
pub use backend_error::BackendError;
pub(crate) type DeviceAccountSession = library_runtime::account::LibraryAccountSession<app::BackendContext>;
pub use client_library::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
pub use library::{
    BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReader, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, ImportSource, LibraryBrowseData,
    LibrarySyncState, LibrarySyncStatus,
};
pub use library_model::{AgentId, AuthorId, FolderDestinationPolicy};
pub use library_model::{EpubCfi, ReadProgress};
pub use library_replica::ScanProgress;
pub use library_replica::{BlobKey, BlobKind, TransferJobKind};
pub use sync::transfer::{LibraryTransfers, TransferOrigin, TransferState, TransferStatus};
pub use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};

#[cfg(target_arch = "wasm32")]
pub(crate) fn web_diagnostic(message: impl AsRef<str>) {
    let message = message.as_ref();
    web_sys::console::info_1(&wasm_bindgen::JsValue::from_str(message));
    WEB_DIAGNOSTICS.with(|subscribers| {
        subscribers.borrow_mut().retain(|subscriber| subscriber.try_send(message.to_owned()).is_ok());
    });
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WEB_DIAGNOSTICS: std::cell::RefCell<Vec<async_channel::Sender<String>>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn web_diagnostic(_message: impl AsRef<str>) {}

#[cfg(target_arch = "wasm32")]
pub(crate) use platform::web_storage::initialize as initialize_web_storage;

/// Initialize the logging system. Call this once at the start of your application.
///
/// Example:
/// ```no_run
/// bokheim_backend::init_logging();
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
#[cfg(test)]
pub(crate) use app::{AppBackend, BackendContext};
pub(crate) use library::LibrarySession;

#[cfg(all(test, feature = "scanner", not(target_arch = "wasm32"), not(target_os = "android")))]
#[path = "../tests/scan.rs"]
mod scan_tests;

#[cfg(target_arch = "wasm32")]
mod browser_adapter;

pub use api::library::EnrichmentStatus;

pub use import_status::{ImportFailure, ImportFailureKind, ImportProgress, ImportState};
#[cfg(not(target_arch = "wasm32"))]
pub use library::AudioStreamControl;
#[cfg(target_arch = "wasm32")]
pub use library::browser_audiobook::{BrowserAudiobook, PlaybackLease};
pub use library::{AudiobookBook, DirectPlayback, DocumentBook, PlaybackLocation, PlaybackMetadata, PlaybackSource, ResolvedBook};
#[cfg(any(test, target_arch = "wasm32"))]
#[path = "browser_adapter/import_progress.rs"]
mod import_progress_wire;
#[cfg(all(feature = "kobo", not(target_arch = "wasm32")))]
pub use api::app::BatteryStatus;
#[cfg(all(feature = "kobo", target_os = "linux"))]
pub use api::app::{BluetoothDevice, BluetoothRequest, BluetoothStatus, WifiNetwork, WifiPhase, WifiRequest, WifiSecurity, WifiSnapshot};

#[cfg(all(test, not(target_arch = "wasm32")))]
pub use library_replica::{LibraryOperation, LibraryOperationKind, LibraryOperationState};

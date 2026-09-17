//! Frontend API: application/library clients, view data, and reader capabilities.
//! Platform launchers and worker entry points use [`host`].
extern crate core;

pub mod host;

mod api;
mod app;
use client_platform_runtime::executor;
pub(crate) use library_runtime::interest as notification_interest;
#[cfg(target_arch = "wasm32")]
use book_access::range_cache;
#[cfg(target_arch = "wasm32")]
mod browser_import;
#[cfg(test)]
pub(crate) use app::data as home;
#[cfg(all(feature = "storage-contract-tests", target_arch = "wasm32"))]
mod contracts;
mod platform;
pub(crate) mod runtime;

#[cfg(test)]
#[path = "../tests/convergence.rs"]
mod convergence_tests;

pub use app::{API_SERVER_ORIGIN, METADATA_SERVER_ORIGIN, ServerUrl, ServerUrlError, api_server_url, metadata_server_url};
pub use app::{APP_NAME, APP_NAME_LOWER};
pub use app::{AccountSnapshot, AppClient, AppStartupState, LibraryClient, ThumbnailResolution};
pub use app::{AccountStatus, AccountStorageUsage, AppDataLocation, LibraryEntry, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
#[cfg(test)]
pub(crate) use app::{AppWorkerRequest, WorkerDispatchResult, WorkerPayload, decode_worker_message, encode_worker_message};
pub use library_backend::{
    BlobKey, BlobKind, BookDownloadStatus, BookPlacement, BookReader, BoxedBookReader,
    ContentHash, DirId, DirectoryImport, DirectoryImportFile, DirectoryImportReader,
    DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState,
    ImportFailure, ImportFailureKind, ImportProgress, ImportSource, ImportState,
    LibraryBrowseData, LibraryCommand, LibraryHandle, LibraryId, LibrarySession,
    LibrarySyncState, LibrarySyncStatus,
    LibraryTransfers, BookSource, RelativeBookPath, RelativeBookPathError,
    RelativeDirPath, RelativeDirPathError, ResolvedBookData, ROOT_DIR_ID, ScanProgress,
    TransferJobKind, TransferOrigin, TransferState,
    TransferStatus,
};
/// Compatibility exports for clients that still import library-owned modules
/// through the application crate. New code should depend on `library-backend`.
pub(crate) use library_backend::{asset_store, import_journal, import_workflow, library, staged_import, sync, worker_cpu};
pub use library_runtime::BackendError;
pub(crate) type DeviceAccountSession = library_runtime::account::LibraryAccountSession<app::BackendContext>;
pub use library_replica::FolderDestinationPolicy;
pub use book_model::{AgentId, AuthorId};
pub use book_model::{EpubCfi, ReadProgress};

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

#[cfg(all(test, not(target_arch = "wasm32"), not(target_os = "android")))]
#[path = "../tests/scan.rs"]
mod scan_tests;

#[cfg(target_arch = "wasm32")]
mod web;

pub use api::library::EnrichmentStatus;

#[cfg(not(target_arch = "wasm32"))]
pub use library::AudioStreamControl;
#[cfg(target_arch = "wasm32")]
pub use library_backend::library::browser_audiobook::{BrowserAudiobook, PlaybackLease};
pub use library_backend::library::{AudiobookBook, DirectPlayback, DocumentBook, PlaybackLocation, PlaybackMetadata, PlaybackSource, ResolvedBook};
#[cfg(any(test, target_arch = "wasm32"))]
#[path = "web/import_progress.rs"]
mod import_progress_wire;
#[cfg(all(feature = "kobo", not(target_arch = "wasm32")))]
pub use api::app::BatteryStatus;
#[cfg(all(feature = "kobo", target_os = "linux"))]
pub use api::app::{BluetoothDevice, BluetoothRequest, BluetoothStatus, WifiNetwork, WifiPhase, WifiRequest, WifiSecurity, WifiSnapshot};

pub use library_replica::{LibraryOperation, LibraryOperationKind, LibraryOperationState};

extern crate core;

pub mod app;
pub(crate) use app::account as auth;
pub(crate) use app::data as home;
pub mod catalog;
pub mod library;
pub(crate) use library::database as db;
pub mod platform;
mod sync;

#[cfg(test)]
#[path = "../tests/convergence.rs"]
mod convergence_tests;

pub use app::{api_server_url, catalog_server_url, ServerUrl, ServerUrlError, API_SERVER_ORIGIN, CATALOG_SERVER_ORIGIN};
pub use app::{AppBackend, AppDataLocation, BackendContext, BackendError, LibraryEntry};
pub use app::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_HIDDEN_SESSION_PATH, APP_HIDDEN_THUMBNAILS_PATH, APP_NAME, APP_NAME_LOWER};
pub use app_contract::download::{BookDownloadStatus, DownloadProgress, DownloadState, FolderDownloadStatus};
pub use app_contract::frontend::*;
pub use domain::{BookLocator, BookPlacement, EpubCfi, ReadProgress, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
pub(crate) use library_files::LibraryRoot;
pub use library::LibraryBackend;
pub use sync::blob::{BlobKey, BlobKind, TransferJobKind};
pub use sync::transfer::{LibraryTransfers, NetworkStatus, NetworkTransport, TransferOrigin, TransferState, TransferStatus};
pub use sync_common::{AuthorId, ContentHash, DirId, LibraryId};

/// Initialize the logging system. Call this once at the start of your application.
///
/// Example:
/// ```no_run
/// epub_svelte_lib::init_logging();
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

#[cfg(test)]
mod domain_tests {
    use super::{ContentHash};

    #[test]
    fn full_content_hash_is_the_book_identity() {
        let digest = blake3::hash(b"typed identity regression");
        let content_hash = ContentHash::new(&digest.to_hex().as_str());
        assert_eq!(content_hash.as_str(), digest.to_hex().as_str());
    }
}

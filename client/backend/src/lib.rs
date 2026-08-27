extern crate core;

pub mod app;
#[cfg(test)]
pub(crate) use app::data as home;
pub use catalog_model as catalog;
pub mod library;
#[cfg(test)]
pub(crate) use library::database as db;
pub mod platform;
mod sync;

#[cfg(test)]
#[path = "../tests/convergence.rs"]
mod convergence_tests;

pub use app::{api_server_url, catalog_server_url, metadata_server_url, taxonomy_server_url, ServerUrl, ServerUrlError, API_SERVER_ORIGIN, CATALOG_SERVER_ORIGIN, METADATA_SERVER_ORIGIN, TAXONOMY_SERVER_ORIGIN};
pub use app::{
    AccountStatus, AccountStorageUsage, AppBackend, AppDataLocation, BackendContext, BackendError, BrowserSession, CatalogPublicationKey, CatalogSubjectBrowseView, LibraryEntry, LibraryPlace, LibrarySyncState, LibrarySyncStatus,
    LocalLibraryStorageUsage, NavigationCrumb, ServerLibraryStorageUsage,
};
pub use app::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_HIDDEN_THUMBNAILS_PATH, APP_NAME, APP_NAME_LOWER};
pub use library::{BookDownloadStatus, BookReader, BoxedBookReader, DirectoryImport, DirectoryImportFile, DirectoryImportReaderFuture, DownloadProgress, DownloadProgressError, DownloadState, FolderDownloadStatus, LibraryBackend};
pub use library_model::{AgentId, AuthorId, FolderDestinationPolicy};
pub use library_model::{EpubCfi, ReadProgress};
pub use library_replica::{BlobKey, BlobKind, TransferJobKind};
pub use library_sqlite::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
pub use sync::transfer::{LibraryTransfers, NetworkStatus, NetworkTransport, TransferOrigin, TransferState, TransferStatus};
pub use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};

#[cfg(target_arch = "wasm32")]
pub use platform::web_storage::initialize as initialize_web_storage;

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

#[cfg(test)]
mod domain_tests {
    use super::ContentHash;

    #[test]
    fn full_content_hash_is_the_book_identity() {
        let digest = blake3::hash(b"typed identity regression");
        let content_hash = ContentHash::new(&digest.to_hex().as_str());
        assert_eq!(content_hash.as_str(), digest.to_hex().as_str());
    }
}

extern crate core;

pub mod app;
pub(crate) use app::account as auth;
pub mod catalog;
pub mod library;
mod paths;
pub mod platform;
mod storage;
mod sync;

#[cfg(test)]
#[path = "../tests/convergence.rs"]
mod convergence_tests;

pub use app::{api_server_url, catalog_server_url, ServerUrl, ServerUrlError, API_SERVER_ORIGIN, CATALOG_SERVER_ORIGIN};
pub use app::{AppBackend, AppDataLocation, BackendContext, BackendError, LibraryEntry};
pub use app::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_HIDDEN_SESSION_PATH, APP_HIDDEN_THUMBNAILS_PATH, APP_NAME, APP_NAME_LOWER};
pub use bokheim_app_contract::download::{BookDownloadStatus, DownloadProgress, DownloadState, FolderDownloadStatus};
pub use bokheim_app_contract::frontend::*;
pub use bokheim_domain::{BookLocator, EpubCfi, ReadProgress};
pub use library::LibraryBackend;
pub use paths::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
pub use sync::blob::{BlobKey, BlobKind, TransferJobKind};
pub use sync::transfer::{LibraryTransfers, NetworkStatus, NetworkTransport, TransferOrigin, TransferState, TransferStatus};
pub use sync_common::{AuthorId, BookId, ContentHash, DirId, LibraryId};

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
    use super::{BookId, ContentHash};

    #[test]
    fn book_id_derives_from_the_first_eight_blake3_bytes() {
        let digest = blake3::hash(b"typed identity regression");
        let content_hash = ContentHash::parse(digest.to_hex().as_ref()).unwrap();
        let mut expected_bytes = [0_u8; 8];
        expected_bytes.copy_from_slice(&digest.as_bytes()[..8]);

        assert_eq!(BookId::from_content_hash(&content_hash), BookId::new(u64::from_le_bytes(expected_bytes)));
    }

    #[test]
    fn book_id_rejects_synthetic_and_malformed_content_keys() {
        assert!(ContentHash::parse("remote:d1:b1:f.epub").is_err());
        assert!(ContentHash::parse(&"z".repeat(64)).is_err());
    }

    #[test]
    fn library_event_keeps_its_existing_string_id_wire_format() {
        let value = serde_json::to_value(crate::platform::scanner::LibraryEvent::BookOpened { book_id: BookId::new(42) }).unwrap();
        assert_eq!(value["BookOpened"]["book_id"], serde_json::json!("42"));
    }
}

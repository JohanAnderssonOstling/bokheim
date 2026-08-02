extern crate core;

pub mod app_core;
pub mod app_name;
pub mod auth;
mod blob;
mod db;
mod download;
pub mod epub;
pub mod frontend_contract;
pub mod home;
pub mod library;
pub mod opds;
mod path_types;
pub mod preferences;
mod reading;
#[cfg(all(feature = "desktop", not(target_os = "android")))]
pub mod scanner;
#[cfg(not(all(feature = "desktop", not(target_os = "android"))))]
#[path = "scanner_stub.rs"]
pub mod scanner;
mod server_url;
mod sync;
mod transfer;

#[cfg(test)]
#[path = "../tests/convergence.rs"]
mod convergence_tests;

pub use app_name::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_HIDDEN_SESSION_PATH, APP_HIDDEN_THUMBNAILS_PATH, APP_NAME, APP_NAME_LOWER};
pub use blob::{BlobKey, BlobKind, TransferJobKind};
pub use download::{BookDownloadStatus, DownloadProgress, DownloadState, FolderDownloadStatus};
pub use path_types::{BookPlacement, RelativeBookPath, RelativeDirPath, RelativeDirPathError};
pub use reading::{EpubCfi, ReadProgress};
pub use server_url::{api_server_url, ServerUrl, ServerUrlError, API_SERVER_ORIGIN};
pub use sync_common::{BookId, ContentHash, DirId, LibraryId};
pub use transfer::{LibraryTransfers, NetworkStatus, NetworkTransport, TransferOrigin, TransferState, TransferStatus};

/// Stable identity for a book within one configured library.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BookLocator {
    library_id: LibraryId,
    book_id: BookId,
}

impl BookLocator {
    pub fn new(library_id: LibraryId, book_id: BookId) -> Self {
        Self { library_id, book_id }
    }

    pub fn parse(library_id: &str, book_id: BookId) -> Result<Self, sync_common::DomainTextError> {
        Ok(Self::new(LibraryId::parse(library_id)?, book_id))
    }

    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn book_id(&self) -> BookId {
        self.book_id
    }
}

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
        let value = serde_json::to_value(crate::scanner::LibraryEvent::BookOpened { book_id: BookId::new(42) }).unwrap();
        assert_eq!(value["BookOpened"]["book_id"], serde_json::json!("42"));
    }
}

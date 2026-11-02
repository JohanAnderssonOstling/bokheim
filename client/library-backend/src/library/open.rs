//! Library-owned opening configuration.

#[cfg(not(target_arch = "wasm32"))]
use crate::BackendError;
use account_client::Url;
#[cfg(not(target_arch = "wasm32"))]
use library_model::{OpdsCredentials, OpdsSource};
use sync_common::storage::{LibraryKind, LibraryStorage};
use sync_common::LibraryId;

/// Application policy captured at library-open time.
///
/// The library runtime receives values, not access to an application object or
/// process-global configuration. Live services are injected separately by the
/// host that constructs the session.
#[derive(Clone, Debug)]
pub struct LibraryOpenConfig {
    pub id: LibraryId,
    pub display_name: String,
    pub root: String,
    pub database_locator: String,
    pub default_sync_server_url: Url,
    pub metadata_server_url: String,
    pub storage: LibraryStorage,
    /// The backend constructs source-specific resources from this value.
    pub kind: LibraryKind,
    pub reset_cloud_presence: bool,
    #[cfg(not(target_arch = "wasm32"))]
    /// Device-local credentials, used only for an OPDS source.
    pub opds_credentials: Option<OpdsCredentials>,
}

impl LibraryOpenConfig {
    /// Describe a local catalog feed using backend-owned cache locations.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn catalog_feed(source: OpdsSource, database_locator: String, credentials: Option<OpdsCredentials>, default_sync_server_url: Url, metadata_server_url: String) -> Result<Self, BackendError> {
        use std::path::Path;

        let root = Path::new(&database_locator).parent().ok_or("catalog feed database has no directory")?.join("catalog-books");
        Ok(Self {
            id: source.id,
            display_name: source.name,
            root: root.to_string_lossy().into_owned(),
            database_locator,
            default_sync_server_url,
            metadata_server_url,
            storage: LibraryStorage::Local,
            kind: LibraryKind::Opds { url: Url::parse(&source.url).map_err(BackendError::operation)? },
            reset_cloud_presence: false,
            opds_credentials: credentials,
        })
    }
}

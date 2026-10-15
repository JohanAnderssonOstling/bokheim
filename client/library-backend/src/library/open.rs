//! Library-owned opening configuration.

use account_client::Url;
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
    pub google_books_api_key: Option<String>,
    pub asset_storage_enabled: bool,
    pub reset_cloud_presence: bool,
}

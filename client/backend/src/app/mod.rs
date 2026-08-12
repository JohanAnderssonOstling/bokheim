//! Device-global application services and library lifecycle management.

pub mod account;
mod backend;
pub(crate) mod data;
pub mod executor;
pub mod name;
pub mod preferences;
mod server_url;

pub(crate) use backend::AppState;
pub use backend::{
    AccountSessionSummary, AccountStatus, AccountStorageUsage, AppBackend, AppEvent, BackendError, CatalogAuthorDiscoveryView, CatalogPageView, CatalogWideSearchView, LibraryCatalogueState, LibrarySyncState, LibrarySyncStatus,
    LibrarySyncSummary, LocalLibraryStorageUsage, ServerLibraryStorageUsage,
};
pub(crate) use data::read_library_manifest;
pub use data::{AppDataLocation, BackendContext, LibraryEntry, LIBRARY_MANIFEST_FILENAME};
pub use name::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_HIDDEN_SESSION_PATH, APP_HIDDEN_THUMBNAILS_PATH, APP_NAME, APP_NAME_LOWER};
pub use server_url::{api_server_url, catalog_server_url, ServerUrl, ServerUrlError, API_SERVER_ORIGIN, CATALOG_SERVER_ORIGIN};

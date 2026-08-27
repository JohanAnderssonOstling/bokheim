//! Device-global application services and library lifecycle management.

mod account_models;
mod backend;
mod browser_session;
pub(crate) mod data;
pub mod executor;
mod librarything;
pub mod name;
pub mod preferences;
mod server_url;

pub use account_models::{AccountStatus, AccountStorageUsage, LibrarySyncState, LibrarySyncStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
pub(crate) use backend::AppState;
pub use backend::{AccountSessionSummary, AppBackend, AppEvent, BackendError, CatalogPageView, CatalogPublicationKey, CatalogSubjectBrowseView, LibraryListState};
pub use browser_session::{BrowserSession, LibraryPlace, NavigationCrumb};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use data::read_library_manifest;
#[cfg(not(target_arch = "wasm32"))]
pub use data::LIBRARY_MANIFEST_FILENAME;
pub use data::{AppDataLocation, BackendContext, LibraryEntry};
pub use name::{APP_HIDDEN_DIR, APP_HIDDEN_LIBRARY_DB_PATH, APP_HIDDEN_THUMBNAILS_PATH, APP_NAME, APP_NAME_LOWER};
pub use server_url::{api_server_url, catalog_server_url, metadata_server_url, taxonomy_server_url, ServerUrl, ServerUrlError, API_SERVER_ORIGIN, CATALOG_SERVER_ORIGIN, METADATA_SERVER_ORIGIN, TAXONOMY_SERVER_ORIGIN};

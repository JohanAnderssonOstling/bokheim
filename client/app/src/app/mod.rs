//! Application startup and assembly of shared domain services.

pub(crate) mod core;
pub(crate) mod data;

pub use account_client::{API_SERVER_ORIGIN, Url, api_server_url};

pub const APP_NAME: &str = "Bokheim";
pub const APP_NAME_LOWER: &str = "bokheim";

pub const METADATA_SERVER_ORIGIN: &str = "https://meta.bokheim.se";

pub use crate::api::AppClient;
pub use crate::hosting::{AccountSnapshot, AppStartupState};
pub(crate) use core::AppBackend;
pub(crate) use data::BackendContext;
pub use data::{AppDataLocation, LibraryEntry};
pub use library_management::account::{AccountStatus, ServerBookStorageUsage, ServerLibraryStorageUsage};

pub fn metadata_server_url() -> Url {
    Url::parse(METADATA_SERVER_ORIGIN).expect("Bokheim metadata origin is a valid URL")
}

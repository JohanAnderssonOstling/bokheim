//! Device-global application services and library lifecycle management.

mod core;
pub(crate) mod data;
mod libraries;
#[cfg(not(target_arch = "wasm32"))]
mod root_recovery;
mod storage_copy;

pub use account_client::{api_server_url, Url, API_SERVER_ORIGIN};

pub const APP_NAME: &str = "Bokheim";
pub const APP_NAME_LOWER: &str = "bokheim";

pub const METADATA_SERVER_ORIGIN: &str = "https://meta.bokheim.se";

pub use crate::api::AppClient;
pub use crate::runtime::{AccountSnapshot, AppStartupState};
pub(crate) use core::AppBackend;
pub(crate) use data::BackendContext;
pub use data::{AppDataLocation, LibraryEntry};
pub use library_backend::account::{AccountStatus, ServerBookStorageUsage, ServerLibraryStorageUsage};

pub fn metadata_server_url() -> Url {
    Url::parse(METADATA_SERVER_ORIGIN).expect("Bokheim metadata origin is a valid URL")
}

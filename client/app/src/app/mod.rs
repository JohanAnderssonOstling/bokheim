//! Device-global application services and library lifecycle management.

mod account;
mod catalog;
mod core;
pub(crate) mod data;
mod libraries;
#[cfg(not(target_arch = "wasm32"))]
mod opds;
pub mod preferences;
#[cfg(not(target_arch = "wasm32"))]
mod stores;

pub use account_client::Url;

pub const APP_NAME: &str = "Bokheim";
pub const APP_NAME_LOWER: &str = "bokheim";

pub const API_SERVER_ORIGIN: &str = "https://api.bokheim.se";
pub const METADATA_SERVER_ORIGIN: &str = "https://meta.bokheim.se";

fn google_books_api_key() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(key) = std::env::var("BOKHEIM_GOOGLE_BOOKS_API_KEY") {
        if !key.trim().is_empty() { return Some(key); }
    }
    option_env!("BOKHEIM_GOOGLE_BOOKS_API_KEY").filter(|key| !key.trim().is_empty()).map(str::to_owned)
}

pub use crate::api::AppClient;
pub use crate::runtime::{AccountSnapshot, AppStartupState};
pub use account::{AccountStatus, LocalLibraryStorageUsage, ServerLibraryStorageUsage};
pub(crate) use core::AppBackend;
pub(crate) use data::BackendContext;
pub use data::{AppDataLocation, LibraryEntry};

pub fn api_server_url() -> Url {
    Url::parse(API_SERVER_ORIGIN).expect("Bokheim API origin is a valid URL")
}

pub fn metadata_server_url() -> Url {
    Url::parse(METADATA_SERVER_ORIGIN).expect("Bokheim metadata origin is a valid URL")
}

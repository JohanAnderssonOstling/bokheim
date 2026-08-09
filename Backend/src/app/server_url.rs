//! Canonical remote service origins.

pub use bokheim_sync_client::{ServerUrl, ServerUrlError};

pub const API_SERVER_ORIGIN: &str = "https://api.bokheim.se";
pub const CATALOG_SERVER_ORIGIN: &str = "https://catalogs.bokheim.se";

pub fn api_server_url() -> ServerUrl {
    ServerUrl::parse(API_SERVER_ORIGIN).expect("Bokheim API origin is a valid HTTPS origin")
}

pub fn catalog_server_url() -> ServerUrl {
    ServerUrl::parse(CATALOG_SERVER_ORIGIN).expect("Bokheim catalog origin is a valid HTTPS origin")
}

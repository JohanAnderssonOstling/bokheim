//! Canonical remote service origins.

pub use sync_client::{ServerUrl, ServerUrlError};

pub const API_SERVER_ORIGIN: &str = "https://api.bokheim.se";
pub const CATALOG_SERVER_ORIGIN: &str = "https://catalogs.bokheim.se";
pub const METADATA_SERVER_ORIGIN: &str = "https://meta.bokheim.se";
pub const TAXONOMY_SERVER_ORIGIN: &str = "https://taxonomy.bokheim.se";

pub fn api_server_url() -> ServerUrl {
    ServerUrl::parse(API_SERVER_ORIGIN).expect("Bokheim API origin is a valid HTTPS origin")
}

pub fn catalog_server_url() -> ServerUrl {
    ServerUrl::parse(CATALOG_SERVER_ORIGIN).expect("Bokheim catalog origin is a valid HTTPS origin")
}

pub fn metadata_server_url() -> ServerUrl {
    ServerUrl::parse(METADATA_SERVER_ORIGIN).expect("Bokheim metadata origin is a valid HTTPS origin")
}

pub fn taxonomy_server_url() -> ServerUrl {
    ServerUrl::parse(TAXONOMY_SERVER_ORIGIN).expect("Bokheim taxonomy origin is a valid HTTPS origin")
}

//! Device-local OPDS subscriptions, credentials, and their library runtimes.
use crate::BackendError;
use library_model::{AddOpdsFeedResult, OpdsCredentials, OpdsSource, OpdsSourceId};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{CatalogService, CatalogServiceConfig};

/// A frontend capability connecting directly to the catalog service for this profile.
#[derive(Clone)]
pub struct CatalogClient {
    #[cfg(not(target_arch = "wasm32"))]
    service: CatalogService,
    #[cfg(not(target_arch = "wasm32"))]
    _host: Arc<dyn Send + Sync>,
}

#[cfg(target_arch = "wasm32")]
impl CatalogClient {
    pub(crate) fn new() -> Self {
        Self {}
    }

    pub async fn list(&self) -> Result<Vec<OpdsSource>, BackendError> {
        Ok(Vec::new())
    }
    pub async fn add(&self, _url: String, _name: Option<String>, _credentials: Option<OpdsCredentials>) -> Result<AddOpdsFeedResult, BackendError> {
        Err(unavailable())
    }
    pub async fn remove(&self, _id: OpdsSourceId) -> Result<(), BackendError> {
        Err(unavailable())
    }
    pub async fn set_credentials(&self, _id: OpdsSourceId, _credentials: OpdsCredentials) -> Result<(), BackendError> {
        Err(unavailable())
    }
}
#[cfg(target_arch = "wasm32")]
fn unavailable() -> BackendError {
    BackendError::message("OPDS feeds are not available in the browser")
}

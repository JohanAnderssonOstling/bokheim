//! Browser storage and deployment adapters for subject reference data.

use client_protocol::{BackendError, Failure, OperationError};
use js_sys::{Uint8Array, global};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Response, WorkerGlobalScope};

mod bisac;
mod taxonomy;

pub use bisac::install as initialize_bisac_reference;
pub use taxonomy::install_bundled_matching as initialize_bundled_taxonomy;

pub(crate) async fn initialize() -> Result<(), BackendError> {
    initialize_bisac_reference().await?;
    taxonomy::install().await
}

/// Fetch a deployed application asset. Fetch decodes its HTTP content encoding.
async fn download_bundle(filename: &str) -> Result<Vec<u8>, BackendError> {
    let scope: WorkerGlobalScope = global().dyn_into().map_err(|_| "Reference data requires a worker")?;
    let base = js_sys::Reflect::get(&global(), &"__BOKHEIM_ASSET_BASE__".into()).ok().and_then(|base| base.as_string()).unwrap_or_else(|| "./".into());
    let response: Response =
        JsFuture::from(scope.fetch_with_str(&format!("{}/{filename}", base.trim_end_matches('/')))).await.map_err(|error| BackendError::operation(OperationError::from(error)).context(format!("{filename} download failed")))?.dyn_into().map_err(|_| format!("Invalid {filename} response"))?;
    if !response.ok() {
        return Err(match Failure::from_http_status(response.status()) {
            Some(failure) => BackendError::classified(failure, format!("{filename} download returned HTTP {}", response.status())),
            None => BackendError::message(format!("{filename} download returned HTTP {}", response.status())),
        });
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(|error| BackendError::operation(OperationError::from(error)).context(format!("{filename} response failed")))?).await.map_err(|error| BackendError::operation(OperationError::from(error)).context(format!("{filename} response failed")))?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

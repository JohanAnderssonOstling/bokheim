//! Browser storage and deployment adapters for subject reference data.

use js_sys::{Uint8Array, global};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Response, WorkerGlobalScope};

mod bisac;
mod taxonomy;

pub use bisac::install as initialize_bisac_reference;
pub use taxonomy::install_bundled_matching as initialize_bundled_taxonomy;

pub(crate) async fn initialize() -> Result<(), String> {
    initialize_bisac_reference().await?;
    taxonomy::install().await
}

/// Fetch a deployed application asset. Fetch decodes its HTTP content encoding.
async fn download_bundle(filename: &str) -> Result<Vec<u8>, String> {
    let scope: WorkerGlobalScope = global().dyn_into().map_err(|_| "Reference data requires a worker")?;
    let base = js_sys::Reflect::get(&global(), &"__BOKHEIM_ASSET_BASE__".into()).ok().and_then(|base| base.as_string()).unwrap_or_else(|| "./".into());
    let response: Response =
        JsFuture::from(scope.fetch_with_str(&format!("{}/{filename}", base.trim_end_matches('/')))).await.map_err(|error| format!("{filename} download failed: {error:?}"))?.dyn_into().map_err(|_| format!("Invalid {filename} response"))?;
    if !response.ok() {
        return Err(format!("{filename} download returned HTTP {}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(|error| format!("{filename} response failed: {error:?}"))?).await.map_err(|error| format!("{filename} response failed: {error:?}"))?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

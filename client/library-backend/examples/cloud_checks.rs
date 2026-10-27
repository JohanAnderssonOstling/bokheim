//! Test-only WASM entry points for the browser cloud runtime fixture.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = prepareCloudChecks)]
pub async fn prepare(endpoint: String) -> Result<(), JsValue> {
    library_backend::prepare_browser_cloud_checks(&endpoint).await.map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = runCloudChecks)]
pub async fn run(endpoint: String, writable: bool) -> Result<String, JsValue> {
    library_backend::check_browser_cloud(&endpoint, writable).await.map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = executeCpu)]
pub async fn execute_cpu(bytes: &[u8], source: JsValue) -> Result<Vec<u8>, JsValue> {
    let response = (async {
        let request: cpu_host::CpuRequest = rmp_serde::from_slice(bytes).map_err(library_backend::BackendError::operation)?;
        match request {
            cpu_host::CpuRequest::Book { task, .. } => {
                library_backend::library::web_reader::connect(&source).await.map_err(|error| library_backend::BackendError::message(format!("{error:?}")))?;
                let reader = library_backend::library::web_reader::Reader::new(source).map_err(library_backend::BackendError::operation)?;
                cpu_host::execute_book_with_mobi(task, reader, |_, _| Err("fixture does not inspect MOBI pages".into()))
            }
            request => cpu_host::execute(request),
        }
    })
    .await;
    rmp_serde::to_vec_named(&response).map_err(|error| JsValue::from_str(&error.to_string()))
}

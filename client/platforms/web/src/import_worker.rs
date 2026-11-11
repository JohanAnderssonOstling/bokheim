//! Browser file capabilities go once to a dedicated copy/hash worker.
use wasm_bindgen::prelude::*;
use operation_failure::OperationError;

#[wasm_bindgen(module = "/src/import_worker.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = initializeImportWorker)]
    fn initialize_worker(url: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = copySelectedFile)]
    pub(crate) async fn copy_file(file: &web_sys::Blob, path: &str, signal: &web_sys::AbortSignal, checksum: Option<&str>) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = hashStoredFile)]
    pub(crate) async fn hash_file(file: &web_sys::Blob, signal: &web_sys::AbortSignal) -> Result<JsValue, JsValue>;
}

pub fn initialize(url: &str) -> Result<(), OperationError> {
    initialize_worker(url).map_err(OperationError::from)
}

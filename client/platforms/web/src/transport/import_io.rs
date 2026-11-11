use std::pin::Pin;
use wasm_bindgen::prelude::*;
use operation_failure::OperationError;

#[wasm_bindgen(module = "/src/transport/selected_files.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = initializeSelectedFiles)]
    fn initialize_selected_files() -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = takeSelectedFile)]
    pub fn take_selected_file(token: &str, index: usize) -> JsValue;
    #[wasm_bindgen(js_name = releaseSelectedFiles)]
    pub fn release_selected_files(token: &str);
    #[wasm_bindgen(catch, js_name = sendSelectedFiles)]
    async fn send_files(token: &str, files: &js_sys::Array, signal: &web_sys::AbortSignal) -> Result<(), JsValue>;
}
pub fn initialize() -> Result<(), OperationError> {
    initialize_selected_files().map_err(OperationError::from)
}

/// A Rust lease can cross workers; browser files stay on the storage owner.
pub struct SelectedFiles {
    id: String,
    release: Option<Box<dyn FnOnce(String) + Send>>,
}
impl SelectedFiles {
    pub fn id(&self) -> &str {
        &self.id
    }
}
impl Drop for SelectedFiles {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release(self.id.clone());
        }
    }
}
pub async fn send_selected_files(files: &js_sys::Array, release: impl FnOnce(String) + Send + 'static) -> Result<SelectedFiles, OperationError> {
    let lease = SelectedFiles { id: uuid::Uuid::new_v4().to_string(), release: Some(Box::new(release)) };
    struct Sending(web_sys::AbortController);
    impl Drop for Sending {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let sending = Sending(web_sys::AbortController::new().map_err(OperationError::from)?);
    send_files(lease.id(), files, &sending.0.signal()).await.map_err(OperationError::from)?;
    Ok(lease)
}

/// A picker-owned browser source, sent to the database worker with its import.
/// JavaScript capabilities stay in this adapter rather than the import model.
pub struct ImportSource(Option<Box<dyn FnOnce() -> JsValue + Send>>);
pub type ImportReader = Pin<Box<dyn futures_util::io::AsyncRead>>;
pub type ImportReaderFuture = Pin<Box<dyn std::future::Future<Output = Result<ImportReader, String>>>>;
impl ImportSource {
    pub fn from_export(export: Option<Box<dyn FnOnce() -> JsValue + Send>>) -> Self {
        Self(export)
    }
    pub fn into_file(self) -> Result<web_sys::File, String> {
        let export = self.0.ok_or("Selected file cannot be sent with the import")?;
        export().dyn_into().map_err(|_| "Selected source is not a browser file".to_owned())
    }
    /// Unknown until the file is taken; the copy reports its own size.
    pub fn size_bytes(&self) -> Option<u64> {
        None
    }
}

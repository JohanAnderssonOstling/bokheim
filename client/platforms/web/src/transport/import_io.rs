use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use wasm_bindgen::prelude::*;

thread_local! {
    /// Picked files that arrived with an import request, by the request's
    /// token, until the import takes them one at a time.
    static SELECTED: RefCell<HashMap<String, Vec<JsValue>>> = RefCell::new(HashMap::new());
}

/// Keeps the files sent with an import request until the import reads them.
pub fn hold_selected_files(token: String, files: &js_sys::Array) {
    SELECTED.with(|selected| selected.borrow_mut().insert(token, files.iter().collect()));
}

/// The picked file at `index` of an import request; undefined once taken.
pub fn take_selected_file(token: &str, index: usize) -> JsValue {
    SELECTED.with(|selected| selected.borrow_mut().get_mut(token).and_then(|files| files.get_mut(index)).map(std::mem::take).unwrap_or(JsValue::UNDEFINED))
}

/// Drops whatever an import request did not take.
pub fn release_selected_files(token: &str) {
    SELECTED.with(|selected| selected.borrow_mut().remove(token));
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

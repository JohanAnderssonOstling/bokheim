//! Browser resources released with their Rust owner.
use crate::web_storage::FileReader;
use wasm_bindgen::prelude::*;
pub struct BlobUrl(String);
impl BlobUrl {
    pub fn new(blob: &web_sys::Blob) -> Result<Self, JsValue> {
        web_sys::Url::create_object_url_with_blob(blob).map(Self)
    }
    pub fn url(&self) -> &str {
        &self.0
    }
}
impl Drop for BlobUrl {
    fn drop(&mut self) {
        let _ = web_sys::Url::revoke_object_url(&self.0);
    }
}

/// Keeps a local media URL and the storage generation it references alive
/// until playback is released.
pub struct LocalMediaLease {
    _url: BlobUrl,
    _file: FileReader,
}

pub async fn local_media_lease(file: FileReader) -> Result<(String, LocalMediaLease), String> {
    let url = BlobUrl::new(&file.upload_file().await.map_err(|error| format!("Could not open audiobook file: {error}"))?)
        .map_err(|error| format!("Could not create audiobook URL: {error:?}"))?;
    let value = url.url().to_owned();
    Ok((value, LocalMediaLease { _url: url, _file: file }))
}

pub struct AsyncCallback(Closure<dyn FnMut() -> js_sys::Promise>);
impl AsyncCallback {
    pub fn new<F, Fut>(mut callback: F) -> Self
    where
        F: FnMut() -> Fut + 'static,
        Fut: std::future::Future<Output = Result<JsValue, JsValue>> + 'static,
    {
        Self(Closure::new(move || wasm_bindgen_futures::future_to_promise(callback())))
    }
    pub fn value(&self) -> JsValue {
        self.0.as_ref().clone()
    }
}

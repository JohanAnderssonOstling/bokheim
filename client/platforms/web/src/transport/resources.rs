//! Browser resources released with their Rust owner.
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

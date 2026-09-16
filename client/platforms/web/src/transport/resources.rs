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

#[cfg(feature = "storage-contract-tests")]
pub async fn contract() -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let blob = web_sys::Blob::new_with_str_sequence(&js_sys::Array::of1(&"audio fixture".into()))?;
    let owned = BlobUrl::new(&blob)?;
    let url = owned.url().to_owned();
    let scope: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let response: web_sys::Response = JsFuture::from(scope.fetch_with_str(&url)).await?.dyn_into()?;
    assert_eq!(JsFuture::from(response.text()?).await?.as_string().as_deref(), Some("audio fixture"));
    drop(owned);
    assert!(JsFuture::from(scope.fetch_with_str(&url)).await.is_err(), "dropped playback URL must be revoked");
    let callback = AsyncCallback::new(|| async { Ok("renewed".into()) });
    let promise: js_sys::Promise = callback.value().unchecked_into::<js_sys::Function>().call0(&JsValue::UNDEFINED)?.dyn_into()?;
    assert_eq!(JsFuture::from(promise).await?.as_string().as_deref(), Some("renewed"));
    Ok(())
}

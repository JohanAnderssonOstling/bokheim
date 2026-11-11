use crate::{DictionaryStore, Result};
use std::{path::Path, sync::Arc};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(module = "/src/service/web.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    fn observe(root: &str, changed: &js_sys::Function) -> std::result::Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    fn publish(channel: &JsValue) -> std::result::Result<(), JsValue>;
    fn close(channel: &JsValue);
}
pub(super) struct Notifications {
    channel: JsValue,
    _changed: Closure<dyn FnMut()>,
}
impl Notifications {
    pub fn new(root: &Path, store: Arc<DictionaryStore>) -> Result<Self> {
        // This callback only signals observers; it does not edit catalog state.
        let changed = Closure::own_assert_unwind_safe(move || store.notify_changed());
        let channel = observe(&root.to_string_lossy(), changed.as_ref().unchecked_ref()).map_err(crate::OperationError::from)?;
        Ok(Self { channel, _changed: changed })
    }
    pub fn publish(&self) {
        if let Err(error) = publish(&self.channel) {
            log::warn!("Dictionary notification failed: {error:?}");
        }
    }
}
impl Drop for Notifications {
    fn drop(&mut self) {
        close(&self.channel);
    }
}

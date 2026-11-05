use wasm_bindgen::{JsCast, prelude::*};
#[derive(Clone)]
pub struct Scope(web_sys::DedicatedWorkerGlobalScope);
impl Scope {
    pub fn current() -> Self {
        Self(js_sys::global().unchecked_into())
    }
    pub fn post_message(&self, message: &JsValue) -> Result<(), JsValue> {
        self.0.post_message(message)
    }
    /// Retained by the worker global for its lifetime; replacement releases the JS callback.
    pub fn receive(&self, mut callback: impl FnMut(JsValue) + 'static) {
        let handler = Closure::<dyn FnMut(web_sys::MessageEvent)>::own_aborting(move |event: web_sys::MessageEvent| callback(event.data())).into_js_value();
        self.0.set_onmessage(Some(handler.unchecked_ref()));
    }
}

impl Scope {
    pub fn post_message_with_transfer(&self, message: &JsValue, transfer: &js_sys::Array) -> Result<(), JsValue> {
        self.0.post_message_with_transfer(message, transfer)
    }
}

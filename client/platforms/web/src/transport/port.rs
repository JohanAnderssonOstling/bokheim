use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{MessageEvent, MessagePort};

/// Owns browser callbacks and closes the port before releasing their closures.
pub struct Port {
    pub raw: MessagePort,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _error: Closure<dyn FnMut(MessageEvent)>,
}
impl Port {
    /// Application adapters receive payloads; the platform owns browser events.
    pub fn listen(raw: MessagePort, mut message: impl FnMut(wasm_bindgen::JsValue) + 'static, mut malformed: impl FnMut() + 'static) -> Self {
        Self::new(raw, move |event| message(event.data()), move |_| malformed())
    }

    pub fn new(raw: MessagePort, message: impl FnMut(web_sys::MessageEvent) + 'static, error: impl FnMut(web_sys::MessageEvent) + 'static) -> Self {
        let message = Closure::<dyn FnMut(MessageEvent)>::new(message);
        let error = Closure::<dyn FnMut(MessageEvent)>::new(error);
        raw.set_onmessage(Some(message.as_ref().unchecked_ref()));
        raw.set_onmessageerror(Some(error.as_ref().unchecked_ref()));
        raw.start();
        Self { raw, _message: message, _error: error }
    }
}
impl Drop for Port {
    fn drop(&mut self) {
        self.raw.set_onmessage(None);
        self.raw.set_onmessageerror(None);
        self.raw.close();
    }
}

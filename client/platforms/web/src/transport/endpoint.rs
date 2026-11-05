//! The synchronous parser owns one dedicated browser worker.
use js_sys::{Array, Function};
use std::rc::Rc;
use wasm_bindgen::JsValue;
use web_sys::Worker;

#[derive(Clone)]
pub struct WorkerEndpoint(Worker);
impl From<Worker> for WorkerEndpoint {
    fn from(worker: Worker) -> Self {
        Self(worker)
    }
}
impl WorkerEndpoint {
    pub fn post_message_with_transfer(&self, message: &JsValue, transfer: &Array) -> Result<(), JsValue> {
        self.0.post_message_with_transfer(message, transfer)
    }
    pub fn set_onmessage(&self, callback: Option<&Function>) {
        self.0.set_onmessage(callback);
    }
    pub fn set_onmessageerror(&self, callback: Option<&Function>) {
        self.0.set_onmessageerror(callback);
    }
    pub fn set_onerror(&self, callback: Option<&Function>) {
        self.0.set_onerror(callback);
    }
    pub fn terminate(&self) {
        self.0.terminate();
    }
}
/// Creates a module worker for synchronous parser execution.
pub fn module_worker(url: &str, name: &str) -> Result<Worker, JsValue> {
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    options.set_name(name);
    Worker::new_with_options(url, &options)
}

/// Owns worker callbacks and disconnects them before terminating the endpoint.
pub struct OwnedEndpoint {
    endpoint: WorkerEndpoint,
    _message: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MessageEvent)>,
    _error: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>,
    _malformed: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MessageEvent)>,
}
impl OwnedEndpoint {
    /// Own event conversion and browser errors while callers choose failure policy.
    pub fn listen(endpoint: WorkerEndpoint, mut message: impl FnMut(JsValue) + 'static, failed: impl Fn(String) + 'static, stopped: &'static str, malformed: &'static str) -> Self {
        let failed = Rc::new(failed);
        let invalid = failed.clone();
        Self::new(
            endpoint,
            move |event| message(event.data()),
            move |event| {
                event.prevent_default();
                let error = super::field(event.as_ref(), "message").as_string().filter(|value| !value.is_empty()).unwrap_or_else(|| stopped.into());
                failed(error);
            },
            move |_| invalid(malformed.into()),
        )
    }

    fn new(endpoint: WorkerEndpoint, message: impl FnMut(web_sys::MessageEvent) + 'static, error: impl FnMut(web_sys::Event) + 'static, malformed: impl FnMut(web_sys::MessageEvent) + 'static) -> Self {
        use wasm_bindgen::{JsCast, closure::Closure};
        let message = Closure::<dyn FnMut(web_sys::MessageEvent)>::own_aborting(message);
        let error = Closure::<dyn FnMut(web_sys::Event)>::own_aborting(error);
        let malformed = Closure::<dyn FnMut(web_sys::MessageEvent)>::own_aborting(malformed);
        endpoint.set_onmessage(Some(message.as_ref().unchecked_ref()));
        endpoint.set_onerror(Some(error.as_ref().unchecked_ref()));
        endpoint.set_onmessageerror(Some(malformed.as_ref().unchecked_ref()));
        Self { endpoint, _message: message, _error: error, _malformed: malformed }
    }
    pub fn endpoint(&self) -> &WorkerEndpoint {
        &self.endpoint
    }
}
impl Drop for OwnedEndpoint {
    fn drop(&mut self) {
        self.endpoint.set_onmessage(None);
        self.endpoint.set_onerror(None);
        self.endpoint.set_onmessageerror(None);
        self.endpoint.terminate();
    }
}

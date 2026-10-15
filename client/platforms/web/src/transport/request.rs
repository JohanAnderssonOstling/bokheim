//! Per-request cancellation over a dedicated reply port.
use super::{BytesFuture, Port, byte_reply, field};
use async_channel::bounded;
use wasm_bindgen::{JsCast, JsValue};

/// A queued source owns its endpoint until transfer succeeds.
#[derive(Default)]
pub struct Source(JsValue);
impl Source {
    pub fn new(value: JsValue) -> Self {
        Self(value)
    }
    pub fn value(&self) -> JsValue {
        self.0.clone()
    }
    pub fn transferred(&mut self) {
        self.0 = JsValue::UNDEFINED;
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        if let Ok(port) = field(&self.0, "port").dyn_into::<web_sys::MessagePort>() {
            port.close();
        }
    }
}

/// Cancellation drops the operation future, which releases queued domain work.
pub fn serve(port: web_sys::MessagePort, response: BytesFuture) {
    let (cancel, cancelled) = bounded::<()>(1);
    let malformed = cancel.clone();
    let owner = Port::listen(
        port,
        move |_message| {
            // The dedicated caller-to-server channel only carries cancellation.
            cancel.close();
        },
        move || {
            malformed.close();
        },
    );
    wasm_bindgen_futures::spawn_local(async move {
        if let Some(result) = client_platform_runtime::pending::until_cancelled(response, cancelled.recv()).await {
            let (message, transfer) = byte_reply(&[], result);
            let _ = owner.raw.post_message_with_transferable(&message, &transfer);
        }
    });
}

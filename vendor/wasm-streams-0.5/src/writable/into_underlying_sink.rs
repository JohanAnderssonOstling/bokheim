use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;

use crate::callback::{future_to_promise_aborting, AbortOnPanic};
use futures_util::{Sink, SinkExt};
use js_sys::Promise;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub(crate) struct IntoUnderlyingSink {
    inner: AbortOnPanic<Rc<RefCell<Inner>>>,
}

impl IntoUnderlyingSink {
    pub fn new(sink: Box<dyn Sink<JsValue, Error = JsValue>>) -> Self {
        IntoUnderlyingSink { inner: AbortOnPanic::new(Rc::new(RefCell::new(Inner::new(sink)))) }
    }
}

#[allow(clippy::await_holding_refcell_ref)]
#[wasm_bindgen]
impl IntoUnderlyingSink {
    pub fn write(&mut self, chunk: JsValue) -> Promise {
        let inner = self.inner.with(Rc::clone);
        future_to_promise_aborting(async move {
            // This mutable borrow can never panic, since the WritableStream always queues
            // each operation on the underlying sink.
            let mut inner = inner.try_borrow_mut().unwrap_throw();
            inner.write(chunk).await.map(|_| JsValue::undefined())
        })
    }

    pub fn close(self) -> Promise {
        let inner = self.inner.with(Rc::clone);
        future_to_promise_aborting(async move {
            let mut inner = inner.try_borrow_mut().unwrap_throw();
            inner.close().await.map(|_| JsValue::undefined())
        })
    }

    pub fn abort(self, reason: JsValue) -> Promise {
        let inner = self.inner.with(Rc::clone);
        future_to_promise_aborting(async move {
            let mut inner = inner.try_borrow_mut().unwrap_throw();
            inner.abort(reason).await.map(|_| JsValue::undefined())
        })
    }
}

struct Inner {
    sink: Option<Pin<Box<dyn Sink<JsValue, Error = JsValue>>>>,
}

impl Inner {
    fn new(sink: Box<dyn Sink<JsValue, Error = JsValue>>) -> Self {
        Inner { sink: Some(sink.into()) }
    }

    async fn write(&mut self, chunk: JsValue) -> Result<(), JsValue> {
        // Take the sink out before the fallible/panickable operation.
        // This ensures that if a panic occurs, self.sink is already None,
        // so any subsequent call will fail cleanly instead of using corrupted state.
        let mut sink = self.sink.take().unwrap_throw();

        match sink.send(chunk).await {
            Ok(()) => {
                // Success: put the sink back for reuse
                self.sink = Some(sink);
                Ok(())
            }
            Err(err) => {
                // Error: the sink is dropped, self.sink remains None
                Err(err)
            }
        }
        // Panic: sink is dropped during unwind, self.sink remains None
    }

    async fn close(&mut self) -> Result<(), JsValue> {
        // Take ownership and close - sink is dropped after close completes
        self.sink.take().unwrap_throw().close().await
    }

    async fn abort(&mut self, _reason: JsValue) -> Result<(), JsValue> {
        // Take and drop the sink immediately
        self.sink = None;
        Ok(())
    }
}

//! An in-process reply. Encoding is deferred until an actual wire boundary.
use crate::{
    BackendError,
    wire::{WorkerPayload, encode_reply},
};
use client_platform_runtime::executor::BackendOutput;
use std::any::Any;

#[cfg(not(target_arch = "wasm32"))]
type Value = Box<dyn Any + Send>;
#[cfg(target_arch = "wasm32")]
type Value = Box<dyn Any>;

pub struct Reply {
    value: Value,
    encode: fn(&dyn Any) -> Result<WorkerPayload, BackendError>,
}

impl Reply {
    pub fn new<T: BackendOutput + serde::Serialize>(value: T) -> Self {
        Self { value: Box::new(value), encode: |value| encode_reply(value.downcast_ref::<T>().expect("reply encoder matches its value")) }
    }

    pub fn take<T: BackendOutput>(self) -> Result<T, BackendError> {
        self.value.downcast::<T>().map(|value| *value).map_err(|_| BackendError::message("unexpected backend reply type"))
    }

    pub fn into_wire(self) -> Result<WorkerPayload, BackendError> {
        (self.encode)(self.value.as_ref())
    }
}

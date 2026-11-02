//! An in-process reply. Encoding is deferred until an actual wire boundary.
#[cfg(target_arch = "wasm32")]
use crate::wire::{encode_reply, WorkerPayload};
use crate::BackendError;
use client_platform_runtime::executor::BackendOutput;
use std::any::Any;

/// Values a command adapter can encode and decode at its transport boundary.
/// Public client operations fix the response type before reaching that adapter.
pub trait WireReply: BackendOutput + serde::Serialize + serde::de::DeserializeOwned {}
impl<T: BackendOutput + serde::Serialize + serde::de::DeserializeOwned> WireReply for T {}

#[cfg(not(target_arch = "wasm32"))]
type Value = Box<dyn Any + Send>;
#[cfg(target_arch = "wasm32")]
type Value = Box<dyn Any>;

pub struct Reply {
    value: Value,
    #[cfg(target_arch = "wasm32")]
    encode: fn(&dyn Any) -> Result<WorkerPayload, BackendError>,
}

impl Reply {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new<T: BackendOutput>(value: T) -> Self {
        Self { value: Box::new(value) }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new<T: BackendOutput + serde::Serialize>(value: T) -> Self {
        Self { value: Box::new(value), encode: |value| encode_reply(value.downcast_ref::<T>().expect("reply encoder matches its value")) }
    }

    pub fn take<T: BackendOutput>(self) -> Result<T, BackendError> {
        self.value.downcast::<T>().map(|value| *value).map_err(|_| BackendError::message("unexpected backend reply type"))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn into_wire(self) -> Result<WorkerPayload, BackendError> {
        (self.encode)(self.value.as_ref())
    }
}

//! Kobo mounted-device filesystem operations and on-device services.
#![cfg(not(target_arch = "wasm32"))]
mod mounted;
mod eject;
#[cfg(feature = "device")]
pub use kobo_device as device;
pub use mounted::*;
pub use eject::{EjectOutcome, eject_device};

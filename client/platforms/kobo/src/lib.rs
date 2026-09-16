//! Kobo mounted-library transfer and on-device services.
#![cfg(not(target_arch = "wasm32"))]
mod mounted;
pub use mounted::*;
#[cfg(feature = "device")]
pub use kobo_device as device;

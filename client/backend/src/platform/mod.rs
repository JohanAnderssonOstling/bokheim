//! Platform-specific discovery and device integration.

pub(crate) mod database;
#[cfg(not(target_arch = "wasm32"))]
pub mod kobo;
pub(crate) mod library_events;
#[cfg(target_arch = "wasm32")]
pub mod web_storage;

#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub mod scanner;

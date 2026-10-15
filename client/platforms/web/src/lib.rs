//! Browser OPFS storage and durable asset indexing.
#[cfg(any(target_arch = "wasm32", test))]
pub mod storage_job;
#[cfg(any(target_arch = "wasm32", test))]
pub mod reply;
#[cfg(any(target_arch = "wasm32", test))]
mod serial;
#[cfg(any(target_arch = "wasm32", test))]
mod asset_index;
#[cfg(target_arch = "wasm32")]
pub mod cpu_host;
#[cfg(target_arch = "wasm32")]
pub mod range_reader;
#[cfg(target_arch = "wasm32")]
pub mod sqlite;
#[cfg(target_arch = "wasm32")]
pub mod transport;
#[cfg(target_arch = "wasm32")]
pub mod web_storage;

#[cfg(target_arch = "wasm32")]
pub mod signal;

#[cfg(target_arch = "wasm32")]
pub use web_storage::reserve_database_capacity;
#[cfg(target_arch = "wasm32")]
pub mod audiobook;

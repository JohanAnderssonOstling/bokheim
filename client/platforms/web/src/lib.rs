//! Browser OPFS storage and durable asset indexing.
// Run the local VFS handle-count regression tests without browser dependencies.
#[cfg(any(target_arch = "wasm32", test))]
mod asset_index;
#[cfg(target_arch = "wasm32")]
pub mod cpu_host;
#[cfg(test)]
#[path = "../../../../vendor/sqlite-wasm-vfs/src/open_files.rs"]
mod opfs_open_files;
#[cfg(target_arch = "wasm32")]
pub mod range_reader;
#[cfg(target_arch = "wasm32")]
mod reader;
#[cfg(target_arch = "wasm32")]
mod reference_storage;
#[cfg(any(target_arch = "wasm32", test))]
pub mod reply;
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

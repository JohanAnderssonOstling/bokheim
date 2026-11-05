//! Independent files on the native filesystem or browser OPFS.

/// One regular file, with its last write time in milliseconds since the Unix epoch.
pub struct DirectoryEntry {
    pub name: String,
    pub size: u64,
    pub modified_ms: u64,
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;

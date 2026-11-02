//! Target-selected mechanisms. Shared storage and library policy live elsewhere.
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::web_storage;

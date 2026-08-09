//! Platform-specific discovery and device integration.

pub mod kobo;

#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub mod scanner;
#[cfg(not(all(feature = "scanner", not(target_os = "android"))))]
#[path = "scanner_stub.rs"]
pub mod scanner;

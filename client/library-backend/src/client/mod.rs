//! Client-facing library name and workflows spanning resource operations.

pub use crate::library::LibraryHandle as LibraryClient;
#[cfg(not(target_arch = "wasm32"))]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
pub use browser::BrowserDownloads;
#[cfg(not(target_arch = "wasm32"))]
mod transfer;
#[cfg(not(target_arch = "wasm32"))]
pub use transfer::{LibraryTransferOutcome, TransferSelection};
#[cfg(target_arch = "wasm32")]
mod web;

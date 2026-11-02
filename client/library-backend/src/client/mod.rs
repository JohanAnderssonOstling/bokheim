mod caches;
mod books;
#[cfg(not(target_arch = "wasm32"))]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
pub use browser::BrowserDownloads;
mod browse;
mod client;
mod directories;
mod import;
mod opds;
mod reading;
#[cfg(not(target_arch = "wasm32"))]
mod transfer;
#[cfg(not(target_arch = "wasm32"))]
pub use transfer::{LibraryTransferOutcome, TransferSelection};

#[cfg(target_arch = "wasm32")]
pub use client::TransportResult;
pub use client::{ClientTransport, LibraryClient};
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
mod annotation_export;

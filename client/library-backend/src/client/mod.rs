mod books;
mod browse;
mod client;
mod directories;
mod import;
mod reading;
#[cfg(not(target_arch = "wasm32"))]
mod torrents;
#[cfg(not(target_arch = "wasm32"))]
mod transfer;
#[cfg(not(target_arch = "wasm32"))]
pub use transfer::{LibraryTransferOutcome, TransferSelection};

#[cfg(target_arch = "wasm32")]
pub use client::TransportResult;
pub use client::{ClientTransport, LibraryClient};
#[cfg(target_arch = "wasm32")]
mod web;

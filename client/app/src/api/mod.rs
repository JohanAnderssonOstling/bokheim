mod client;

pub use client::AppClient;

#[cfg(not(target_arch = "wasm32"))]
mod launch;
#[cfg(not(target_arch = "wasm32"))]
pub use launch::BackendLaunch;

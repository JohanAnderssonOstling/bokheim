//! Typed network execution; the library asset scheduler owns ordering and retry.
mod authentication;
mod executor;
pub use executor::AssetExecutor;

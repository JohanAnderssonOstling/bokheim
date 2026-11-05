//! Browser startup and deployment reference data.
#![cfg(target_arch = "wasm32")]
mod reference_data;
pub(crate) use reference_data::initialize as initialize_reference_data;
pub use reference_data::{initialize_bisac_reference, initialize_bundled_taxonomy};

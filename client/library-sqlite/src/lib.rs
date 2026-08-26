//! Shared SQLite library backend compiled unchanged for native and browser WASM.

pub mod database;
pub mod filenames;
mod local_paths;

pub use database::*;
pub use local_paths::{BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};

//! Publication files, verified staging, leases, and physical placement.
pub use client_library::{RelativeBookPath, RelativeDirPath};
use client_platform_runtime::executor;
pub use library_replica::BlobKind;
pub use publication_access::{BookReader, BoxedBookReader};
pub use sync_common::{ContentHash, DirId, ROOT_DIR_ID};
pub const APP_HIDDEN_DIR: &str = ".bokheim";
pub const APP_HIDDEN_LIBRARY_DB_PATH: &str = ".bokheim/library.db";
pub mod asset_store;
pub mod database;
#[cfg(target_arch = "wasm32")]
pub mod hashing;
pub use asset_store::{AssetStore, AssetStoreError};

pub use library_replica::ScanProgress;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
pub mod events;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
pub mod scanner;
#[cfg(all(test, feature = "scanner", not(target_arch = "wasm32")))]
mod test_database;

pub mod thumbnail_jobs;

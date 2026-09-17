//! Metadata synchronization and binary asset transfer.

mod asset_sync;
mod error;
mod library_sync;
pub(crate) mod recovery;
mod scheduler;
mod state_sync;
pub mod timing;
#[cfg(target_arch = "wasm32")]
mod web_coordinator;
#[cfg(target_arch = "wasm32")]
pub(crate) use state_sync::{apply_pull_page, inventory_checkpoint, save_inventory_checkpoint};
pub(crate) mod transfer;

pub(crate) use error::SyncError;
#[cfg(test)]
pub(crate) use library_sync::PersistedSyncWork;
pub use library_sync::{LibrarySync, LibrarySyncConfig};
pub use library_runtime::scheduler::{SyncRequest, SyncScheduler};
#[cfg(target_arch = "wasm32")]
pub use scheduler::run_timed;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_coordinator::synchronize;
#[cfg(target_arch = "wasm32")]
pub use web_coordinator::run;
pub(crate) use sync_engine::SyncOutcome;
pub(crate) use library_database::Database;

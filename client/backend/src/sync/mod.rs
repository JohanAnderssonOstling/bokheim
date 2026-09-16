//! Metadata synchronization and binary asset transfer.

mod asset_sync;
mod error;
mod library_sync;
pub(crate) mod recovery;
mod scheduler;
mod state_sync;
pub(crate) mod timing;
pub(crate) use state_sync::ensure_sync_account;
#[cfg(target_arch = "wasm32")]
pub(crate) use state_sync::{apply_pull_page, inventory_checkpoint, save_inventory_checkpoint};
pub(crate) mod transfer;

pub(crate) use error::SyncError;
#[cfg(test)]
pub(crate) use library_sync::PersistedSyncWork;
pub(crate) use library_sync::{LibrarySync, LibrarySyncConfig};
pub(crate) use scheduler::SyncRequest;
pub(crate) use scheduler::SyncScheduler;
#[cfg(target_arch = "wasm32")]
pub(crate) use scheduler::run_timed;
pub(crate) use sync_engine::SyncOutcome;
pub(crate) type DbPool = crate::storage::database::Database;

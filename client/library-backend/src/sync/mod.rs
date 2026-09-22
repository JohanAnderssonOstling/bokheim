//! Metadata synchronization and binary asset transfer.

mod asset_sync;
mod error;
mod library_sync;
pub(crate) mod recovery;
mod scheduler;
mod state_sync;
#[cfg(target_arch = "wasm32")]
mod web_coordinator;

pub(crate) use error::SyncError;
pub(crate) use library_database::Database;
pub use library_runtime::scheduler::{BackgroundTiming, SyncRequest, SyncScheduler};
#[cfg(test)]
pub(crate) use library_sync::PersistedSyncWork;
pub use library_sync::{LibrarySync, LibrarySyncConfig};
#[cfg(target_arch = "wasm32")]
pub use scheduler::run_timed;
pub(crate) use sync_engine::SyncOutcome;
#[cfg(target_arch = "wasm32")]
pub use web_coordinator::run;
#[cfg(target_arch = "wasm32")]
pub(crate) use web_coordinator::synchronize;

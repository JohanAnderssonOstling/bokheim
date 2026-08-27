//! Metadata synchronization and binary asset transfer.

mod asset_sync;
mod error;
mod manager;
mod state_sync;
pub(crate) mod transfer;

pub(crate) use error::SyncError;
pub(crate) use manager::{AssetSync, ReadingPositionSync, SyncManager, SyncManagerConfig, SyncOutcome, SyncPolicy};
pub(crate) type DbPool = crate::platform::database::Database;

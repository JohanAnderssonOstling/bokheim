//! Metadata synchronization and binary asset transfer.

pub(crate) mod blob;
mod manager;
pub(crate) mod transfer;

pub(crate) use manager::{AssetSync, DbPool, ReadingPositionSync, SyncManager, SyncOutcome, SyncPolicy};

//! Session-owned transfer execution: the worker snapshots, drains, commits.
mod coordination;
mod executor;
mod worker;

pub(crate) use worker::TransferWorker;

type TransferRunResult = Result<(), TransferError>;

pub use asset_transfer::{LibraryTransfers, TransferOrigin, TransferState, TransferStatus};
pub(crate) use asset_transfer::{TransferJob, TransferOperation, TransferQueue};

mod authentication;

pub(crate) use library_runtime::transfer_history::queue_with_history;

pub(super) use asset_transfer::TransferError;
use authentication::RefreshScope;

#[cfg(test)]
use crate::{BookPlacement, ContentHash};
#[cfg(test)]
use sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES;

#[cfg(test)]
mod tests;

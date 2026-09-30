//! Asset transfer planning, transfer commits, and operation status.

mod downloads;
mod operations;
mod planning;
mod presence;
mod transfer;
mod uploads;

pub use operations::{OperationActivity, OperationStatus};
pub use planning::{BookUploadCompletion, BookUploadSnapshot, RejectedAsset};
pub use transfer::{AssetTransferState, PlacementTransferEntry, TransferOutcome, TransferSnapshot, TransferWrite};
pub(crate) use uploads::record_local_version;

use include_sqlite_sql::include_sql;

use crate::DatabaseError;

fn unix_secs() -> Result<i64, DatabaseError> {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).map_err(DatabaseError::operation)
}

/// An upload the server rejected waits this long before it is tried again.
const REJECTION_BACKOFF_SECONDS: i64 = 5 * 60;

/// Rejections recorded after this time still hold their upload back.
pub(crate) fn rejection_cutoff() -> Result<i64, DatabaseError> {
    Ok(unix_secs()?.saturating_sub(REJECTION_BACKOFF_SECONDS))
}

include_sql!("src/assets/sql/schema/tables.sql");
include_sql!("src/assets/sql/transfers.sql");
include_sql!("src/assets/sql/operations.sql");

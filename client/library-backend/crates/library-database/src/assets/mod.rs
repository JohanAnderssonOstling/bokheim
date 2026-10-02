//! Asset transfer planning, transfer commits, and library activity.

pub(crate) mod downloads;
pub(crate) mod activity;
mod planning;
mod presence;
mod transfer;
pub(crate) mod uploads;

pub use activity::LibraryWork;
pub use planning::{BookUploadSnapshot, RejectedAsset};
pub use transfer::{AssetTransferState, PlacementTransferEntry, TransferOutcome, TransferSnapshot, TransferWrite};

use include_sqlite_sql::include_sql;

use crate::DatabaseError;

/// An upload the server rejected waits this long before it is tried again.
const REJECTION_BACKOFF_SECONDS: i64 = 5 * 60;

/// Rejections recorded after this time still hold their upload back.
pub fn rejection_cutoff() -> Result<i64, DatabaseError> {
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(DatabaseError::operation)?.as_secs() as i64;
    Ok(now.saturating_sub(REJECTION_BACKOFF_SECONDS))
}

include_sql!("src/assets/sql/schema/tables.sql");
include_sql!("src/assets/sql/transfers.sql");
include_sql!("src/assets/sql/activity.sql");

include_sql!("src/assets/sql/schema/asset_views.sql");

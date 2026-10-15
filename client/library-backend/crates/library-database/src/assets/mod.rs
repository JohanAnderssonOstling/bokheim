//! Asset transfer planning and transfer commits.

use crate::SqlParam;
pub(crate) mod downloads;
mod planning;
mod presence;
mod transfer;
pub(crate) mod uploads;

pub use planning::BookUploadSnapshot;
pub use transfer::{AssetTransferState, PlacementTransferEntry, TransferSnapshot};

use content_address::ContentHash;
use include_sqlite_sql::include_sql;

include_sql!("src/assets/sql/schema/tables.sql");
include_sql!("src/assets/sql/transfers.sql");
include_sql!("src/assets/sql/activity.sql");

include_sql!("src/assets/sql/schema/asset_views.sql");

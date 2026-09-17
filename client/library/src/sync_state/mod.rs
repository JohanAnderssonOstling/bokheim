mod inventory;
use include_sqlite_sql::include_sql;
mod outbox;

pub use inventory::{
    begin_cursor_recovery, begin_cursor_recovery_in_tx, complete_cursor_recovery, cursor_recovery_pending, enqueue_missing_state_cells, enqueue_missing_state_cells_in_tx, get_library_sync_details, get_pull_cursor, get_sync_metadata,
    recover_state_page, recover_state_page_in_tx, set_pull_cursor, set_sync_metadata, state_inventory, state_inventory_page, LibrarySyncDetails, RemovedBookDir, CURSOR_RECOVERY_PENDING_KEY,
};
pub use outbox::*;

mod functions;
pub(crate) use functions::*;
mod identity;
pub use identity::*;

include_sql!("src/sync_state/sql/sync.sql");

mod status;
pub use status::{has_pending_outbox, stored_sync_status};

mod exchange;
pub use exchange::{apply_prepared_pull_page_in_tx, apply_pull_page, inventory_checkpoint, save_inventory_checkpoint};

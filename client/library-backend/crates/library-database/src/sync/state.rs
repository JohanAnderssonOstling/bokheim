//! Sync cursor and recovery primitives.

use crate::query_methods::{first_column, optional_text_column, parse_uuid_column, query_last, query_rows};
use crate::shared_sql::SharedSql;
use crate::sync::{SyncStateSql, read_pull_cursor};
use crate::{Database, DatabaseError};
use sync_common::StateKey;

// Sync cursor and recovery primitives.
//
// Each method is one bounded read or one bounded write in its own
// transaction. Page- and chunk-sized looping belongs to the orchestrator,
// which drives these methods and the network between them.

use sync_common::{ReplicaId, StateAddress, SyncCursor};

impl Database {
    /// Reads the server history and both pull checkpoints together.
    pub fn sync_pull_cursor(&self) -> Result<SyncCursor, DatabaseError> {
        read_pull_cursor(&self.connection)
    }

    /// Whether a rejected cursor still needs recovery.
    pub fn sync_cursor_recovery_pending(&self) -> Result<bool, DatabaseError> {
        let pending = self.connection.sync_cursor_recovery_pending(|row| row.get::<_, Option<i64>>(0))?;
        Ok(pending == Some(1))
    }

    /// Clears the generation and both checkpoints, persisting the recovery marker.
    pub fn sync_begin_cursor_recovery(&self) -> Result<(), DatabaseError> {
        self.connection.sync_begin_cursor_recovery().map(|_| ()).map_err(Into::into)
    }

    pub fn replica_id(&self) -> Result<Option<ReplicaId>, DatabaseError> {
        Ok(query_last(|read| self.connection.shared_owner_replica_id(read), |row| optional_text_column(row, 0)?.map(|value| parse_uuid_column(value, 0)).transpose())?.flatten())
    }

    pub fn set_last_successful_sync_at(&self, value: u64) -> Result<(), DatabaseError> {
        use crate::assets::ActivitySql;
        self.connection.activity_set_last_successful_sync_at(crate::sync::storage_i64(value, "last sync timestamp")?)?;
        Ok(())
    }

    pub fn server_library_created(&self) -> Result<Option<String>, DatabaseError> {
        Ok(first_column::<Option<String>>(|read| self.connection.shared_owner_server_library_created(read))?.flatten())
    }

    pub fn set_server_library_created(&self, identity: &str) -> Result<(), DatabaseError> {
        use crate::OwnerSql;
        self.connection.owner_set_server_library_created(identity)?;
        Ok(())
    }

    /// Read only the next bounded set of register identities. Local changes
    /// made behind this cursor already create durable outbox work through
    /// normal triggers.
    pub fn sync_inventory_page(&self, after: Option<&StateAddress>) -> Result<Vec<StateAddress>, DatabaseError> {
        let start = StateKey::Text(String::new());
        let (kind, key, subkey) = after.map(|cell| (cell.kind.as_str(), &cell.entity_key, cell.entity_subkey.as_str())).unwrap_or(("", &start, ""));
        Ok(query_rows(|read| self.connection.sync_state_inventory_page(kind, key, subkey, read), |row| Ok(StateAddress { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? }))?)
    }

    /// Recreates durable outbox entries for the given state cells, in one
    /// transaction. Cells already queued are skipped
    /// without re-encoding; cells with no current value are skipped.
    pub fn sync_enqueue_missing_state_cells(&self, cells: &[StateAddress]) -> Result<usize, DatabaseError> {
        if cells.is_empty() {
            return Ok(0);
        }
        self.with_write_transaction(|tx| {
            let mut inserted = 0;
            for cell in cells {
                inserted += usize::from(crate::sync::apply::registers::recover(tx, cell)?);
            }
            Ok(crate::transactions::WriteOutcome::Commit(inserted))
        })
    }

    /// Recovery commits one page at a time while retaining its durable
    /// marker. After interruption, restarting from the first page is safe
    /// because enqueue preserves existing outbox identities. Only an empty
    /// final page clears the marker.
    pub fn sync_recover_state_page(&self, after: Option<&StateAddress>) -> Result<(Option<StateAddress>, usize), DatabaseError> {
        let cells = self.sync_inventory_page(after)?;
        if cells.is_empty() {
            self.connection.sync_clear_cursor_recovery()?;
            return Ok((None, 0));
        }
        let inserted = self.sync_enqueue_missing_state_cells(&cells)?;
        Ok((cells.last().cloned(), inserted))
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn sync_pull_cursor() -> SyncCursor;
    arguments () = ();
    execute database, arguments => database.sync_pull_cursor();

    fn sync_cursor_recovery_pending() -> bool;
    arguments () = ();
    execute database, arguments => database.sync_cursor_recovery_pending();

    fn sync_begin_cursor_recovery() -> ();
    arguments () = ();
    execute database, arguments => database.sync_begin_cursor_recovery();

    fn replica_id() -> Option<ReplicaId>;
    arguments () = ();
    execute database, arguments => database.replica_id();

    fn set_last_successful_sync_at(value: u64) -> ();
    arguments (u64,) = (value,);
    execute database, arguments => database.set_last_successful_sync_at(arguments.0);

    fn server_library_created() -> Option<String>;
    arguments () = ();
    execute database, arguments => database.server_library_created();

    fn set_server_library_created(identity: &str) -> ();
    arguments (String,) = (identity.to_owned(),);
    execute database, arguments => database.set_server_library_created(&arguments.0);

    fn sync_inventory_page(after: Option<&StateAddress>) -> Vec<StateAddress>;
    arguments (Option<StateAddress>,) = (after.cloned(),);
    execute database, arguments => database.sync_inventory_page(arguments.0.as_ref());

    fn sync_enqueue_missing_state_cells(cells: &[StateAddress]) -> usize;
    arguments (Vec<StateAddress>,) = (cells.to_vec(),);
    execute database, arguments => database.sync_enqueue_missing_state_cells(&arguments.0);

    fn sync_recover_state_page(after: Option<&StateAddress>) -> (Option<StateAddress>, usize);
    arguments (Option<StateAddress>,) = (after.cloned(),);
    execute database, arguments => database.sync_recover_state_page(arguments.0.as_ref());
}

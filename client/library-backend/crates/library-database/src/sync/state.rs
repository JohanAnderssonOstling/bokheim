//! Sync cursor, inventory, and recovery primitives.

use crate::shared_sql::SharedSql;
use crate::sync::{PullCursorKey, SyncApplySql, SyncStateSql, write_cursor_revision};
use crate::{Database, DatabaseError};

// Sync cursor, inventory, and recovery primitives.
//
// Each method is one bounded read or one bounded write in its own
// transaction. Page- and chunk-sized looping belongs to the orchestrator,
// which drives these methods and the network between them.

use sync_common::{StateCell, SyncCursor};

fn read_cell(row: &rusqlite::Row<'_>) -> rusqlite::Result<StateCell> {
    Ok(StateCell { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? })
}

fn parse_stored_revision(value: Option<String>) -> Result<Option<sync_common::LibraryRevision>, DatabaseError> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(DatabaseError::operation)?;
            sync_common::LibraryRevision::new(raw).map_err(DatabaseError::operation)
        })
        .transpose()
}

/// Durable marker key for an in-progress cursor recovery.
pub const CURSOR_RECOVERY_PENDING_KEY: &str = "cursor_recovery_pending";

impl Database {
    /// Reads the independent state and reading pull checkpoints.
    pub fn sync_pull_cursor(&self) -> Result<SyncCursor, DatabaseError> {
        let mut raw = None;
        self.connection.sync_pull_cursor(|row| {
            raw = Some((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?));
            Ok(())
        })?;
        let (state, reading) = raw.ok_or_else(|| DatabaseError::message("sync metadata row is missing"))?;
        Ok(SyncCursor { state_revision: parse_stored_revision(state)?, reading_revision: parse_stored_revision(reading)? })
    }

    /// Whether a rejected cursor still needs recovery.
    pub fn sync_cursor_recovery_pending(&self) -> Result<bool, DatabaseError> {
        let mut pending: Option<i64> = None;
        self.connection.sync_cursor_recovery_pending(|row| {
            pending = row.get(0)?;
            Ok(())
        })?;
        Ok(pending == Some(1))
    }

    /// Resets both pull channels and persists the recovery marker in one commit.
    pub fn sync_begin_cursor_recovery(&self) -> Result<(), DatabaseError> {
        self.connection.sync_begin_cursor_recovery().map(|_| ()).map_err(Into::into)
    }

    /// Persists both pull checkpoints. A missing revision clears its channel.
    pub fn set_sync_pull_cursor(&self, cursor: &SyncCursor) -> Result<(), DatabaseError> {
        write_cursor_revision(&self.connection, PullCursorKey::State, cursor.state_revision)?;
        write_cursor_revision(&self.connection, PullCursorKey::Reading, cursor.reading_revision)?;
        Ok(())
    }

    /// Reads one explicit sync_metadata column as text. Unknown keys read as
    /// `None`; absent rows and null columns both read as `None`.
    pub fn sync_metadata(&self, key: &str) -> Result<Option<String>, DatabaseError> {
        use crate::query_methods::first_column;
        let value: Option<Option<String>> = match key {
            "replica_id" => first_column(|read| self.connection.shared_owner_replica_id(read))?,
            "change_origin" => first_column(|read| self.connection.change_origin_select(read))?,
            "last_successful_sync_at" => first_column(|read| self.connection.shared_owner_last_successful_sync_at(read))?,
            "last_pull_state_seq" => first_column(|read| self.connection.shared_cursor_state_select(read))?,
            "last_pull_reading_seq" => first_column(|read| self.connection.shared_cursor_reading_select(read))?,
            "cursor_recovery_pending" => first_column(|read| self.connection.syncmeta_cursor_recovery_pending(read))?,
            "scan_seq" => first_column(|read| self.connection.shared_scanner_sequence(read))?,
            "cloud_storage_enabled" => first_column(|read| self.connection.syncmeta_cloud_storage_enabled(read))?,
            "remote_library_name" => first_column(|read| self.connection.shared_owner_remote_library_name(read))?,
            "server_library_created" => first_column(|read| self.connection.shared_owner_server_library_created(read))?,
            _ => return Ok(None),
        };
        Ok(value.flatten())
    }

    /// Read only the next bounded set of register identities. Local changes
    /// made behind this cursor already create durable outbox work through
    /// normal triggers.
    pub fn sync_inventory_page(&self, after: Option<&StateCell>) -> Result<Vec<StateCell>, DatabaseError> {
        let (kind, key, subkey) = after.map(|cell| (cell.kind.as_str(), cell.entity_key.as_str(), cell.entity_subkey.as_str())).unwrap_or(("", "", ""));
        let mut cells = Vec::new();
        self.connection.sync_state_inventory_page(kind, key, subkey, |row| {
            cells.push(read_cell(row)?);
            Ok(())
        })?;
        Ok(cells)
    }

    pub fn sync_inventory_checkpoint(&self) -> Result<Option<StateCell>, DatabaseError> {
        let mut raw = None;
        self.connection.sync_inventory_checkpoint(|row| {
            raw = Some((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?));
            Ok(())
        })?;
        Ok(raw.and_then(|(kind, key, subkey)| kind.map(|kind| StateCell { kind, entity_key: key.unwrap_or_default(), entity_subkey: subkey.unwrap_or_default() })))
    }

    pub fn sync_save_inventory_checkpoint(&self, after: Option<StateCell>) -> Result<(), DatabaseError> {
        if let Some(after) = after {
            self.connection.sync_save_inventory_checkpoint(&after.kind, &after.entity_key, &after.entity_subkey)?;
        } else {
            self.connection.sync_clear_inventory_checkpoint()?;
        }
        Ok(())
    }

    /// Recreates durable outbox entries for state cells confirmed absent on
    /// the server, in one transaction. Cells already queued are skipped
    /// without re-encoding; cells with no current value are skipped.
    pub fn sync_enqueue_missing_state_cells(&self, cells: &[StateCell]) -> Result<usize, DatabaseError> {
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
    pub fn sync_recover_state_page(&self, after: Option<&StateCell>) -> Result<(Option<StateCell>, usize), DatabaseError> {
        let cells = self.sync_inventory_page(after)?;
        if cells.is_empty() {
            self.clear_cursor_recovery()?;
            return Ok((None, 0));
        }
        let inserted = self.sync_enqueue_missing_state_cells(&cells)?;
        Ok((cells.last().cloned(), inserted))
    }

    fn clear_cursor_recovery(&self) -> Result<(), DatabaseError> {
        self.connection.sync_clear_cursor_recovery().map(|_| ()).map_err(Into::into)
    }
}

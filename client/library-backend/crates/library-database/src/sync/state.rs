//! Sync cursor and recovery primitives.

use crate::shared_sql::SharedSql;
use crate::sync::{write_cursor_revision, PullCursorKey, SyncApplySql, SyncStateSql};
use crate::{Database, DatabaseError};

// Sync cursor and recovery primitives.
//
// Each method is one bounded read or one bounded write in its own
// transaction. Page- and chunk-sized looping belongs to the orchestrator,
// which drives these methods and the network between them.

use sync_common::{StateAddress, SyncCursor};

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
    /// Reads the lifecycle and field pull checkpoints.
    pub fn sync_pull_cursor(&self) -> Result<SyncCursor, DatabaseError> {
        let mut raw = None;
        self.connection.sync_pull_cursor(|row| {
            raw = Some((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?));
            Ok(())
        })?;
        let (lifecycle, field) = raw.ok_or_else(|| DatabaseError::message("sync metadata row is missing"))?;
        Ok(SyncCursor { lifecycle_revision: parse_stored_revision(lifecycle)?, field_revision: parse_stored_revision(field)? })
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

    /// Resets both pull checkpoints and persists the recovery marker in one commit.
    pub fn sync_begin_cursor_recovery(&self) -> Result<(), DatabaseError> {
        self.connection.sync_begin_cursor_recovery().map(|_| ()).map_err(Into::into)
    }

    /// Persists both pull checkpoints. A missing revision clears its channel.
    pub fn set_sync_pull_cursor(&self, cursor: &SyncCursor) -> Result<(), DatabaseError> {
        write_cursor_revision(&self.connection, PullCursorKey::Lifecycle, cursor.lifecycle_revision)?;
        write_cursor_revision(&self.connection, PullCursorKey::Field, cursor.field_revision)?;
        Ok(())
    }

    /// Reads one explicit sync_metadata column as text. Unknown keys read as
    /// `None`; absent rows and null columns both read as `None`.
    pub fn sync_metadata(&self, key: &str) -> Result<Option<String>, DatabaseError> {
        use crate::query_methods::first_column;
        let value: Option<Option<String>> = match key {
            "replica_id" => first_column(|read| self.connection.shared_owner_replica_id(read))?,
            "last_successful_sync_at" => first_column(|read| self.connection.shared_owner_last_successful_sync_at(read))?,
            "last_pull_lifecycle_seq" => first_column(|read| self.connection.shared_cursor_lifecycle_select(read))?,
            "last_pull_field_seq" => first_column(|read| self.connection.shared_cursor_field_select(read))?,
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
    pub fn sync_inventory_page(&self, after: Option<&StateAddress>) -> Result<Vec<StateAddress>, DatabaseError> {
        let (kind, key, subkey) = after.map(|cell| (cell.kind.as_str(), cell.entity_key.as_str(), cell.entity_subkey.as_str())).unwrap_or(("", "", ""));
        let mut cells = Vec::new();
        self.connection.sync_state_inventory_page(kind, key, subkey, |row| {
            cells.push((|row: &rusqlite::Row<'_>| -> rusqlite::Result<StateAddress> { Ok(StateAddress { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? }) })(row)?);
            Ok(())
        })?;
        Ok(cells)
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

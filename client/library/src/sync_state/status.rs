/// Inspect durable work without requiring the current schema to be installed.
pub fn has_pending_outbox(connection: &rusqlite::Connection) -> rusqlite::Result<bool> {
    let exists: bool = connection.query_row(include_str!("sql/has_pending_outbox_select.sql"), [], |row| row.get(0))?;
    if !exists {
        return Ok(false);
    }
    connection.query_row(include_str!("sql/has_pending_outbox_select_2.sql"), [], |row| row.get(0))
}

pub fn stored_sync_status(connection: &rusqlite::Connection) -> rusqlite::Result<super::LibrarySyncDetails> {
    let empty = super::LibrarySyncDetails { has_unsynced_changes: false, last_synced_at: None };
    let initialized: bool = connection.query_row(include_str!("sql/stored_sync_status_select.sql"), [], |r| r.get(0))?;
    if !initialized {
        return Ok(empty);
    }
    super::get_library_sync_details(&connection)
}

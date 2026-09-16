//! Pull-page persistence and resumable inventory checkpoints.
use sync_common::{PullStateResponse, StateCell};

// Shared by the native store and the account-checked browser command adapter.
pub fn apply_pull_page(conn: &mut rusqlite::Connection, page: &PullStateResponse) -> Result<bool, Box<dyn std::error::Error>> {
    if page.mutations.is_empty() && crate::sync_state::get_pull_cursor(conn)? == page.next_cursor {
        return Ok(false);
    }
    let prepared = crate::remote_reducer::prepare_remote_changes(&page.mutations)?;
    let tx = conn.transaction()?;
    let changed = apply_prepared_pull_page_in_tx(&tx, page, &prepared)?;
    tx.commit()?;
    Ok(changed)
}

pub fn apply_prepared_pull_page_in_tx(tx: &rusqlite::Transaction<'_>, page: &PullStateResponse, prepared: &crate::remote_reducer::PreparedRemoteChanges<'_>) -> Result<bool, Box<dyn std::error::Error>> {
    if page.mutations.is_empty() && crate::sync_state::get_pull_cursor(tx)? == page.next_cursor {
        return Ok(false);
    }
    tx.pragma_update(None, "defer_foreign_keys", "ON")?;
    crate::remote_reducer::apply_prepared_remote_changes_in_tx(tx, prepared)?;
    crate::sync_state::set_pull_cursor(&tx, page.next_cursor)?;
    Ok(!page.mutations.is_empty())
}

pub fn inventory_checkpoint(conn: &rusqlite::Connection) -> Result<Option<StateCell>, Box<dyn std::error::Error>> {
    crate::sync_state::get_sync_metadata(conn, "inventory_repair_after")?.map(|value| serde_json::from_str(&value).map_err(Into::into)).transpose()
}

pub fn save_inventory_checkpoint(conn: &rusqlite::Connection, after: Option<StateCell>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(after) = after {
        crate::sync_state::set_sync_metadata(conn, "inventory_repair_after", &serde_json::to_string(&after)?)?;
    } else {
        conn.execute(include_str!("sql/exchange/save_inventory_checkpoint_delete.sql"), [])?;
    }
    Ok(())
}

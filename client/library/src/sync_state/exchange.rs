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
    use rusqlite::OptionalExtension;
    let row: Option<(Option<String>, Option<String>, Option<String>)> = conn
        .query_row("SELECT inventory_repair_kind, inventory_repair_key, inventory_repair_subkey FROM sync_metadata WHERE singleton = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .optional()?;
    Ok(row.and_then(|(kind, key, subkey)| kind.map(|kind| StateCell { kind, entity_key: key.unwrap_or_default(), entity_subkey: subkey.unwrap_or_default() })))
}

pub fn save_inventory_checkpoint(conn: &rusqlite::Connection, after: Option<StateCell>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(after) = after {
        conn.execute(
            "INSERT INTO sync_metadata(singleton, inventory_repair_kind, inventory_repair_key, inventory_repair_subkey) VALUES (1, ?1, ?2, ?3)
            ON CONFLICT(singleton) DO UPDATE SET inventory_repair_kind = excluded.inventory_repair_kind, inventory_repair_key = excluded.inventory_repair_key, inventory_repair_subkey = excluded.inventory_repair_subkey",
            rusqlite::params![after.kind, after.entity_key, after.entity_subkey],
        )?;
    } else {
        conn.execute(include_str!("sql/exchange/save_inventory_checkpoint_delete.sql"), [])?;
    }
    Ok(())
}

#[cfg(test)]
use crate::schema::init_db;
use crate::schema::library_write_transaction;
use crate::storage_values::invalid_text_column;
use crate::storage_values::unix_millis;
use crate::sync_state::queries::SyncSql;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use sync_common::ContentHash;
use sync_common::DirId;
use sync_common::StateCell;

#[cfg(test)]
#[path = "common_sync_benchmarks.rs"]
mod common_sync_benchmarks;

pub const CURSOR_RECOVERY_PENDING_KEY: &str = "cursor_recovery_pending";

pub fn state_inventory(conn: &Connection) -> Result<Vec<StateCell>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/inventory/state_inventory_select.sql"))?;
    let cells = statement.query_map([], |row| Ok(StateCell { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? }))?.collect();
    cells
}

/// Recreates durable outbox entries for state cells confirmed absent on the
/// server. Current domain values are encoded by the same SQLite functions used
/// by ordinary mutation triggers, so repair cannot drift from normal sync.
pub fn enqueue_missing_state_cells(conn: &mut Connection, cells: &[StateCell]) -> Result<usize, rusqlite::Error> {
    if cells.is_empty() {
        return Ok(0);
    }
    let transaction = library_write_transaction(conn)?;
    let inserted = enqueue_missing_state_cells_in_tx(&transaction, cells)?;
    transaction.commit()?;
    Ok(inserted)
}

pub fn enqueue_missing_state_cells_in_tx(transaction: &Transaction<'_>, cells: &[StateCell]) -> Result<usize, rusqlite::Error> {
    let changed_at = unix_millis()?;
    let mut inserted = 0;
    let mut exists = transaction.prepare_cached(include_str!("sql/inventory/enqueue_missing_state_cells_in_tx_select.sql"))?;
    for cell in cells {
        if exists.query_row(rusqlite::params![cell.kind, cell.entity_key, cell.entity_subkey], |row| row.get::<_, bool>(0))? {
            continue;
        }
        let (body, conflict_rank) = current_state_body(&transaction, cell)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        inserted += transaction.execute(include_str!("sql/inventory/enqueue_missing_state_cells_in_tx_insert.sql"), rusqlite::params![cell.kind, cell.entity_key, cell.entity_subkey, body, changed_at, conflict_rank])?;
    }
    Ok(inserted)
}

pub fn cursor_recovery_pending(conn: &Connection) -> Result<bool, rusqlite::Error> {
    Ok(get_sync_metadata(conn, CURSOR_RECOVERY_PENDING_KEY)?.as_deref() == Some("1"))
}

/// Resets both pull channels and persists the recovery marker in one commit.
pub fn begin_cursor_recovery(conn: &Connection) -> Result<(), rusqlite::Error> {
    let transaction = conn.unchecked_transaction()?;
    begin_cursor_recovery_in_tx(&transaction)?;
    transaction.commit()
}

/// Begin recovery within the caller's account-validated write transaction.
pub fn begin_cursor_recovery_in_tx(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    set_pull_cursor(transaction, sync_common::SyncCursor::default())?;
    set_sync_metadata(transaction, CURSOR_RECOVERY_PENDING_KEY, "1")
}

/// Synchronous recovery with a bounded transaction per page. Async callers
/// should schedule pages individually so other storage commands can run.
pub fn complete_cursor_recovery(conn: &mut Connection) -> Result<usize, rusqlite::Error> {
    let mut inserted = 0;
    let mut after = None;
    loop {
        let (next, count) = recover_state_page(conn, after.as_ref())?;
        inserted += count;
        if next.is_none() {
            return Ok(inserted);
        }
        after = next;
    }
}

fn current_state_body(transaction: &Transaction<'_>, cell: &StateCell) -> Result<Option<(Vec<u8>, i64)>, rusqlite::Error> {
    let key = cell.entity_key.as_str();
    let subkey = cell.entity_subkey.as_str();
    let row = match cell.kind.as_str() {
        "directory_name" => transaction.query_row(include_str!("sql/inventory/current_state_body_select.sql"), [key], pair).optional()?,
        "directory_parent" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_2.sql"), [key], pair).optional()?,
        "directory_lifecycle" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_3.sql"), [key], pair).optional()?,
        "book_lifecycle" => {
            let lifecycle = transaction.query_row(include_str!("sql/inventory/current_state_body_select_4.sql"), [key], |row| row.get::<_, i64>(0)).optional()?;
            match lifecycle {
                Some(2) => transaction.query_row(include_str!("sql/inventory/current_state_body_select_5.sql"), [key], pair).optional()?,
                Some(1) => transaction.query_row(include_str!("sql/inventory/current_state_body_select_6.sql"), [key], pair).optional()?,
                Some(0) => transaction.query_row(include_str!("sql/inventory/current_state_body_select_7.sql"), [key], pair).optional()?,
                Some(_) | None => None,
            }
        }
        "placement" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_8.sql"), [key, subkey], pair).optional()?,
        "reading_position" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_9.sql"), [key], pair).optional()?,
        "description" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_10.sql"), [key], pair).optional()?,
        "metadata" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_11.sql"), [key], pair).optional()?,
        "annotation" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_12.sql"), [key], pair).optional()?,
        "catalog_publication" => transaction.query_row(include_str!("sql/inventory/current_state_body_select_13.sql"), [key, subkey], pair).optional()?,
        _ => None,
    };
    Ok(row)
}

fn pair(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Vec<u8>, i64)> {
    Ok((row.get(0)?, row.get(1)?))
}
pub fn get_sync_metadata(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    let mut value: Option<String> = None;
    conn.get_sync_metadata_value(key, |row| {
        value = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(value)
}
pub fn set_sync_metadata(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.set_sync_metadata(key, value)?;
    Ok(())
}

fn parse_revision(value: Option<String>) -> Result<Option<sync_common::LibraryRevision>, rusqlite::Error> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))?;
            sync_common::LibraryRevision::new(raw).map_err(|error| invalid_text_column(0, error.to_string()))
        })
        .transpose()
}

/// Reads the independent state and reading pull checkpoints.
pub fn get_pull_cursor(conn: &Connection) -> Result<sync_common::SyncCursor, rusqlite::Error> {
    let state = get_sync_metadata(conn, "last_pull_state_seq")?;
    let reading = get_sync_metadata(conn, "last_pull_reading_seq")?;
    Ok(sync_common::SyncCursor { state_revision: parse_revision(state)?, reading_revision: parse_revision(reading)? })
}

pub fn set_pull_cursor(conn: &Connection, cursor: sync_common::SyncCursor) -> Result<(), rusqlite::Error> {
    for (key, revision) in [("last_pull_state_seq", cursor.state_revision), ("last_pull_reading_seq", cursor.reading_revision)] {
        if let Some(revision) = revision {
            set_sync_metadata(conn, key, &revision.get().to_string())?;
        } else {
            conn.execute(include_str!("sql/inventory/legacy_recovery_delete.sql"), [key])?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibrarySyncDetails {
    pub has_unsynced_changes: bool,
    pub last_synced_at: Option<u64>,
}

pub fn get_library_sync_details(conn: &Connection) -> Result<LibrarySyncDetails, rusqlite::Error> {
    let has_unsynced_changes = conn.query_row(include_str!("sql/inventory/get_library_sync_details_select.sql"), [], |row| row.get(0))?;
    let last_synced_at = get_sync_metadata(conn, "last_successful_sync_at")?.map(|value| value.parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))).transpose()?;
    Ok(LibrarySyncDetails { has_unsynced_changes, last_synced_at })
}

/// Starts remote reconciliation from a clean account boundary while retaining
/// every local book, placement, annotation, and pending mutation.
pub fn rebind_sync_account(conn: &Connection, user_id: &str) -> Result<(), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    crate::assets::requeue_local_uploads(&tx)?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_delete.sql"), [])?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_insert.sql"), [])?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_delete_2.sql"), [])?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_delete_3.sql"), [])?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_delete_4.sql"), [])?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_delete_5.sql"), [])?;
    tx.execute(include_str!("sql/inventory/rebind_sync_account_insert_2.sql"), [user_id])?;
    tx.commit()
}
/// A placement removed by synchronization so the caller can reconcile its file.
#[derive(Debug, Clone)]
pub struct RemovedBookDir {
    pub dir_id: DirId,
    pub content_hash: ContentHash,
}

/// Read only the next bounded set of register identities. Local changes made
/// behind this cursor already create durable outbox work through normal triggers.
pub fn state_inventory_page(conn: &Connection, after: Option<&StateCell>) -> Result<Vec<StateCell>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/inventory/state_inventory_page_select.sql"))?;
    let (kind, key, subkey) = after.map(|cell| (cell.kind.as_str(), cell.entity_key.as_str(), cell.entity_subkey.as_str())).unwrap_or(("", "", ""));
    let cells = statement.query_map(rusqlite::params![kind, key, subkey], |row| Ok(StateCell { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? }))?.collect();
    cells
}

/// Recovery commits one page at a time while retaining its durable marker.
/// After interruption, restarting from the first page is safe because enqueue
/// preserves existing outbox identities. Only an empty final page clears it.
pub fn recover_state_page(conn: &mut Connection, after: Option<&StateCell>) -> Result<(Option<StateCell>, usize), rusqlite::Error> {
    let tx = library_write_transaction(conn)?;
    let result = recover_state_page_in_tx(&tx, after)?;
    tx.commit()?;
    Ok(result)
}

/// Selection, encoding, and marker completion share the caller's writer
/// reservation, including its account-binding check. At most 256 cells are
/// inspected; existing outbox payloads are neither encoded nor overwritten.
pub fn recover_state_page_in_tx(tx: &Transaction<'_>, after: Option<&StateCell>) -> Result<(Option<StateCell>, usize), rusqlite::Error> {
    let cells = state_inventory_page(tx, after)?;
    if cells.is_empty() {
        tx.execute(include_str!("sql/inventory/legacy_recovery_delete.sql"), [CURSOR_RECOVERY_PENDING_KEY])?;
        return Ok((None, 0));
    }
    let inserted = enqueue_missing_state_cells_in_tx(tx, &cells)?;
    Ok((cells.last().cloned(), inserted))
}

#[cfg(test)]
mod recovery_benchmarks {
    use super::*;
    use std::time::{Duration, Instant};

    // Reproduce the former native recovery path, including encoding entries
    // which ON CONFLICT subsequently discards. Benchmark only, not production.
    fn legacy_recovery(conn: &mut Connection) -> usize {
        let tx = library_write_transaction(conn).unwrap();
        let cells = state_inventory(&tx).unwrap();
        let changed_at = unix_millis().unwrap();
        let mut inserted = 0;
        for cell in cells {
            let (body, rank) = current_state_body(&tx, &cell).unwrap().unwrap();
            inserted += tx.execute(include_str!("sql/inventory/legacy_recovery_insert.sql"), rusqlite::params![cell.kind, cell.entity_key, cell.entity_subkey, body, changed_at, rank]).unwrap();
        }
        tx.execute(include_str!("sql/inventory/legacy_recovery_delete.sql"), [CURSOR_RECOVERY_PENDING_KEY]).unwrap();
        tx.commit().unwrap();
        inserted
    }

    #[test]
    #[ignore = "manual release disk-WAL recovery benchmark; temporary synthetic libraries"]
    fn benchmark_desktop_cursor_recovery() {
        run_recovery_benchmark(&[1000, 5000], &[1050]);
    }

    #[test]
    #[ignore = "manual release description-size comparison; temporary synthetic libraries"]
    fn benchmark_desktop_recovery_description_sizes() {
        run_recovery_benchmark(&[1000], &[0, 1024, 10240]);
    }

    fn run_recovery_benchmark(book_counts: &[i32], description_sizes: &[usize]) {
        for &books in book_counts {
            for &description_bytes in description_sizes {
                let root = tempfile::tempdir().unwrap();
                let mut conn = Connection::open(root.path().join("library.db")).unwrap();
                init_db(&conn).unwrap();
                let tx = library_write_transaction(&mut conn).unwrap();
                let description = "Synthetic recovery benchmark description. ".repeat(description_bytes / 42 + 1)[..description_bytes].to_owned();
                for id in 1..=books {
                    tx.execute(include_str!("sql/inventory/run_recovery_benchmark_insert.sql"), [format!("{id:064x}")]).unwrap();
                    tx.execute(include_str!("sql/inventory/run_recovery_benchmark_insert_2.sql"), rusqlite::params![format!("{id:064x}"), description]).unwrap();
                }
                // Two sync cells per book: lifecycle plus independently edited metadata.
                tx.execute(include_str!("sql/inventory/run_recovery_benchmark_update.sql"), []).unwrap();
                tx.commit().unwrap();
                let cells = state_inventory(&conn).unwrap().len();
                let journal: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
                let synchronous: i64 = conn.pragma_query_value(None, "synchronous", |r| r.get(0)).unwrap();
                assert_eq!(journal, "wal");
                eprintln!("fixture: books={books} cells={cells} description_bytes={} journal={journal} synchronous={synchronous}", description.len());
                for populated in [false, true] {
                    let mut totals = [Vec::new(), Vec::new()];
                    let mut longest = [Vec::new(), Vec::new()];
                    for trial in 0..5 {
                        // Alternate execution order to reduce systematic cache bias.
                        for batched in if trial % 2 == 0 { [false, true] } else { [true, false] } {
                            conn.execute(include_str!("sql/common_sync_benchmarks/fixture_delete.sql"), []).unwrap();
                            if populated {
                                complete_cursor_recovery(&mut conn).unwrap();
                            }
                            begin_cursor_recovery(&conn).unwrap();
                            // Exclude setup's outstanding WAL frames from the measured
                            // workload, while leaving SQLite's auto-checkpoint enabled.
                            let checkpoint: (i64, i64, i64) = conn.query_row(include_str!("sql/inventory/run_recovery_benchmark_pragma.sql"), [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
                            assert_eq!(checkpoint.0, 0);
                            let mut max_write = Duration::ZERO;
                            let start = Instant::now();
                            let inserted = if batched {
                                let mut after = None;
                                let mut inserted = 0;
                                loop {
                                    let page_start = Instant::now();
                                    let (next, count) = recover_state_page(&mut conn, after.as_ref()).unwrap();
                                    max_write = max_write.max(page_start.elapsed());
                                    inserted += count;
                                    if next.is_none() {
                                        break;
                                    }
                                    after = next;
                                    // Model native recovery's pause, without a live storage queue.
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                                inserted
                            } else {
                                let inserted = legacy_recovery(&mut conn);
                                max_write = start.elapsed();
                                inserted
                            };
                            totals[usize::from(batched)].push(start.elapsed());
                            longest[usize::from(batched)].push(max_write);
                            assert_eq!(inserted, if populated { 0 } else { cells });
                            assert!(!cursor_recovery_pending(&conn).unwrap());
                            assert_eq!(conn.query_row(include_str!("sql/inventory/run_recovery_benchmark_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), cells as i64);
                        }
                    }
                    for i in 0..2 {
                        totals[i].sort();
                        longest[i].sort();
                        eprintln!(
                            "books={books} description_bytes={description_bytes} populated={populated} batched={} trials=5: total_median={:?} longest_transaction_median={:?} worst_transaction={:?}",
                            i == 1,
                            totals[i][2],
                            longest[i][2],
                            longest[i][4]
                        );
                    }
                }
            }
        }
    }
}

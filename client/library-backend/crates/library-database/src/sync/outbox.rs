//! Sync outbox snapshots and acknowledgements.

use crate::query_methods::{blob_column, optional_blob_column, query_rows};
pub(crate) use crate::sync::SyncOutboxSql;
use crate::sync::apply::registers::publication_snapshot;
use crate::sync::{MutationIssuesSql, storage_i64};
use crate::transactions::WriteOutcome::Commit;
pub(crate) use crate::{Database, DatabaseError};
use content_address::BookId;
pub(crate) use library_replica::{LibraryStateEntry, StatePublication};
use rusqlite::{Connection, Row, named_params};
use std::collections::HashSet;
pub(crate) use sync_common::ReplicaSeq;
use sync_common::StateKey;

const OUTBOX_PAGE_ROWS: usize = 256;
const OUTBOX_PAGE_BYTES: usize = 256 * 1024;

// Sync outbox snapshots and acknowledgements.
//
// The orchestrator reads a publishable snapshot, performs its network work
// with no database reference held, then commits acknowledgements back in
// caller-sized batches. Prepared pages use short reads without a writer.

#[derive(Debug)]
pub(crate) struct OutboxRow<'a> {
    pub(crate) state_kind: String,
    pub(crate) state_key: StateKey,
    pub(crate) body: &'a [u8],
    pub(crate) changed_at: i64,
    pub(crate) replica_seq: i64,
    pub(crate) state_subkey: String,
    pub(crate) winning_version: Option<&'a [u8]>,
}

pub(crate) fn read_outbox_row<'a>(row: &'a Row<'_>) -> rusqlite::Result<OutboxRow<'a>> {
    Ok(OutboxRow { state_kind: row.get(0)?, state_key: row.get(1)?, body: blob_column(row, 2)?, changed_at: row.get(3)?, replica_seq: row.get(4)?, state_subkey: row.get(5)?, winning_version: optional_blob_column(row, 6)? })
}

pub(crate) fn decode_outbox_row(row: OutboxRow<'_>) -> Result<StatePublication, DatabaseError> {
    let corrupt = |reason: &'static str| DatabaseError::message(format!("corrupt local sync mutation at replica sequence {}: {reason}", row.replica_seq));
    let body: LibraryStateEntry = sync_common::wire::decode(row.body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|_| corrupt("body is not a canonical mutation"))?;
    let (kind, key, subkey) = crate::sync::apply::sync_state_identity(&body);
    if row.state_kind != kind || row.state_key != key || row.state_subkey != subkey {
        return Err(corrupt("stored coalescing identity does not match the mutation body"));
    }
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let publisher_seq = ReplicaSeq::new(u64::try_from(row.replica_seq).map_err(|_| corrupt("negative replica sequence"))?).map_err(|_| corrupt("replica sequence must be positive"))?;
    let bytes = row.winning_version.ok_or_else(|| corrupt("missing winning version"))?;
    let version: sync_common::VersionKey = sync_common::wire::decode(bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
    if version.changed_at != changed_at || version.conflict_rank != body.conflict_rank() {
        return Err(corrupt("winning version does not match its value"));
    }
    Ok(sync_common::Publication { publisher_seq, state: sync_common::Versioned { value: body, version } })
}

/// Prepared queue bounds and reading-position exclusions. No database lease.
#[derive(Clone)]
pub struct OutboxScan {
    through: i64,
    after: i64,
    lifecycles: bool,
    finished: bool,
    reading_positions: HashSet<BookId>,
}

fn record_corrupt_publication(connection: &Connection, sequence: i64, error: &DatabaseError) -> Result<(), DatabaseError> {
    let sequence_id = ReplicaSeq::new(u64::try_from(sequence).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?;
    let issue = sync_common::wire::encode(&sync_common::MutationIssue::Untransmittable(sequence_id)).map_err(DatabaseError::operation)?;
    connection.sync_issue_record(sequence, 2, &issue)?;
    log::error!("sync publication {sequence} was isolated without deleting it: {error}");
    Ok(())
}

impl Database {
    /// Freeze and select a publication snapshot under one writer reservation.
    pub(crate) fn with_publications<T>(&self, read: impl FnOnce(&Connection) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        self.with_write_transaction(|tx| {
            let pending = query_rows(
                |callback| tx.outbox_pending_snapshots(callback),
                |r| {
                    let fields = (|| -> rusqlite::Result<_> { Ok((r.get::<_, String>(0)?, r.get::<_, StateKey>(1)?, r.get::<_, String>(2)?)) })();
                    Ok((r.get::<_, i64>(3)?, fields))
                },
            )?;
            for (seq, fields) in pending {
                let (kind, key, subkey) = match fields {
                    Ok(value) => value,
                    Err(error) => {
                        record_corrupt_publication(tx, seq, &DatabaseError::from(error))?;
                        continue;
                    }
                };
                let snapshot = match publication_snapshot(tx, &kind, &key, &subkey) {
                    Ok(value) => value,
                    Err(error) => {
                        record_corrupt_publication(tx, seq, &error)?;
                        continue;
                    }
                };
                let Some((bytes, current)) = snapshot else {
                    record_corrupt_publication(tx, seq, &DatabaseError::message("queued publication has no canonical value"))?;
                    continue;
                };
                let version = sync_common::wire::encode(&current).map_err(DatabaseError::operation)?;
                // Recovery forwards the exact retained version, never a newer edit.
                tx.outbox_freeze_snapshot(&bytes, &(storage_i64(current.changed_at, "publication timestamp")?), &version, &seq)?;
            }
            Ok(Commit(read(tx)?))
        })
    }

    /// Fixes the end of an upload scan so continuous local edits cannot keep
    /// it running. The coordinator pages up to this sequence number.
    pub fn sync_outbox_watermark(&self) -> Result<i64, DatabaseError> {
        Ok(self.connection.sync_outbox_watermark(|row| row.get(0))?)
    }

    /// The coordinator supplies its publication policy as a value snapshot.
    /// Storage freezes these exclusions for the duration of this bounded scan.
    pub fn sync_prepare_outbox_excluding_positions(&self, waiting: HashSet<BookId>) -> Result<OutboxScan, DatabaseError> {
        self.with_publications(|connection| {
            let through = connection.sync_outbox_watermark(|row| row.get(0))?;
            Ok(OutboxScan { through, after: i64::MIN, lifecycles: true, finished: false, reading_positions: waiting })
        })
    }

    /// Short read pages: no repeated freezing, projection, or dependency scan.
    /// Corrupt publications receive diagnostics after releasing the reader.
    /// Lifecycles precede fields across the entire watermark, not just each page.
    pub fn sync_next_outbox_page(&self, scan: &mut OutboxScan) -> Result<Vec<StatePublication>, DatabaseError> {
        while !scan.finished {
            let (page, after, done) = self.read_publication_page(&self.connection, &scan.reading_positions, scan.after, scan.through, scan.lifecycles)?;
            scan.after = after;
            if done {
                if scan.lifecycles {
                    scan.lifecycles = false;
                    scan.after = i64::MIN;
                } else {
                    scan.finished = true;
                }
            }
            if !page.is_empty() {
                return Ok(page);
            }
        }
        Ok(Vec::new())
    }

    // Stream each SQL page, stop at
    // the publication budget, and continue scanning past deferred rows.
    fn read_publication_page(&self, connection: &Connection, reading_positions: &HashSet<BookId>, after: i64, through: i64, lifecycles: bool) -> Result<(Vec<StatePublication>, i64, bool), DatabaseError> {
        let mut corrupt = Vec::new();
        let result = (|| -> Result<_, DatabaseError> {
            let mut statement = connection.prepare_cached(include_str!("sql/outbox_page.sql"))?;
            let mut mutations = Vec::new();
            let mut bytes = 0usize;
            let mut scanned_through = after;
            loop {
                let mut rows = statement.query(named_params! { ":after": scanned_through, ":through": through, ":limit": OUTBOX_PAGE_ROWS as i64, ":lifecycles": lifecycles })?;
                let mut scanned = 0;
                while let Some(row) = rows.next()? {
                    let sequence: i64 = row.get(4)?;
                    let decoded = (|| {
                        let stored = read_outbox_row(row)?;
                        let body_bytes = stored.body.len();
                        Ok::<_, DatabaseError>((decode_outbox_row(stored)?, body_bytes))
                    })();
                    let (mutation, body_bytes) = match decoded {
                        Ok(value) => value,
                        Err(error) => {
                            corrupt.push((sequence, error));
                            scanned_through = sequence;
                            scanned += 1;
                            continue;
                        }
                    };
                    if !mutations.is_empty() && body_bytes > OUTBOX_PAGE_BYTES.saturating_sub(bytes) {
                        // Leave this row for the next bounded page.
                        return Ok((mutations, scanned_through, false));
                    }
                    scanned_through = sequence;
                    scanned += 1;
                    let position_ready = match &mutation.state.value {
                        LibraryStateEntry::ReadingPosition { book_id, .. } => !reading_positions.contains(book_id),
                        _ => true,
                    };
                    if !position_ready {
                        continue;
                    }
                    bytes += body_bytes;
                    mutations.push(mutation);
                    if bytes >= OUTBOX_PAGE_BYTES || mutations.len() >= OUTBOX_PAGE_ROWS {
                        return Ok((mutations, scanned_through, false));
                    }
                }
                if scanned < OUTBOX_PAGE_ROWS {
                    return Ok((mutations, scanned_through, true));
                }
            }
        })()?;
        // Finish the read before writing durable per-entry diagnostics. Leave
        // corrupt rows intact, excluded until a newer edit supersedes them.
        if !corrupt.is_empty() {
            self.write(|tx| {
                for (sequence, error) in &corrupt {
                    record_corrupt_publication(tx, *sequence, error)?;
                }
                Ok(())
            })?;
        }
        Ok(result)
    }

    /// Acknowledges exactly these publisher sequences in one transaction. Callers
    /// batch large id sets into repeated calls; batching policy belongs to
    /// the orchestrator, not to storage.
    pub fn sync_acknowledge_mutations(&self, ids: &[ReplicaSeq]) -> Result<(), DatabaseError> {
        if ids.is_empty() {
            return Ok(());
        }
        let payload = serde_json::to_string(ids).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            transaction.sync_acknowledge_mutations(&payload)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn prepared_pages_do_not_reserve_a_writer_or_freeze_later_edits() {
        let path = std::env::temp_dir().join(format!("prepared-outbox-{}.db", uuid::Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        db.create_directory_with_id(&uuid::Uuid::from_u128(1), &sync_common::ROOT_DIR_ID, &"First".into()).unwrap();
        let mut scan = db.sync_prepare_outbox_excluding_positions(Default::default()).unwrap();
        db.create_directory_with_id(&uuid::Uuid::from_u128(2), &sync_common::ROOT_DIR_ID, &"Later".into()).unwrap();
        db.connection.busy_timeout(std::time::Duration::from_millis(100)).unwrap();
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let page = db.sync_next_outbox_page(&mut scan).unwrap();
        assert!(!page.is_empty());
        assert!(page.iter().all(|p| p.publisher_seq.get() as i64 <= scan.through));
        assert!(db.sync_next_outbox_page(&mut scan).unwrap().is_empty());
        writer.execute_batch("ROLLBACK").unwrap();
        let later: bool = db.connection.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE replica_seq>?1 AND body IS NULL)", [scan.through], |row| row.get(0)).unwrap();
        assert!(later, "reading prepared pages must not freeze new edits");
        drop(writer);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn sync_outbox_watermark() -> i64;
    arguments () = ();
    execute database, arguments => database.sync_outbox_watermark();

    fn sync_prepare_outbox_excluding_positions(waiting: HashSet<BookId>) -> OutboxScan;
    arguments (HashSet<BookId>,) = (waiting,);
    execute database, arguments => database.sync_prepare_outbox_excluding_positions(arguments.0);

    fn sync_acknowledge_mutations(ids: &[ReplicaSeq]) -> ();
    arguments (Vec<ReplicaSeq>,) = (ids.to_vec(),);
    execute database, arguments => database.sync_acknowledge_mutations(&arguments.0);

    fn outbox_page(scan: OutboxScan) -> (OutboxScan, Vec<StatePublication>);
        arguments (OutboxScan,) = (scan,);
        execute database, arguments => {
            let mut scan = arguments.0;
            let page = database.sync_next_outbox_page(&mut scan)?;
            Ok((scan, page))
        };
}

impl crate::DatabaseClient {
    pub async fn sync_next_outbox_page(&self, scan: &mut OutboxScan) -> Result<Vec<StatePublication>, DatabaseError> {
        let (next, page) = self.outbox_page(scan.clone()).await?;
        *scan = next;
        Ok(page)
    }
}

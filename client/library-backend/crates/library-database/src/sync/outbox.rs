//! Sync outbox snapshots and acknowledgements.

use crate::query_methods::{blob_column, optional_blob_column, query_rows};
use crate::sync::apply::registers::publication_snapshot;
pub(crate) use crate::sync::SyncOutboxSql;
use crate::sync::storage_i64;
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

/// All book state waits for its draft to complete. Non-removal state also
/// waits for the book bytes when cloud storage is enabled.
struct PublicationDependencies {
    drafts: HashSet<BookId>,
    uploads: HashSet<BookId>,
    reading_positions: HashSet<BookId>,
}

/// Prepared queue bounds and dependencies. Contains no connection or transaction.
/// The unique outbox identity already coalesces each value before this scan.
pub struct OutboxScan {
    through: i64,
    after: i64,
    lifecycles: bool,
    finished: bool,
    pending: PublicationDependencies,
}

fn book_ready(mutation: &StatePublication, pending: &PublicationDependencies) -> bool {
    mutation.state.value.book().is_none_or(|hash| !pending.drafts.contains(&hash)) && mutation.state.value.upload_dependency().is_none_or(|hash| !pending.uploads.contains(&hash))
}

/// Content hashes whose exact local book version is still uploading. Pending
/// state stays durable until completion releases it.
fn outbox_pending_uploads(connection: &Connection) -> Result<PublicationDependencies, DatabaseError> {
    let cloud_storage_enabled = connection.sync_cloud_storage_enabled(|row| row.get::<_, Option<i64>>(0))?;
    let mut pending = PublicationDependencies { drafts: HashSet::new(), uploads: HashSet::new(), reading_positions: HashSet::new() };
    connection.sync_pending_book_drafts(|row| {
        pending.drafts.insert(row.get::<_, BookId>(0)?);
        Ok(())
    })?;
    if cloud_storage_enabled == Some(0) {
        return Ok(pending);
    }
    connection.sync_pending_uploads(|row| {
        pending.uploads.insert(row.get::<_, BookId>(0)?);
        Ok(())
    })?;
    Ok(pending)
}

impl Database {
    /// Freeze and select a publication snapshot under one writer reservation.
    pub(crate) fn with_publications<T>(&self, read: impl FnOnce(&Connection) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        self.with_write_transaction(|tx| {
            let pending = query_rows(|callback| tx.outbox_pending_snapshots(callback), |r| Ok((r.get::<_, String>(0)?, r.get::<_, StateKey>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
            for (kind, key, subkey, seq) in pending {
                let Some((bytes, current)) = publication_snapshot(tx, &kind, &key, &subkey)? else { continue };
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
            let mut pending = outbox_pending_uploads(connection)?;
            pending.reading_positions = waiting;
            Ok(OutboxScan { through, after: i64::MIN, lifecycles: true, finished: false, pending })
        })
    }

    /// Short read-only pages: no repeated freezing, projection, or dependency scan.
    /// Lifecycles precede fields across the entire watermark, not just each page.
    pub fn sync_next_outbox_page(&self, scan: &mut OutboxScan) -> Result<Vec<StatePublication>, DatabaseError> {
        while !scan.finished {
            let (page, after, done) = self.read_publication_page(&self.connection, &scan.pending, scan.after, scan.through, scan.lifecycles)?;
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
    fn read_publication_page(&self, connection: &Connection, pending: &PublicationDependencies, after: i64, through: i64, lifecycles: bool) -> Result<(Vec<StatePublication>, i64, bool), DatabaseError> {
        let mut statement = connection.prepare_cached(include_str!("sql/outbox_page.sql"))?;
        let mut mutations = Vec::new();
        let mut bytes = 0usize;
        let mut scanned_through = after;
        loop {
            let mut rows = statement.query(named_params! { ":after": scanned_through, ":through": through, ":limit": OUTBOX_PAGE_ROWS as i64, ":lifecycles": lifecycles })?;
            let mut scanned = 0;
            while let Some(row) = rows.next()? {
                let stored = read_outbox_row(row)?;
                let body_bytes = stored.body.len();
                if !mutations.is_empty() && body_bytes > OUTBOX_PAGE_BYTES.saturating_sub(bytes) {
                    // Do not decode or advance past the next page's first row.
                    return Ok((mutations, scanned_through, false));
                }
                scanned_through = stored.replica_seq;
                scanned += 1;
                let mutation = decode_outbox_row(stored)?;
                let position_ready = match &mutation.state.value {
                    LibraryStateEntry::ReadingPosition { book_id, .. } => !pending.reading_positions.contains(book_id),
                    _ => true,
                };
                if !book_ready(&mutation, pending) || !position_ready {
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

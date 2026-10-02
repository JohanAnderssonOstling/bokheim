//! Sync outbox snapshots and acknowledgements.

use std::collections::HashSet;
use crate::transactions::WriteOutcome::Commit;
pub(crate) use crate::sync::SyncOutboxSql;
pub(crate) use crate::{Database, DatabaseError};
pub(crate) use library_replica::{LibraryStateEntry, StatePublication};
pub(crate) use sync_common::ReplicaSeq;

// Sync outbox snapshots and acknowledgements.
//
// The orchestrator reads a publishable snapshot, performs its network work
// with no database reference held, then commits acknowledgements back in
// caller-sized batches. One method call is one transaction.

#[derive(Clone, Debug)]
pub(crate) struct OutboxRow {
    pub(crate) state_kind: String,
    pub(crate) state_key: String,
    pub(crate) body: Vec<u8>,
    pub(crate) changed_at: i64,
    pub(crate) replica_seq: i64,
    pub(crate) state_subkey: String,
    pub(crate) winning_version: Option<Vec<u8>>,
}

pub(crate) fn read_outbox_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxRow> {
    Ok(OutboxRow { state_kind: row.get(0)?, state_key: row.get(1)?, body: row.get(2)?, changed_at: row.get(3)?, replica_seq: row.get(4)?, state_subkey: row.get(5)?, winning_version: row.get(6)? })
}

pub(crate) fn decode_outbox_row(row: OutboxRow) -> Result<StatePublication, DatabaseError> {
    let corrupt = |reason: &'static str| DatabaseError::message(format!("corrupt local sync mutation at replica sequence {}: {reason}", row.replica_seq));
    let body: LibraryStateEntry = sync_common::wire::decode(&row.body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|_| corrupt("body is not a canonical mutation"))?;
    let (kind, key, subkey) = crate::sync::apply::sync_state_identity(&body);
    if row.state_kind != kind || row.state_key != key || row.state_subkey != subkey {
        return Err(corrupt("stored coalescing identity does not match the mutation body"));
    }
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let publisher_seq = ReplicaSeq::new(u64::try_from(row.replica_seq).map_err(|_| corrupt("negative replica sequence"))?).map_err(|_| corrupt("replica sequence must be positive"))?;
    let bytes = row.winning_version.as_deref().ok_or_else(|| corrupt("missing winning version"))?;
    let version: sync_common::VersionKey = sync_common::wire::decode(bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
    if version.changed_at != changed_at || version.conflict_rank != body.conflict_rank() {
        return Err(corrupt("winning version does not match its value"));
    }
    Ok(sync_common::Publication { publisher_seq, state: sync_common::Versioned { value: body, version } })
}

/// Pending state stays durable while its exact local book version is
/// uploading (including rejected/retryable jobs). Only completion releases it.
pub(crate) fn book_ready(mutation: &StatePublication, pending: &HashSet<String>) -> bool {
    mutation.state.value.upload_dependency().is_none_or(|hash| !pending.contains(hash.as_str()))
}

/// Content hashes whose exact local book version is still uploading. Pending
/// state stays durable until completion releases it.
pub(crate) fn outbox_pending_uploads(connection: &rusqlite::Connection) -> Result<HashSet<String>, DatabaseError> {
    let mut cloud_storage_enabled: Option<i64> = None;
    connection.sync_cloud_storage_enabled(|row| {
        cloud_storage_enabled = row.get(0)?;
        Ok(())
    })?;
    if cloud_storage_enabled == Some(0) {
        return Ok(HashSet::new());
    }
    let mut pending = HashSet::new();
    connection.sync_pending_uploads(|row| {
        pending.insert(row.get::<_, String>(0)?);
        Ok(())
    })?;
    Ok(pending)
}

impl Database {
    /// Freeze and select a publication snapshot under one writer reservation.
    pub(crate) fn with_publications<T>(&self, read: impl FnOnce(&rusqlite::Connection) -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        self.with_write_transaction(|tx| {
            ({
                use crate::sync::apply::registers::*;
                let conn: &Connection = tx;

                let pending = conn
                    .prepare("SELECT state_kind,state_key,state_subkey,replica_seq FROM sync_outbox WHERE body IS NULL ORDER BY replica_seq")?
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                for (kind, key, subkey, seq) in pending {
                    let Some((bytes, _)) = canonical_body(conn, &kind, &key, &subkey)? else {
                        continue;
                    };
                    let current = read_version(conn, &kind, &key, &subkey)?.ok_or_else(|| DatabaseError::message("queued register has no version"))?;
                    let version = sync_common::wire::encode(&current).map_err(DatabaseError::operation)?;
                    // Recovery forwards the exact retained version, never a newer edit.
                    conn.execute("UPDATE sync_outbox SET body=?1,changed_at=?2,winning_version=?3 WHERE replica_seq=?4", params![bytes, storage_i64(current.changed_at, "publication timestamp")?, version, seq])?;
                }
                ()
            });
            Ok(Commit(read(tx)?))
        })
    }

    /// Reads every outbox mutation whose book dependencies are
    /// satisfied. The returned snapshot is plain data: the caller performs
    /// network work with no database reference held, then commits
    /// acknowledgements with [`Database::sync_acknowledge_mutations`].
    pub fn sync_publishable_mutations(&self) -> Result<Vec<StatePublication>, DatabaseError> {
        self.with_publications(|connection| {
            let pending = outbox_pending_uploads(connection)?;
            let mut rows = Vec::new();
            connection.sync_outbox_rows(|row| {
                rows.push(read_outbox_row(row)?);
                Ok(())
            })?;
            let mut mutations = Vec::with_capacity(rows.len());
            for row in rows {
                let mutation = decode_outbox_row(row)?;
                if book_ready(&mutation, &pending) {
                    mutations.push(mutation);
                }
            }
            Ok(mutations)
        })
    }

    /// Fixes the end of an upload scan so continuous local edits cannot keep
    /// it running. The coordinator pages up to this sequence number.
    pub fn sync_outbox_watermark(&self) -> Result<i64, DatabaseError> {
        let mut watermark = 0i64;
        self.connection.sync_outbox_watermark(|row| {
            watermark = row.get(0)?;
            Ok(())
        })?;
        Ok(watermark)
    }

    /// Reads one bridge-sized page of publishable outbox mutations. Keyset
    /// paging stays valid when acknowledgments remove rows between pages.
    /// At least one row is returned even if its body exceeds the byte
    /// target, so oversized mutations cannot starve later rows.
    pub fn sync_pending_mutations_page(&self, after: i64, through: i64) -> Result<Vec<StatePublication>, DatabaseError> {
        self.with_publications(|connection| {
            const BYTE_TARGET: usize = 256 * 1024;
            let pending = outbox_pending_uploads(connection)?;
            let mut rows = Vec::new();
            connection.sync_outbox_rows_page(after, through, |row| {
                rows.push(read_outbox_row(row)?);
                Ok(())
            })?;
            let mut mutations = Vec::new();
            let mut bytes = 0usize;
            for stored in rows {
                let body_bytes = stored.body.len();
                let mutation = decode_outbox_row(stored)?;
                if !book_ready(&mutation, &pending) {
                    continue;
                }
                if !mutations.is_empty() && bytes + body_bytes > BYTE_TARGET {
                    break;
                }
                bytes += body_bytes;
                mutations.push(mutation);
                if bytes >= BYTE_TARGET || mutations.len() >= 256 {
                    break;
                }
            }
            Ok(mutations)
        })
    }

    /// Enqueues one reading-position change directly into the durable outbox.
    pub fn enqueue_outbox_reading_change(&self, state_key: &str, body: &[u8], changed_at: i64) -> Result<(), DatabaseError> {
        self.with_write_transaction(|tx| {
            let body: LibraryStateEntry = sync_common::wire::decode(body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
            if !matches!(&body,LibraryStateEntry::ReadingPosition { content_hash,.. } if content_hash.as_str()==state_key) {
                return Err(DatabaseError::message("reading identity mismatch"));
            }
            crate::sync::apply::registers::accept_value(tx, &body, u64::try_from(changed_at).map_err(DatabaseError::operation)?)?;
            Ok(Commit(()))
        })
    }

    /// Drops every durable outbox entry.
    pub fn clear_sync_outbox(&self) -> Result<(), DatabaseError> {
        self.connection.clear_sync_outbox()?;
        Ok(())
    }

    /// Seeds one outbox entry with an explicit replica sequence, e.g. a
    /// poison mutation the server will permanently reject.
    pub fn seed_poison_outbox_mutation(&self, replica_seq: i64, body: &[u8], changed_at: i64) -> Result<(), DatabaseError> {
        self.connection.seed_poison_outbox_mutation(replica_seq, body, changed_at)?;
        Ok(())
    }

    /// Acknowledges exactly these publisher sequences in one transaction. Callers
    /// batch large id sets into repeated calls; batching policy belongs to
    /// the orchestrator, not to storage.
    pub fn sync_acknowledge_mutations(&self, ids: &[ReplicaSeq]) -> Result<(), DatabaseError> {
        if ids.is_empty() {
            return Ok(());
        }
        let payload = serde_json::to_string(&ids.iter().map(|id| id.get()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            transaction.sync_acknowledge_mutations(&payload)?;
            Ok(())
        })
    }
}

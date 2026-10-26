//! Sync outbox snapshots and acknowledgements.

use crate::query_methods::query_rows;
pub(crate) use crate::sync::SyncOutboxSql;
use crate::transactions::WriteOutcome::Commit;
pub(crate) use crate::{Database, DatabaseError};
use content_address::BookId;
pub(crate) use library_replica::{LibraryStateEntry, StatePublication};
use rusqlite::{Connection, Row, named_params, types::ValueRef};
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

fn row_blob<'a>(row: &Row<'_>, index: usize, value: ValueRef<'a>) -> rusqlite::Result<&'a [u8]> {
    match value {
        ValueRef::Blob(bytes) => Ok(bytes),
        value => Err(rusqlite::Error::InvalidColumnType(index, row.as_ref().column_name(index)?.to_owned(), value.data_type())),
    }
}

pub(crate) fn read_outbox_row<'a>(row: &'a Row<'_>) -> rusqlite::Result<OutboxRow<'a>> {
    Ok(OutboxRow {
        state_kind: row.get(0)?,
        state_key: row.get(1)?,
        body: row_blob(row, 2, row.get_ref(2)?)?,
        changed_at: row.get(3)?,
        replica_seq: row.get(4)?,
        state_subkey: row.get(5)?,
        winning_version: match row.get_ref(6)? {
            ValueRef::Null => None,
            value => Some(row_blob(row, 6, value)?),
        },
    })
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
    let mut cloud_storage_enabled: Option<i64> = None;
    connection.sync_cloud_storage_enabled(|row| {
        cloud_storage_enabled = row.get(0)?;
        Ok(())
    })?;
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
            ({
                use crate::sync::apply::registers::*;
                let conn: &Connection = tx;

                let pending = query_rows(|callback| conn.outbox_pending_snapshots(callback), |r| Ok((r.get::<_, String>(0)?, r.get::<_, StateKey>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
                for (kind, key, subkey, seq) in pending {
                    let (bytes, current) = if records::is_record(&kind) {
                        let Some(snapshot) = records::snapshot(conn, &kind, &key, &subkey)? else {
                            continue;
                        };
                        snapshot
                    } else {
                        let Some((bytes, _)) = canonical_body(conn, &kind, &key, &subkey)? else {
                            continue;
                        };
                        let current = publication_version(conn, &kind, &key, &subkey)?.ok_or_else(|| DatabaseError::message("queued register has no version"))?;
                        (bytes, current)
                    };
                    let version = sync_common::wire::encode(&current).map_err(DatabaseError::operation)?;
                    // Recovery forwards the exact retained version, never a newer edit.
                    conn.outbox_freeze_snapshot(&bytes, &(storage_i64(current.changed_at, "publication timestamp")?), &version, &seq)?;
                }
            });
            Ok(Commit(read(tx)?))
        })
    }

    /// Reads outbox mutations whose book dependencies are satisfied. The returned snapshot is plain data: the caller performs
    /// network work with no database reference held, then commits
    /// acknowledgements with [`Database::sync_acknowledge_mutations`].
    pub fn sync_publishable_mutations(&self) -> Result<Vec<StatePublication>, DatabaseError> {
        let mutations = self.with_publications(|connection| {
            let mut through = 0i64;
            connection.sync_outbox_watermark(|row| {
                through = row.get(0)?;
                Ok(())
            })?;
            let pending = outbox_pending_uploads(connection)?;
            let mut after = i64::MIN;
            let mut mutations = Vec::new();
            loop {
                let page = self.read_publication_page(connection, &pending, after, through, None)?.0;
                let Some(last) = page.last() else {
                    return Ok(mutations);
                };
                after = last.publisher_seq.get() as i64;
                mutations.extend(page);
            }
        })?;
        Ok(mutations)
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
        let mutations = self.with_publications(|connection| {
            let pending = outbox_pending_uploads(connection)?;
            self.read_publication_page(connection, &pending, after, through, None).map(|page| page.0)
        })?;
        Ok(mutations)
    }

    /// Freeze once, then release the writer before any network request.
    pub fn sync_prepare_outbox(&self) -> Result<OutboxScan, DatabaseError> {
        self.sync_prepare_outbox_excluding_positions(HashSet::new())
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
            let (page, after, done) = self.read_publication_page(&self.connection, &scan.pending, scan.after, scan.through, Some(scan.lifecycles))?;
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

    // Both publication paths use this policy. Stream each SQL page, stop at
    // the publication budget, and continue scanning past deferred rows.
    fn read_publication_page(&self, connection: &Connection, pending: &PublicationDependencies, after: i64, through: i64, lifecycles: Option<bool>) -> Result<(Vec<StatePublication>, i64, bool), DatabaseError> {
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
                    LibraryStateEntry::ReadingPosition { content_hash, .. } => !pending.reading_positions.contains(content_hash),
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

    /// Enqueues one reading-position change directly into the durable outbox.
    pub fn enqueue_outbox_reading_change(&self, state_key: &BookId, body: &[u8], changed_at: i64) -> Result<(), DatabaseError> {
        self.with_write_transaction(|tx| {
            let body: LibraryStateEntry = sync_common::wire::decode(body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
            if !matches!(&body,LibraryStateEntry::ReadingPosition { content_hash,.. } if content_hash==state_key) {
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

#[cfg(test)]
mod performance_tests {
    use super::*;
    use library_replica::{BookAddition, BookFacts, EnrichmentResult};
    use std::time::Instant;
    use sync_common::{ConflictRank, ReplicaId, VersionKey, wire};

    #[test]
    fn prepared_pages_do_not_reserve_a_writer_or_freeze_later_edits() {
        let path = std::env::temp_dir().join(format!("prepared-outbox-{}.db", uuid::Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        db.create_directory_with_id(&uuid::Uuid::from_u128(1), &sync_common::ROOT_DIR_ID, &"First".into()).unwrap();
        let mut scan = db.sync_prepare_outbox().unwrap();
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

    #[test]
    fn borrowed_outbox_payloads_keep_sql_type_errors_and_missing_version_checks() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.set_asset_storage_policy(false, false).unwrap();
        let hash = BookId::from_bytes([1; 32]);
        let body = LibraryStateEntry::BookLifecycle { content_hash: hash, value: library_replica::BookLifecycleState::Present };
        let version = VersionKey::new(1, body.conflict_rank(), ReplicaId::from_u128(1), ReplicaSeq::new(1).unwrap());
        let body = wire::encode(&body).unwrap();
        let version = wire::encode(&version).unwrap();
        db.connection.execute("INSERT INTO sync_outbox(replica_seq,state_kind,state_key,body,changed_at,winning_version) VALUES(1,'book_lifecycle',?1,?2,1,?3)", rusqlite::params![StateKey::Book(hash), body, version]).unwrap();
        for (column, index) in [("body", 2), ("winning_version", 6)] {
            db.connection.execute(&format!("UPDATE sync_outbox SET {column}=5"), []).unwrap();
            let expected = db.connection.query_row("SELECT state_kind,state_key,body,changed_at,replica_seq,state_subkey,winning_version FROM sync_outbox", [], |row| row.get::<_, Vec<u8>>(index)).unwrap_err();
            let actual = db.sync_pending_mutations_page(i64::MIN, 1).unwrap_err();
            assert_eq!(actual.to_string(), expected.to_string());
            db.connection.execute("UPDATE sync_outbox SET body=?1,winning_version=?2", rusqlite::params![body, version]).unwrap();
        }
        db.connection.execute("UPDATE sync_outbox SET winning_version=NULL", []).unwrap();
        assert!(db.sync_pending_mutations_page(i64::MIN, 1).unwrap_err().to_string().contains("missing winning version"));
    }

    #[test]
    #[ignore = "release benchmark of frozen outbox reads; set SYNC_OUTBOX_BENCH_CASE=small or large"]
    fn frozen_outbox_read_performance() {
        let case = std::env::var("SYNC_OUTBOX_BENCH_CASE").unwrap();
        let (count, payload) = match case.as_str() {
            "small" => (1_000, 128),
            "large" => (1_000, 64 * 1024),
            _ => panic!("unknown outbox benchmark case"),
        };
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.set_asset_storage_policy(false, false).unwrap();
        let value = LibraryStateEntry::BookAdd {
            content_hash: BookId::from_bytes([1; 32]),
            value: BookAddition {
                facts: BookFacts { added_at: 1, format: book_model::BookFormat::Epub },
                record: book_model::BookRecord { metadata: Default::default(), description: "x".repeat(payload) },
                navigation: None,
                pdf: None,
                recording: None,
                enrichment: Some(EnrichmentResult::NoMatch),
            },
        };
        let tx = db.connection.unchecked_transaction().unwrap();
        {
            let mut insert = tx.prepare("INSERT INTO sync_outbox(replica_seq,state_kind,state_key,state_subkey,body,changed_at,winning_version) VALUES(?1,'book_add',?2,'',?3,1,?4)").unwrap();
            for index in 1..=count {
                let mut hash = [0; 32];
                hash[..8].copy_from_slice(&(index as u64).to_le_bytes());
                let hash = BookId::from_bytes(hash);
                let mut body = value.clone();
                if let LibraryStateEntry::BookAdd { content_hash, .. } = &mut body {
                    *content_hash = hash;
                }
                let version = VersionKey::new(1, ConflictRank::Update, ReplicaId::from_u128(1), ReplicaSeq::new(index as u64).unwrap());
                insert.execute(rusqlite::params![index, StateKey::Book(hash), wire::encode(&body).unwrap(), wire::encode(&version).unwrap()]).unwrap();
            }
        }
        tx.commit().unwrap();
        let mut samples = Vec::new();
        for _ in 0..8 {
            let started = Instant::now();
            let publications = db.sync_publishable_mutations().unwrap();
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(publications.len(), count as usize);
            assert!(publications.windows(2).all(|rows| rows[0].publisher_seq < rows[1].publisher_seq));
        }
        samples.remove(0);
        samples.sort_by(f64::total_cmp);
        println!("OUTBOX case={case} cells={count} payload_bytes={payload} median_read_ms={:.3}", samples[3]);
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(line) = status.lines().find(|line| line.starts_with("VmHWM:")) {
                println!("OUTBOX {line}");
            }
        }
    }
}

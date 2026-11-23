//! Mutable register merging and shared publication helpers.
pub(crate) use super::*;
use crate::SqlParam;
use crate::query_methods::query_rows;
use content_address::BookId;
pub(crate) use rusqlite::{Connection, OptionalExtension};
use sync_common::StateKey;

pub(super) struct MergeState {
    version: Option<VersionKey>,
    previous_book: Option<BookId>,
    has_body: bool,
    pub(super) can_merge: bool,
    pub(super) queued: bool,
}

/// No writer can change these checks between this read and the admission.
/// The existing transaction owns the merge and its publication invalidation.
pub(super) fn read_merge_state(conn: &Connection, kind: &str, key: &StateKey, subkey: &str, book: Option<&BookId>) -> Result<MergeState, DatabaseError> {
    let mut statement = conn.prepare_cached(include_str!("../sql/merge_state.sql"))?;
    Ok(statement.query_row(rusqlite::params![kind, key, subkey, book], |row| {
        let can_merge: bool = row.get(7)?;
        if !can_merge {
            return Ok(MergeState { version: None, previous_book: None, has_body: false, can_merge, queued: false });
        }
        let version = match row.get::<_, Option<i64>>(0)? {
            Some(time) => StoredVersion::decode(time, row.get(1)?, text_column(row, 2)?, row.get(3)?, [0, 1, 2, 3])?.map(|stored| stored.0),
            None => None,
        };
        Ok(MergeState { version, previous_book: row.get(4)?, has_body: row.get(5)?, queued: row.get(6)?, can_merge })
    })?)
}

pub(crate) fn book_key(body: &LibraryStateEntry) -> Option<&BookId> {
    match body {
        LibraryStateEntry::DirectoryName { .. } | LibraryStateEntry::DirectoryParent { .. } | LibraryStateEntry::DirectoryLifecycle { .. } => None,
        LibraryStateEntry::Annotation { value, .. } => Some(&value.book_id),
        LibraryStateEntry::BookAdd { book_id, .. }
        | LibraryStateEntry::BookContent { book_id, .. }
        | LibraryStateEntry::BookLifecycle { book_id, .. }
        | LibraryStateEntry::Placement { book_id, .. }
        | LibraryStateEntry::ReadingPosition { book_id, .. }
        | LibraryStateEntry::ChapterReadStatus { book_id, .. } => Some(book_id),
    }
}

/// Caller owns the transaction. Book fields exist only while their book exists.
pub(crate) fn merge(conn: &Connection, state: &sync_common::Versioned<LibraryStateEntry>) -> Result<bool, DatabaseError> {
    merge_inner(conn, state, true)
}

pub(crate) fn merge_inner<T: std::borrow::Borrow<LibraryStateEntry>>(conn: &Connection, state: &sync_common::Versioned<T>, invalidate_publication: bool) -> Result<bool, DatabaseError> {
    let body = state.value.borrow();
    let version = &state.version;
    body.validate().map_err(DatabaseError::operation)?;
    match body {
        LibraryStateEntry::Annotation { value, .. } => {
            storage_i64(value.modified_at, "annotation modification timestamp")?;
        }
        _ => {}
    }
    if let LibraryStateEntry::BookContent { book_id, checksum, size_bytes, .. } = body {
        conn.execute("INSERT OR IGNORE INTO book(content_hash) VALUES(?1)", [book_id])?;
        crate::content_identity::freeze_content(conn, *book_id, *checksum, *size_bytes)?;
    }
    let (kind, key, subkey) = sync_state_identity(body);
    let current = read_merge_state(conn, kind, &key, &subkey, book_key(body))?;
    if !current.can_merge {
        return Ok(false);
    }
    if version.conflict_rank != body.conflict_rank() {
        return Err(DatabaseError::message("mutation conflict rank does not match its value"));
    }
    if let Some(stored) = &current.version
        && (version < stored || (version == stored && current.has_body))
    {
        return Ok(false);
    }
    if current.previous_book.as_ref() != book_key(body)
        && let Some(hash) = current.previous_book
    {
        conn.registers_mark_book_dirty(&hash)?;
    }
    if let LibraryStateEntry::BookAdd { book_id, value } = body {
        if current.version.is_none() {
            conn.execute(
                "INSERT INTO book_transfer(content_hash,enrichment_retry_at) VALUES(?1,?2)
                ON CONFLICT(content_hash) DO UPDATE SET enrichment_retry_at=excluded.enrichment_retry_at",
                rusqlite::params![book_id, value.enrichment.is_none().then_some(0i64)],
            )?;
        }
        conn.execute("UPDATE book_transfer SET cover_checked=0 WHERE content_hash=?1", [book_id])?;
    }
    let encoded = sync_common::wire::encode(body).map_err(DatabaseError::operation)?;
    conn.registers_store(
        &kind,
        &key,
        &subkey,
        &(storage_i64(version.changed_at, "register timestamp")?),
        &(u8::from(version.conflict_rank)),
        &(version.replica_id.to_string()),
        &(storage_i64(version.replica_seq.get(), "register sequence")?),
        &encoded,
        &(book_key(body)),
    )?;
    conn.registers_mark_projection_dirty(&kind, &key, &subkey)?;
    if invalidate_publication && current.queued {
        schedule(conn, kind, &key, &subkey, None)?;
    }
    Ok(true)
}

/// Record a local edit. Unchanged canonical values are not republished.
pub(crate) fn record_local(conn: &Connection, body: LibraryStateEntry) -> Result<(), DatabaseError> {
    body.validate().map_err(DatabaseError::operation)?;
    let (kind, key, subkey) = sync_state_identity(&body);
    let current = canonical_body(conn, kind, &key, &subkey)?;
    if let Some((bytes, _)) = &current {
        let current: LibraryStateEntry = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
        if current == body {
            return Ok(());
        }
    }
    // File facts start below optional enrichment, including on offline imports.
    let time = if current.is_none() && matches!(body, LibraryStateEntry::BookAdd { .. }) { 0 } else { crate::sync::unix_millis()? as u64 };
    accept_value(conn, &body, time)
}

fn replica_id(conn: &Connection) -> Result<uuid::Uuid, DatabaseError> {
    Ok(conn.registers_replica_id(|row| parse_uuid_column(text_column(row, 0)?, 0))?)
}

pub(crate) fn next_sequence(conn: &Connection) -> Result<ReplicaSeq, DatabaseError> {
    let sequence: i64 = conn.registers_allocate_sequence(|r| r.get(0))?;
    ReplicaSeq::new(nonnegative_u64(sequence, 0, "replica sequence")?).map_err(DatabaseError::operation)
}

pub(crate) fn accept_value(conn: &Connection, body: &LibraryStateEntry, time: u64) -> Result<(), DatabaseError> {
    let (kind, key, subkey) = sync_state_identity(body);
    let current = read_version(conn, kind, &key, &subkey)?;
    let time = match current {
        Some(current) => time.max(current.changed_at.checked_add(1).ok_or_else(|| DatabaseError::message("register clock exhausted"))?),
        None => time,
    };
    let version = VersionKey::new(time, body.conflict_rank(), replica_id(conn)?, next_sequence(conn)?);
    if merge_inner(conn, &sync_common::Versioned { value: body, version: version.clone() }, false)? {
        schedule(conn, kind, &key, &subkey, Some(&version))?;
        if matches!(body, LibraryStateEntry::BookLifecycle { value: library_replica::BookLifecycleState::Present, .. }) {
            // Our own winning existence declaration may be the first state
            // published after another device's purge/readd. Its server echo
            // has an equal version, so queue surviving fields here as well.
            for cell in book_fields(conn, &key.book().ok_or_else(|| DatabaseError::message("book lifecycle requires a book key"))?)? {
                recover(conn, &cell)?;
            }
        }
    }
    Ok(())
}

/// A queue generation has no payload until preparation. Superseding it assigns
/// a new identity, so an acknowledgement of an in-flight older snapshot is safe.
pub(crate) fn schedule(conn: &Connection, kind: &str, key: &StateKey, subkey: &str, local: Option<&VersionKey>) -> Result<(), DatabaseError> {
    let seq = match local {
        Some(version) => version.replica_seq,
        None => next_sequence(conn)?,
    };
    conn.registers_schedule_publication(&(storage_i64(seq.get(), "queue sequence")?), &kind, &key, &subkey)?;
    Ok(())
}

pub(crate) fn book_fields(conn: &Connection, hash: &BookId) -> Result<Vec<sync_common::StateAddress>, DatabaseError> {
    Ok(query_rows(|callback| conn.registers_book_fields(&hash, callback), |r| Ok(sync_common::StateAddress { kind: r.get(0)?, entity_key: r.get(1)?, entity_subkey: r.get(2)? }))?)
}

/// Recovery queues canonical identities only. Merge has already invalidated any
/// older queued snapshot. Unknown payloads stay unknown and cannot be published.
pub(crate) fn recover(conn: &Connection, cell: &sync_common::StateAddress) -> Result<bool, DatabaseError> {
    if canonical_body(conn, &cell.kind, &cell.entity_key, &cell.entity_subkey)?.is_none() {
        return Ok(false);
    }
    let queued: bool = conn.registers_queued(&cell.kind, &cell.entity_key, &cell.entity_subkey, |r| r.get(0))?;
    if queued {
        return Ok(false);
    }
    schedule(conn, &cell.kind, &cell.entity_key, &cell.entity_subkey, None)?;
    Ok(true)
}

/// Canonical bytes and their publication version, read together.
pub(crate) fn publication_snapshot(conn: &Connection, kind: &str, key: &StateKey, subkey: &str) -> Result<Option<(Vec<u8>, VersionKey)>, DatabaseError> {
    let stored = conn
        .registers_canonical_body(&kind, &key, &subkey, |row| {
            let bytes: Vec<u8> = row.get(0)?;
            let rank: i64 = row.get(1)?;
            Ok((bytes, StoredVersion::decode(row.get(2)?, rank, text_column(row, 3)?, row.get(4)?, [2, 1, 3, 4])?))
        })
        .optional()?;
    let Some((bytes, version)) = stored else { return Ok(None) };
    let version = version.ok_or_else(|| DatabaseError::message("queued register has no version"))?;
    Ok(Some((bytes, version.0)))
}

pub(crate) fn canonical_body(conn: &Connection, kind: &str, key: &StateKey, subkey: &str) -> Result<Option<(Vec<u8>, i64)>, DatabaseError> {
    Ok(conn.registers_canonical_body(&kind, &key, &subkey, |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
}

// Validation must not depend on whether the book happens to be purged today.
// Otherwise an accepted value could poison a later reimport's projection.

include_sqlite_sql::include_sql!("src/sync/sql/registers_operations.sql");

#[cfg(test)]
mod merge_tests {
    use super::*;
    use library_replica::{BookAddition, BookFacts, EnrichmentResult};
    use sync_common::{ConflictRank, ROOT_DIR_ID, Versioned};

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../sql/schema/tables.sql")).unwrap();
        conn.execute_batch(include_str!("../../assets/sql/schema/tables.sql")).unwrap();
        conn.execute("INSERT INTO sync_metadata(singleton,replica_id) VALUES(1,?1)", [uuid::Uuid::from_u128(1).to_string()]).unwrap();
        conn.execute("INSERT INTO sync_clock(singleton,sequence) VALUES(1,100)", []).unwrap();
        conn
    }

    fn state(value: LibraryStateEntry, time: u64) -> Versioned<LibraryStateEntry> {
        Versioned { version: VersionKey::new(time, value.conflict_rank(), uuid::Uuid::from_u128(2), ReplicaSeq::new(time + 1).unwrap()), value }
    }

    #[test]
    fn merged_read_preserves_stale_echo_and_missing_body_rules() {
        let conn = connection();
        let tx = conn.unchecked_transaction().unwrap();
        let body = LibraryStateEntry::DirectoryName { dir_id: ROOT_DIR_ID, name: "Winner".into() };
        let winner = state(body.clone(), 20);
        assert!(merge(&tx, &winner).unwrap());
        assert!(!merge(&tx, &state(body, 10)).unwrap());
        assert!(!merge(&tx, &winner).unwrap());
        tx.execute("UPDATE sync_state_version SET body=NULL", []).unwrap();
        assert!(merge(&tx, &winner).unwrap());
        let (kind, key, subkey) = sync_state_identity(&winner.value);
        let stored = read_merge_state(&tx, kind, &key, &subkey, None).unwrap();
        assert_eq!(stored.version, Some(winner.version));
        assert!(stored.has_body);
        assert!(!stored.queued);
    }

    #[test]
    fn merged_read_rebinds_keys_and_retains_book_and_rank_checks() {
        let conn = connection();
        let tx = conn.unchecked_transaction().unwrap();
        let hash = BookId::new(&"a".repeat(64));
        let other = BookId::new(&"b".repeat(64));
        let placement = LibraryStateEntry::Placement { dir_id: ROOT_DIR_ID, book_id: hash, present: true, origin_folder_id: None };
        let mut incoming = state(placement, 10);
        incoming.version.conflict_rank = ConflictRank::Delete;
        // Missing books are ignored before rank admission, as before.
        assert!(!merge(&tx, &incoming).unwrap());
        tx.execute("INSERT INTO book(content_hash) VALUES(?1)", [&hash]).unwrap();
        assert!(merge(&tx, &incoming).is_err());
        incoming.version.conflict_rank = ConflictRank::Update;
        assert!(merge(&tx, &incoming).unwrap());
        let key = StateKey::Book(hash);
        assert!(read_merge_state(&tx, "placement", &key, &ROOT_DIR_ID.to_string(), Some(&hash)).unwrap().can_merge);
        assert!(!read_merge_state(&tx, "placement", &StateKey::Book(other), &ROOT_DIR_ID.to_string(), Some(&other)).unwrap().can_merge);
        assert!(read_merge_state(&tx, "book_lifecycle", &StateKey::Book(other), "", Some(&other)).unwrap().can_merge);
    }

    #[test]
    fn queued_unknown_values_get_a_new_publication_generation() {
        let conn = connection();
        let tx = conn.unchecked_transaction().unwrap();
        let incoming = state(LibraryStateEntry::DirectoryName { dir_id: ROOT_DIR_ID, name: "Winner".into() }, 10);
        let (kind, key, subkey) = sync_state_identity(&incoming.value);
        tx.execute("INSERT INTO sync_outbox(replica_seq,state_kind,state_key,state_subkey,body,changed_at,winning_version) VALUES(50,?1,?2,?3,x'01',1,x'02')", rusqlite::params![kind, key, subkey]).unwrap();
        assert!(merge(&tx, &incoming).unwrap());
        let generation = tx
            .query_row("SELECT replica_seq,body,changed_at,winning_version FROM sync_outbox", [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?, row.get::<_, Option<i64>>(2)?, row.get::<_, Option<Vec<u8>>>(3)?)))
            .unwrap();
        assert_eq!(generation, (101, None, None, None));
        assert!(!merge(&tx, &incoming).unwrap());
        assert_eq!(tx.query_row("SELECT replica_seq FROM sync_outbox", [], |row| row.get::<_, i64>(0)).unwrap(), 101);
    }

    #[test]
    fn record_confirmation_uses_version_order() {
        let conn = connection();
        let tx = conn.unchecked_transaction().unwrap();
        let hash = BookId::new(&"c".repeat(64));
        let addition = BookAddition {
            facts: BookFacts { added_at: 1, format: book_model::BookFormat::Epub },
            record: book_model::BookRecord { metadata: Default::default(), description: "Server value".into() },
            navigation: None,
            pdf: None,
            recording: None,
            enrichment: Some(EnrichmentResult::NoMatch),
        };
        let incoming = state(LibraryStateEntry::BookAdd { book_id: hash, value: addition }, 20);
        assert!(!merge(&tx, &incoming).unwrap());
        tx.execute("INSERT INTO book(content_hash) VALUES(?1)", [&hash]).unwrap();
        tx.execute("INSERT INTO book_transfer(content_hash,cover_checked) VALUES(?1,1)", [&hash]).unwrap();
        assert!(merge(&tx, &incoming).unwrap());
        assert!(!merge(&tx, &incoming).unwrap());
        // Older publications cannot replace the current record.
        let older_token = state(incoming.value.clone(), 10);
        assert!(!merge(&tx, &older_token).unwrap());
        assert_eq!(read_version(&tx, "book_add", &StateKey::Book(hash), "").unwrap(), Some(incoming.version));
        assert_eq!(tx.query_row("SELECT cover_checked FROM book_transfer", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
}

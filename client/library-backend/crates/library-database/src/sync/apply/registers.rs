//! Mutable register merging and shared publication helpers.
pub(crate) use super::*;
use crate::SqlParam;
use crate::query_methods::query_rows;
use content_address::ContentHash;
pub(crate) use rusqlite::{Connection, OptionalExtension};
use sync_common::StateKey;

pub(crate) fn book_key(body: &LibraryStateEntry) -> Option<&ContentHash> {
    match body {
        LibraryStateEntry::DirectoryName { .. } | LibraryStateEntry::DirectoryParent { .. } | LibraryStateEntry::DirectoryLifecycle { .. } => None,
        LibraryStateEntry::Annotation { value, .. } => Some(&value.content_hash),
        LibraryStateEntry::BookAdd { content_hash, .. } | LibraryStateEntry::BookLifecycle { content_hash, .. } | LibraryStateEntry::Placement { content_hash, .. } | LibraryStateEntry::ReadingPosition { content_hash, .. } => {
            Some(content_hash)
        }
    }
}

/// Caller owns the transaction. Book fields exist only while their book exists.
pub(crate) fn merge(conn: &Connection, state: &sync_common::Versioned<LibraryStateEntry>) -> Result<bool, DatabaseError> {
    if records::is_record(state.value.kind()) {
        return records::confirm(conn, state);
    }
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
    if !matches!(body, LibraryStateEntry::BookLifecycle { .. })
        && let Some(hash) = book_key(body)
        && !conn.registers_book_exists(&hash, |r| r.get::<_, bool>(0))?
    {
        return Ok(false);
    }
    let (kind, key, subkey) = sync_state_identity(body);
    let current = read_version(conn, kind, &key, &subkey)?;
    if version.conflict_rank != body.conflict_rank() {
        return Err(DatabaseError::message("mutation conflict rank does not match its value"));
    }
    if let Some(stored) = &current
        && (version < stored || (version == stored && canonical_body(conn, kind, &key, &subkey)?.is_some()))
    {
        return Ok(false);
    }
    let previous_book: Option<ContentHash> = conn.registers_previous_owner(&kind, &key, &subkey, |r| r.get(0)).optional()?.flatten();
    if previous_book.as_ref() != book_key(body)
        && let Some(hash) = previous_book
    {
        conn.registers_mark_book_dirty(&hash)?;
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
    if invalidate_publication && conn.registers_queued(&kind, &key, &subkey, |r| r.get::<_, bool>(0))? {
        schedule(conn, kind, &key, &subkey, None)?;
    }
    Ok(true)
}

/// Record a local edit. Unchanged canonical values are not republished.
pub(crate) fn record_local(conn: &Connection, body: LibraryStateEntry) -> Result<(), DatabaseError> {
    body.validate().map_err(DatabaseError::operation)?;
    if records::is_record(body.kind()) {
        return records::insert(conn, &body);
    }
    let (kind, key, subkey) = sync_state_identity(&body);
    if let Some((bytes, _)) = canonical_body(conn, kind, &key, &subkey)? {
        let current: LibraryStateEntry = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
        if current == body {
            return Ok(());
        }
    }
    accept_value(conn, &body, crate::sync::unix_millis()? as u64)
}

fn replica_id(conn: &Connection) -> Result<uuid::Uuid, DatabaseError> {
    let raw: String = conn.registers_replica_id(|r| r.get(0))?;
    Ok(parse_uuid_column(raw, 0)?)
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
        if matches!(body, LibraryStateEntry::BookLifecycle { value: library_replica::BookLifecycleState::Present | library_replica::BookLifecycleState::Deleted { .. }, .. }) {
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

pub(crate) fn book_fields(conn: &Connection, hash: &ContentHash) -> Result<Vec<sync_common::StateAddress>, DatabaseError> {
    Ok(query_rows(|callback| conn.registers_book_fields(&hash, callback), |r| Ok(sync_common::StateAddress { kind: r.get(0)?, entity_key: r.get(1)?, entity_subkey: r.get(2)? }))?.collect::<Result<Vec<_>, _>>()?)
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

pub(crate) fn canonical_body(conn: &Connection, kind: &str, key: &StateKey, subkey: &str) -> Result<Option<(Vec<u8>, i64)>, DatabaseError> {
    if records::is_record(kind) {
        return Ok(records::snapshot(conn, kind, key, subkey)?.map(|(body, _)| (body, 0)));
    }
    Ok(conn.registers_canonical_body(&kind, &key, &subkey, |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
}

// Validation must not depend on whether the book happens to be purged today.
// Otherwise an accepted value could poison a later reimport's projection.

include_sqlite_sql::include_sql!("src/sync/sql/registers_operations.sql");

//! One acceptance rule for local and remote registers; projection never feeds it.
pub(crate) use super::*;
pub(crate) use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn book_key(body: &LibraryStateEntry) -> Option<&str> {
    match body {
        LibraryStateEntry::DirectoryName { .. } | LibraryStateEntry::DirectoryParent { .. } | LibraryStateEntry::DirectoryLifecycle { .. } => None,
        LibraryStateEntry::Annotation { value, .. } => Some(value.content_hash.as_str()),
        LibraryStateEntry::BookFacts { content_hash, .. }
        | LibraryStateEntry::BookLifecycle { content_hash, .. }
        | LibraryStateEntry::Placement { content_hash, .. }
        | LibraryStateEntry::ReadingPosition { content_hash, .. }
        | LibraryStateEntry::Metadata { content_hash, .. }
        | LibraryStateEntry::Description { content_hash, .. }
        | LibraryStateEntry::PdfReaderMetadata { content_hash, .. }
        | LibraryStateEntry::BookToc { content_hash, .. } => Some(content_hash.as_str()),
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
        LibraryStateEntry::BookFacts { value, .. } => {
            storage_i64(value.added_at, "book addition timestamp")?;
        }
        LibraryStateEntry::Annotation { value, .. } => {
            storage_i64(value.modified_at, "annotation modification timestamp")?;
        }
        LibraryStateEntry::BookToc { value, .. } => {
            if let Some(duration) = value.duration_ms {
                storage_i64(duration, "audiobook duration")?;
            }
        }
        _ => {}
    }
    if !matches!(body, LibraryStateEntry::BookLifecycle { .. }) {
        if let Some(hash) = book_key(body) {
            if !conn.query_row("SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=?1)", [hash], |r| r.get::<_, bool>(0))? {
                return Ok(false);
            }
        }
    }
    let (kind, key, subkey) = sync_state_identity(body);
    let current = read_version(conn, kind, &key, &subkey)?;
    if version.conflict_rank != body.conflict_rank() {
        return Err(DatabaseError::message("mutation conflict rank does not match its value"));
    }
    if let Some(stored) = &current {
        if version < stored || (version == stored && canonical_body(conn, kind, &key, &subkey)?.is_some()) {
            return Ok(false);
        }
    }
    let previous_book: Option<String> = conn.query_row("SELECT book_key FROM sync_state_version WHERE state_kind=?1 AND state_key=?2 AND state_subkey=?3", params![kind, key, subkey], |r| r.get(0)).optional()?.flatten();
    if previous_book.as_deref() != book_key(body) {
        if let Some(hash) = previous_book {
            conn.execute("INSERT OR IGNORE INTO sync_projection_dirty VALUES('book_projection',?1,'')", [hash])?;
        }
    }
    let encoded = sync_common::wire::encode(body).map_err(DatabaseError::operation)?;
    conn.execute(
        "INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank,
         replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,body=excluded.body,book_key=excluded.book_key",
        params![
            kind,
            key,
            subkey,
            storage_i64(version.changed_at, "register timestamp")?,
            u8::from(version.conflict_rank),
            version.replica_id.to_string(),
            storage_i64(version.replica_seq.get(), "register sequence")?,
            encoded,
            book_key(body)
        ],
    )?;
    conn.execute("INSERT OR IGNORE INTO sync_projection_dirty VALUES(?1,?2,?3)", params![kind, key, subkey])?;
    if invalidate_publication && conn.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE state_kind=?1 AND state_key=?2 AND state_subkey=?3)", params![kind, key, subkey], |r| r.get::<_, bool>(0))? {
        schedule(conn, kind, &key, &subkey, None)?;
    }
    Ok(true)
}

/// Record a local edit. Unchanged canonical values are not republished.
pub(crate) fn record_local(conn: &Connection, body: LibraryStateEntry) -> Result<(), DatabaseError> {
    body.validate().map_err(DatabaseError::operation)?;
    let (kind, key, subkey) = sync_state_identity(&body);
    if let Some((bytes, _)) = canonical_body(conn, kind, &key, &subkey)? {
        let current: LibraryStateEntry = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
        if current == body {
            return Ok(());
        }
    }
    accept_value(conn, &body, crate::sync::unix_millis()? as u64)
}

pub(crate) fn next_sequence(conn: &Connection) -> Result<ReplicaSeq, DatabaseError> {
    let sequence: i64 = conn.query_row("UPDATE sync_clock SET sequence=sequence+1 WHERE singleton=1 AND sequence<9223372036854775807 RETURNING sequence", [], |r| r.get(0))?;
    ReplicaSeq::new(nonnegative_u64(sequence, 0, "replica sequence")?).map_err(DatabaseError::operation)
}

pub(crate) fn accept_value(conn: &Connection, body: &LibraryStateEntry, time: u64) -> Result<(), DatabaseError> {
    let (kind, key, subkey) = sync_state_identity(body);
    let current = read_version(conn, kind, &key, &subkey)?;
    let time = match current {
        Some(current) => time.max(current.changed_at.checked_add(1).ok_or_else(|| DatabaseError::message("register clock exhausted"))?),
        None => time,
    };
    let version = VersionKey::new(
        time,
        body.conflict_rank(),
        {
            let (conn,): (&Connection,) = (conn,);
            (|| -> Result<uuid::Uuid, DatabaseError> {
                let raw: String = conn.query_row("SELECT replica_id FROM sync_metadata WHERE singleton=1", [], |r| r.get(0))?;
                Ok(parse_uuid_column(raw, 0)?)
            })()
        }?,
        next_sequence(conn)?
    );
    if merge_inner(conn, &sync_common::Versioned { value: body, version: version.clone() }, false)? {
        schedule(conn, kind, &key, &subkey, Some(&version))?;
        if matches!(body, LibraryStateEntry::BookLifecycle { value: library_replica::BookLifecycleState::Present | library_replica::BookLifecycleState::Deleted { .. }, .. }) {
            // Our own winning existence declaration may be the first state
            // published after another device's purge/readd. Its server echo
            // has an equal version, so queue surviving fields here as well.
            for cell in book_fields(conn, &key)? {
                recover(conn, &cell)?;
            }
        }
    }
    Ok(())
}

/// A queue generation has no payload until preparation. Superseding it assigns
/// a new identity, so an acknowledgement of an in-flight older snapshot is safe.
pub(crate) fn schedule(conn: &Connection, kind: &str, key: &str, subkey: &str, local: Option<&VersionKey>) -> Result<(), DatabaseError> {
    let seq = match local {
        Some(version) => version.replica_seq,
        None => next_sequence(conn)?,
    };
    conn.execute(
        "INSERT INTO sync_outbox(replica_seq,state_kind,state_key,state_subkey) VALUES(?1,?2,?3,?4)
      ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,body=NULL,changed_at=NULL,winning_version=NULL",
        params![storage_i64(seq.get(), "queue sequence")?, kind, key, subkey],
    )?;
    Ok(())
}

pub(crate) fn book_fields(conn: &Connection, hash: &str) -> Result<Vec<sync_common::StateAddress>, DatabaseError> {
    Ok(conn
        .prepare("SELECT state_kind,state_key,state_subkey FROM sync_state_version WHERE book_key=?1 AND state_kind!='book_lifecycle' AND body IS NOT NULL")?
        .query_map([hash], |r| Ok(sync_common::StateAddress { kind: r.get(0)?, entity_key: r.get(1)?, entity_subkey: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Recovery queues canonical identities only. Merge has already invalidated any
/// older queued snapshot. Unknown payloads stay unknown and cannot be published.
pub(crate) fn recover(conn: &Connection, cell: &sync_common::StateAddress) -> Result<bool, DatabaseError> {
    if canonical_body(conn, &cell.kind, &cell.entity_key, &cell.entity_subkey)?.is_none() {
        return Ok(false);
    }
    let queued: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE state_kind=?1 AND state_key=?2 AND state_subkey=?3)", params![cell.kind, cell.entity_key, cell.entity_subkey], |r| r.get(0))?;
    if queued {
        return Ok(false);
    }
    schedule(conn, &cell.kind, &cell.entity_key, &cell.entity_subkey, None)?;
    Ok(true)
}

pub(crate) fn canonical_body(conn: &Connection, kind: &str, key: &str, subkey: &str) -> Result<Option<(Vec<u8>, i64)>, DatabaseError> {
    Ok(conn.query_row("SELECT body,conflict_rank FROM sync_state_version WHERE state_kind=?1 AND state_key=?2 AND state_subkey=?3 AND body IS NOT NULL", params![kind, key, subkey], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
}

// Validation must not depend on whether the book happens to be purged today.
// Otherwise an accepted value could poison a later reimport's projection.

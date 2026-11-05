//! Completed immutable book additions. The server accepts the
//! first publication; its deliveries are authoritative, regardless of clocks.
use super::registers::{allocate_sequences, next_sequence, read_merge_state, schedule};
use crate::DatabaseError;
use crate::SqlParam;
use library_replica::LibraryStateEntry;
use rusqlite::{Connection, OptionalExtension, ToSql, params_from_iter};
use sync_common::{ConflictRank, ReplicaSeq, StateKey, VersionKey, Versioned, wire};

pub(crate) fn is_record(kind: &str) -> bool {
    kind == "book_add"
}

pub(crate) fn snapshot(conn: &Connection, kind: &str, key: &StateKey, subkey: &str) -> Result<Option<(Vec<u8>, VersionKey)>, DatabaseError> {
    let stored: Option<(Vec<u8>, Vec<u8>)> = conn.records_snapshot(&kind, &key, &subkey, |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
    stored.map(|(body, token)| Ok((body, wire::decode(&token, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?))).transpose()
}

pub(crate) fn publication_version(conn: &Connection, kind: &str, key: &StateKey, subkey: &str) -> Result<Option<VersionKey>, DatabaseError> {
    let token: Option<Vec<u8>> = conn.records_publication_version(&kind, &key, &subkey, |row| row.get(0)).optional()?;
    token.map(|token| wire::decode(&token, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)).transpose()
}

/// Keep an offline addition locally until it has a definitive enrichment answer.
pub(crate) fn stage(conn: &Connection, hash: &sync_common::BookId, value: library_replica::BookAddition) -> Result<(), DatabaseError> {
    if value.enrichment.is_some() {
        return Err(DatabaseError::message("completed addition cannot be staged"));
    }
    if conn.records_contains(&"book_add", &hash, |row| row.get::<_, bool>(0))? {
        return Ok(());
    }
    let body = wire::encode(&LibraryStateEntry::BookAdd { book_id: *hash, value }).map_err(DatabaseError::operation)?;
    if conn.records_stage(&hash, &body)? != 0 {
        conn.records_mark_projection_dirty(&"book_add", &hash)?;
    }
    Ok(())
}

/// Insert once. Sequence allocation identifies the publication, not an edit.
pub(crate) fn insert(conn: &Connection, body: &LibraryStateEntry) -> Result<(), DatabaseError> {
    let address = body.address();
    if conn.records_contains(&address.kind, &address.entity_key, |r| r.get::<_, bool>(0))? {
        return Ok(());
    }
    let replica: String = conn.records_replica_id(|r| r.get(0))?;
    // The shared transport still carries a VersionKey. Its timestamp is fixed
    // and its rank is Update; this path never reads or advances an edit clock.
    let token = VersionKey::new(0, ConflictRank::Update, replica.parse().map_err(DatabaseError::operation)?, next_sequence(conn)?);
    store(conn, body, &token)?;
    schedule(conn, &address.kind, &address.entity_key, "", Some(&token))
}

/// Publish a bounded batch of additions proven absent under the caller's
/// writer transaction. A conflicting insertion fails the entire transaction.
pub(crate) fn insert_new_batch(conn: &Connection, values: impl IntoIterator<Item = LibraryStateEntry>) -> Result<(), DatabaseError> {
    let encoded = values
        .into_iter()
        .map(|value| {
            value.validate().map_err(DatabaseError::operation)?;
            let LibraryStateEntry::BookAdd { book_id, .. } = &value else {
                return Err(DatabaseError::message("immutable batch requires book additions"));
            };
            Ok((*book_id, wire::encode(&value).map_err(DatabaseError::operation)?))
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    if encoded.is_empty() {
        return Ok(());
    }
    let (first, replica) = allocate_sequences(conn, encoded.len())?;
    let mut publications = Vec::with_capacity(encoded.len());
    let mut records = Vec::with_capacity(encoded.len());
    for (index, (hash, body)) in encoded.into_iter().enumerate() {
        let sequence = first + index as i64;
        let token = VersionKey::new(0, ConflictRank::Update, replica, ReplicaSeq::new(u64::try_from(sequence).map_err(DatabaseError::operation)?).map_err(DatabaseError::operation)?);
        records.push((hash, body, wire::encode(&token).map_err(DatabaseError::operation)?));
        publications.push((hash, sequence));
    }
    let mut sql = String::from("INSERT INTO sync_book_record(state_kind,content_hash,body,publication) VALUES ");
    let mut bindings: Vec<&dyn ToSql> = Vec::with_capacity(records.len() * 3);
    for (index, (hash, body, publication)) in records.iter().enumerate() {
        if index != 0 {
            sql.push(',');
        }
        sql.push_str("('book_add',?,?,?)");
        bindings.extend([hash as &dyn ToSql, body as &dyn ToSql, publication as &dyn ToSql]);
    }
    conn.prepare(&sql)?.execute(params_from_iter(bindings))?;
    let publications = serde_json::to_string(&publications).map_err(DatabaseError::operation)?;
    conn.records_reset_cover_checks_batch(&publications)?;
    conn.records_discard_drafts_batch(&publications)?;
    conn.records_mark_projection_dirty_batch(&publications)?;
    conn.records_schedule_batch(&publications)?;
    Ok(())
}

/// Adopt the server's accepted record. An earlier local candidate can differ.
pub(crate) fn confirm(conn: &Connection, state: &Versioned<LibraryStateEntry>) -> Result<bool, DatabaseError> {
    let body = &state.value;
    body.validate().map_err(DatabaseError::operation)?;
    if state.version.conflict_rank != ConflictRank::Update {
        return Err(DatabaseError::message("immutable publication has an invalid transport rank"));
    }
    let hash = body.book().ok_or_else(|| DatabaseError::message("immutable record has no book"))?;
    let address = body.address();
    let current = read_merge_state(conn, &address.kind, &address.entity_key, "", Some(&hash))?;
    if !current.can_merge || !store(conn, body, &state.version)? {
        return Ok(false);
    }
    if current.queued {
        schedule(conn, &address.kind, &address.entity_key, "", None)?;
    }
    Ok(true)
}

fn store(conn: &Connection, body: &LibraryStateEntry, token: &VersionKey) -> Result<bool, DatabaseError> {
    let address = body.address();
    let bytes = wire::encode(body).map_err(DatabaseError::operation)?;
    let publication = wire::encode(token).map_err(DatabaseError::operation)?;
    let changed = conn.records_store(&address.kind, &address.entity_key, &bytes, &publication)? != 0;
    if changed {
        conn.records_reset_cover_check(&(&address.entity_key))?;
        conn.records_discard_draft(&(&address.entity_key))?;
        conn.records_mark_projection_dirty(&address.kind, &address.entity_key)?;
    }
    Ok(changed)
}

include_sqlite_sql::include_sql!("src/sync/sql/records_operations.sql");

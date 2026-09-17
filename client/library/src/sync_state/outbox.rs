use crate::sync_state::SyncSql;
use library_replica::MutationBody;
use library_replica::StateMutation;
use rusqlite::Connection;
use sync_common::MutationId;
use sync_common::ReplicaSeq;

#[derive(Debug)]
pub enum OutboxReadError {
    Database(rusqlite::Error),
    CorruptMutation { replica_seq: i64, reason: &'static str },
}

impl std::fmt::Display for OutboxReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "failed to read local sync outbox: {error}"),
            Self::CorruptMutation { replica_seq, reason } => write!(formatter, "corrupt local sync mutation at replica sequence {replica_seq}: {reason}"),
        }
    }
}

impl std::error::Error for OutboxReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptMutation { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OutboxRow {
    pub mutation_id: String,
    pub state_kind: String,
    pub state_key: String,
    pub body: Vec<u8>,
    pub changed_at: i64,
    pub replica_seq: i64,
    pub state_subkey: String,
}

pub(crate) fn decode_outbox_row(row: OutboxRow) -> Result<StateMutation, OutboxReadError> {
    let raw_local_seq = row.replica_seq;
    let corrupt = |reason| OutboxReadError::CorruptMutation { replica_seq: raw_local_seq, reason };
    if row.state_kind == "catalog_book" {
        return Err(corrupt("catalog-book mutations are no longer supported"));
    }
    let mutation_id = MutationId::parse(&row.mutation_id).map_err(|_| corrupt("invalid mutation id"))?;
    let body: MutationBody = sync_common::wire::decode(&row.body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|_| corrupt("body is not a canonical mutation"))?;
    let (kind, key, subkey) = canonical_outbox_key(&body);
    if row.state_kind != kind || row.state_key != key || row.state_subkey != subkey {
        return Err(corrupt("stored coalescing identity does not match the mutation body"));
    }
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let replica_seq = ReplicaSeq::new(u64::try_from(row.replica_seq).map_err(|_| corrupt("negative replica sequence"))?).map_err(|_| corrupt("replica sequence must be positive"))?;
    Ok(StateMutation { mutation_id, body, changed_at, replica_seq })
}

fn canonical_outbox_key(body: &MutationBody) -> (&'static str, String, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.to_string(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.to_string(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.to_string(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.to_string()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::CatalogBook { .. } => unreachable!("catalog mutations are rejected before decoding"),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string(), String::new()),
        MutationBody::PdfReaderMetadata { content_hash, value } => ("pdf_reader_metadata", content_hash.to_string(), value.checksum.clone()),
        MutationBody::Description { content_hash, .. } => ("description", content_hash.to_string(), String::new()),
        MutationBody::BookToc { content_hash, .. } => ("book_toc", content_hash.to_string(), String::new()),
    }
}

pub fn get_pending_mutations(conn: &Connection) -> Result<Vec<StateMutation>, OutboxReadError> {
    let mut mutations = Vec::new();
    let mut decode_error = None;
    let query_result = conn.get_pending_mutations(|row| {
        let stored = OutboxRow { mutation_id: row.get(0)?, state_kind: row.get(1)?, state_key: row.get(2)?, body: row.get(3)?, changed_at: row.get(4)?, replica_seq: row.get(5)?, state_subkey: row.get(6)? };
        match decode_outbox_row(stored) {
            Ok(change) => {
                mutations.push(change);
                Ok(())
            }
            Err(error) => {
                decode_error = Some(error);
                Err(rusqlite::Error::InvalidQuery)
            }
        }
    });
    if let Some(error) = decode_error {
        return Err(error);
    }
    query_result.map_err(OutboxReadError::Database)?;
    Ok(mutations)
}

/// Pending state stays durable while its exact local book version is
/// uploading (including rejected/retryable jobs). Only completion releases it.
pub fn get_publishable_mutations(conn: &Connection) -> Result<Vec<StateMutation>, OutboxReadError> {
    let pending = pending_uploads(conn)?;
    Ok(get_pending_mutations(conn)?.into_iter().filter(|m| book_ready(m, &pending)).collect())
}

fn pending_uploads(conn: &Connection) -> Result<std::collections::HashSet<String>, OutboxReadError> {
    if super::get_sync_metadata(conn, "cloud_storage_enabled").map_err(OutboxReadError::Database)?.as_deref() == Some("0") {
        return Ok(Default::default());
    }
    let mut statement = conn.prepare(include_str!("sql/outbox/pending_uploads_select.sql")).map_err(OutboxReadError::Database)?;
    let rows = statement.query_map([], |row| row.get(0)).map_err(OutboxReadError::Database)?;
    rows.collect::<rusqlite::Result<_>>().map_err(OutboxReadError::Database)
}

fn book_ready(mutation: &StateMutation, pending: &std::collections::HashSet<String>) -> bool {
    mutation.body.upload_dependency().is_none_or(|hash| !pending.contains(hash.as_str()))
}

#[cfg(test)]
#[path = "outbox_upload_tests.rs"]
mod upload_tests;

pub fn acknowledge_mutations(conn: &Connection, mutation_ids: &[MutationId]) -> Result<(), rusqlite::Error> {
    if mutation_ids.is_empty() {
        return Ok(());
    }
    let mut statement = conn.prepare_cached(include_str!("sql/outbox/acknowledge_mutations_delete.sql"))?;
    for mutation_id in mutation_ids {
        statement.execute([mutation_id.to_string()])?;
    }
    Ok(())
}

/// Fix the end of a scan so continuous local edits cannot keep it running.
pub fn outbox_watermark(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(include_str!("sql/outbox/outbox_watermark_select.sql"), [], |row| row.get(0))
}

/// Keyset paging remains valid when acknowledgments remove rows or local writes
/// replace them between commands. At least one row is returned even if its body
/// exceeds the byte target, so oversized mutations cannot starve later rows.
pub fn pending_mutations_page(conn: &Connection, after: i64, through: i64) -> Result<Vec<StateMutation>, OutboxReadError> {
    const BYTE_TARGET: usize = 256 * 1024;
    let pending = pending_uploads(conn)?;
    let mut statement = conn.prepare(include_str!("sql/outbox/pending_mutations_page_select.sql")).map_err(OutboxReadError::Database)?;
    let mut rows = statement.query(rusqlite::params![after, through]).map_err(OutboxReadError::Database)?;
    let mut mutations = Vec::new();
    let mut bytes = 0;
    while let Some(row) = rows.next().map_err(OutboxReadError::Database)? {
        let stored = (|| -> Result<OutboxRow, rusqlite::Error> {
            Ok(OutboxRow { mutation_id: row.get(0)?, state_kind: row.get(1)?, state_key: row.get(2)?, body: row.get(3)?, changed_at: row.get(4)?, replica_seq: row.get(5)?, state_subkey: row.get(6)? })
        })()
        .map_err(OutboxReadError::Database)?;
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
}

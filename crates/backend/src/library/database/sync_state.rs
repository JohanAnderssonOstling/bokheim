use super::*;
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
    pub(crate) mutation_id: String,
    pub(crate) state_kind: String,
    pub(crate) state_key: String,
    pub(crate) body: Vec<u8>,
    pub(crate) changed_at: i64,
    pub(crate) replica_seq: i64,
    pub(crate) state_subkey: String,
}
pub(crate) fn decode_outbox_row(row: OutboxRow) -> Result<StateMutation, OutboxReadError> {
    let raw_local_seq = row.replica_seq;
    let corrupt = |reason| OutboxReadError::CorruptMutation { replica_seq: raw_local_seq, reason };
    let mutation_id = MutationId::parse(&row.mutation_id).map_err(|_| corrupt("invalid mutation id"))?;
    let body: MutationBody = sync_common::wire::decode(&row.body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|_| corrupt("body is not a canonical mutation"))?;
    let (expected_kind, expected_key, expected_subkey) = canonical_outbox_key(&body);
    let key_matches = row.state_key == expected_key && row.state_subkey == expected_subkey;
    if row.state_kind != expected_kind || !key_matches {
        return Err(corrupt("stored coalescing identity does not match the mutation body"));
    }
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let replica_seq = u64::try_from(row.replica_seq).map_err(|_| corrupt("negative replica sequence"))?;
    let replica_seq = ReplicaSeq::new(replica_seq).map_err(|_| corrupt("replica sequence must be positive"))?;
    Ok(StateMutation { mutation_id, body, changed_at, replica_seq })
}
fn canonical_outbox_key(body: &MutationBody) -> (&'static str, String, String) {
    match body {
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.as_str().to_owned(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.as_str().to_owned(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.as_str().to_owned(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.as_str().to_owned()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::Bookmark { bookmark_id, .. } => ("bookmark", bookmark_id.clone(), String::new()),
        MutationBody::BookLink { link_id, .. } => ("book_link", link_id.clone(), String::new()),
        MutationBody::Metadata { content_hash, .. } => ("metadata", content_hash.to_string(), String::new()),
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
pub fn acknowledge_mutations(conn: &Connection, mutation_ids: &[MutationId]) -> Result<(), rusqlite::Error> {
    for mutation_id in mutation_ids {
        conn.acknowledge_mutation(&mutation_id.to_string())?;
    }
    Ok(())
}
/// Bumps and returns the monotonic scan generation counter. Each full library
/// scan claims a fresh id, stamps every placement it sees with it, and then
/// reconciles unseen local placements via [`mark_vanished_book_dirs_unavailable`].
pub fn next_scan_id(conn: &Connection) -> Result<i64, rusqlite::Error> {
    let current = match get_sync_metadata(conn, "scan_seq")? {
        Some(value) => value.parse::<i64>().map_err(|error| invalid_text_column(0, error.to_string()))?,
        None => 0,
    };
    let next = current.checked_add(1).ok_or_else(|| invalid_text_column(0, "scan sequence overflow"))?;
    set_sync_metadata(conn, "scan_seq", &next.to_string())?;
    Ok(next)
}
/// Consecutive scans a downloaded file must be absent before it's deleted. The
/// grace rides out transient misses; because deletions propagate to other devices
/// (destructive), the scanner pairs it with a scheduled re-scan so the count can
/// advance to this threshold.
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub const VANISH_GRACE_SCANS: i64 = 2;
/// Soft-deletes locally-present placements absent for at least [`VANISH_GRACE_SCANS`]
/// scans (files removed from disk). A move surfaces here as the old path being
/// deleted while the new path was added during the same scan. Emits
/// book_dir_removed events.
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn mark_vanished_book_dirs_unavailable(conn: &Connection, scan_id: i64) -> Result<(), rusqlite::Error> {
    conn.mark_vanished_book_dirs_unavailable(scan_id, VANISH_GRACE_SCANS)?;
    Ok(())
}
/// Count of placements missing this scan but still within the grace window, i.e.
/// pending deletions a follow-up scan should revisit.
#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn count_pending_vanish_candidates(conn: &Connection, scan_id: i64) -> Result<i64, rusqlite::Error> {
    let mut count = 0_i64;
    conn.count_pending_vanish_candidates(scan_id, VANISH_GRACE_SCANS, |row| {
        count = row.get(0)?;
        Ok(())
    })?;
    Ok(count)
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
/// A placement removed by synchronization so the caller can reconcile its file.
#[derive(Debug, Clone)]
pub struct RemovedBookDir {
    pub dir_id: DirId,
    pub content_hash: ContentHash,
}

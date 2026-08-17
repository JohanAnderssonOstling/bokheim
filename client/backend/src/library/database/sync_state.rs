use super::*;
use rusqlite::OptionalExtension;
use sync_common::StateCell;
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
        MutationBody::DirectoryName { dir_id, .. } => ("directory_name", dir_id.to_string(), String::new()),
        MutationBody::DirectoryParent { dir_id, .. } => ("directory_parent", dir_id.to_string(), String::new()),
        MutationBody::DirectoryLifecycle { dir_id, .. } => ("directory_lifecycle", dir_id.to_string(), String::new()),
        MutationBody::BookLifecycle { content_hash, .. } => ("book_lifecycle", content_hash.to_string(), String::new()),
        MutationBody::Placement { dir_id, content_hash, .. } => ("placement", content_hash.to_string(), dir_id.to_string()),
        MutationBody::ReadingPosition { content_hash, .. } => ("reading_position", content_hash.to_string(), String::new()),
        MutationBody::Annotation { annotation_id, .. } => ("annotation", annotation_id.clone(), String::new()),
        MutationBody::Bookmark { bookmark_id, .. } => ("bookmark", bookmark_id.clone(), String::new()),
        MutationBody::BookLink { link_id, .. } => ("book_link", link_id.clone(), String::new()),
        MutationBody::CatalogPublication { source_id, publication, .. } => ("catalog_publication", source_id.as_str().to_owned(), catalog_publication_state_subkey(&publication.edition_id, publication.work_id.as_deref())),
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

pub fn state_inventory(conn: &Connection) -> Result<Vec<StateCell>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT state_kind, state_key, state_subkey
         FROM sync_state_version
         ORDER BY state_kind, state_key, state_subkey",
    )?;
    let cells = statement.query_map([], |row| Ok(StateCell { kind: row.get(0)?, entity_key: row.get(1)?, entity_subkey: row.get(2)? }))?.collect();
    cells
}

/// Recreates durable outbox entries for state cells confirmed absent on the
/// server. Current domain values are encoded by the same SQLite functions used
/// by ordinary mutation triggers, so repair cannot drift from normal sync.
pub fn enqueue_missing_state_cells(conn: &mut Connection, cells: &[StateCell]) -> Result<usize, rusqlite::Error> {
    if cells.is_empty() {
        return Ok(0);
    }
    let transaction = conn.transaction()?;
    let changed_at = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(i64::MAX);
    let mut inserted = 0;
    for cell in cells {
        let (body, conflict_rank) = current_state_body(&transaction, cell)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        inserted += transaction.execute(
            "INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(state_kind,state_key,state_subkey) DO NOTHING",
            rusqlite::params![cell.kind, cell.entity_key, cell.entity_subkey, body, changed_at, conflict_rank],
        )?;
    }
    transaction.commit()?;
    Ok(inserted)
}

fn current_state_body(transaction: &Transaction<'_>, cell: &StateCell) -> Result<Option<(Vec<u8>, i64)>, rusqlite::Error> {
    let key = cell.entity_key.as_str();
    let subkey = cell.entity_subkey.as_str();
    let row = match cell.kind.as_str() {
        "directory_name" => transaction
            .query_row("SELECT bokheim_mutation('directory_name',id,name),0 FROM dir WHERE id=?1", [key], pair)
            .optional()?,
        "directory_parent" => transaction
            .query_row("SELECT bokheim_mutation('directory_parent',id,parent_id),0 FROM dir WHERE id=?1", [key], pair)
            .optional()?,
        "directory_lifecycle" => transaction
            .query_row("SELECT bokheim_mutation('directory_lifecycle',id,CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END),CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END FROM dir WHERE id=?1", [key], pair)
            .optional()?,
        "book_lifecycle" => {
            let lifecycle = transaction
                .query_row("SELECT CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END FROM book WHERE content_hash=?1", [key], |row| row.get::<_, i64>(0))
                .optional()?;
            match lifecycle {
                Some(2) => transaction
                    .query_row("SELECT bokheim_mutation('book_lifecycle',content_hash,2,'',X'','',X'',0,'',NULL),2 FROM book WHERE content_hash=?1", [key], pair)
                    .optional()?,
                Some(1) => transaction
                    .query_row("SELECT bokheim_mutation('book_lifecycle',content_hash,1,'',X'','',X'',0,'',trash_origin_dir_id),1 FROM book WHERE content_hash=?1", [key], pair)
                    .optional()?,
                Some(0) => transaction
                    .query_row(
                        "SELECT bokheim_mutation('book_lifecycle',content_hash,0,COALESCE(title,content_hash),contributors_for_book(row_id),COALESCE(description,''),publication_metadata,COALESCE(added_at,0),format,NULL),0 FROM book WHERE content_hash=?1",
                        [key], pair,
                    )
                    .optional()?,
                Some(_) | None => None,
            }
        }
        "placement" => transaction
            .query_row(
                "SELECT bokheim_mutation('placement',book_dir.dir_id,book.content_hash,book_dir.deleted_at IS NULL,book_dir.trash_origin_dir_id),book_dir.deleted_at IS NOT NULL
                 FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id
                 WHERE book.content_hash=?1 AND book_dir.dir_id=?2",
                [key, subkey], pair,
            )
            .optional()?,
        "reading_position" => transaction
            .query_row(
                "SELECT bokheim_mutation('reading_position',content_hash,read_pos,COALESCE(read_progress,0)),0 FROM book WHERE content_hash=?1 AND read_pos IS NOT NULL",
                [key], pair,
            )
            .optional()?,
        "metadata" => transaction
            .query_row(
                "SELECT bokheim_mutation('metadata',content_hash,COALESCE(title,''),contributors_for_book(row_id),COALESCE(description,''),publication_metadata),0 FROM book WHERE content_hash=?1",
                [key], pair,
            )
            .optional()?,
        "annotation" => transaction
            .query_row(
                "SELECT bokheim_mutation('annotation',annotation.id,book.content_hash,annotation.anchor_kind,annotation.anchor_cfi,annotation.anchor_page,annotation.anchor_rects,annotation.anchor_fallback_cfi,annotation.exact_text,annotation.style,annotation.color,annotation.note,annotation.created_at,annotation.modified_at,annotation.deleted_at IS NOT NULL),annotation.deleted_at IS NOT NULL
                 FROM annotation JOIN book ON book.row_id=annotation.book_row_id WHERE annotation.id=?1",
                [key], pair,
            )
            .optional()?,
        "bookmark" => transaction
            .query_row(
                "SELECT bokheim_mutation('bookmark',bookmark.id,book.content_hash,bookmark.cfi,bookmark.label,bookmark.created_at,bookmark.modified_at,bookmark.deleted_at IS NOT NULL),bookmark.deleted_at IS NOT NULL
                 FROM bookmark JOIN book ON book.row_id=bookmark.book_row_id WHERE bookmark.id=?1",
                [key], pair,
            )
            .optional()?,
        "book_link" => transaction
            .query_row(
                "SELECT bokheim_mutation('book_link',book_link.id,source.content_hash,target.content_hash,book_link.relation,book_link.note,book_link.created_at,book_link.modified_at,book_link.deleted_at IS NOT NULL),book_link.deleted_at IS NOT NULL
                 FROM book_link JOIN book source ON source.row_id=book_link.source_book_row_id JOIN book target ON target.row_id=book_link.target_book_row_id WHERE book_link.id=?1",
                [key], pair,
            )
            .optional()?,
        "catalog_publication" => transaction
            .query_row(
                "SELECT bokheim_mutation('catalog_publication',catalog.source_id,catalog.edition_id,NULLIF(catalog.work_id,''),book.content_hash),0
                 FROM catalog_publication_book catalog JOIN book ON book.row_id=catalog.book_row_id
                 WHERE catalog.source_id=?1 AND catalog_publication_state_subkey(catalog.edition_id,NULLIF(catalog.work_id,''))=?2",
                [key, subkey], pair,
            )
            .optional()?,
        _ => None,
    };
    Ok(row)
}

fn pair(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Vec<u8>, i64)> {
    Ok((row.get(0)?, row.get(1)?))
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

fn parse_revision(value: Option<String>) -> Result<Option<sync_common::LibraryRevision>, rusqlite::Error> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))?;
            sync_common::LibraryRevision::new(raw).map_err(|error| invalid_text_column(0, error.to_string()))
        })
        .transpose()
}

/// Reads the partitioned pull cursor, promoting the legacy scalar checkpoint to
/// both channels until the first successful version-12 exchange persists it.
pub fn get_pull_cursor(conn: &Connection) -> Result<sync_common::SyncCursor, rusqlite::Error> {
    let state = get_sync_metadata(conn, "last_pull_state_seq")?;
    let reading = get_sync_metadata(conn, "last_pull_reading_seq")?;
    if state.is_none() && reading.is_none() {
        let legacy = parse_revision(get_sync_metadata(conn, "last_pull_server_seq")?)?;
        return Ok(sync_common::SyncCursor { state_revision: legacy, reading_revision: legacy });
    }
    Ok(sync_common::SyncCursor { state_revision: parse_revision(state)?, reading_revision: parse_revision(reading)? })
}

pub fn set_pull_cursor(conn: &Connection, cursor: sync_common::SyncCursor) -> Result<(), rusqlite::Error> {
    for (key, revision) in [("last_pull_state_seq", cursor.state_revision), ("last_pull_reading_seq", cursor.reading_revision)] {
        if let Some(revision) = revision {
            set_sync_metadata(conn, key, &revision.get().to_string())?;
        } else {
            conn.execute("DELETE FROM sync_metadata WHERE key=?1", [key])?;
        }
    }
    conn.execute("DELETE FROM sync_metadata WHERE key='last_pull_server_seq'", [])?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LibrarySyncDetails {
    pub(crate) has_unsynced_changes: bool,
    pub(crate) last_synced_at: Option<u64>,
}

pub(crate) fn get_library_sync_details(conn: &Connection) -> Result<LibrarySyncDetails, rusqlite::Error> {
    let has_unsynced_changes = conn.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox LIMIT 1)", [], |row| row.get(0))?;
    let last_synced_at = get_sync_metadata(conn, "last_successful_sync_at")?.map(|value| value.parse::<u64>().map_err(|error| invalid_text_column(0, error.to_string()))).transpose()?;
    Ok(LibrarySyncDetails { has_unsynced_changes, last_synced_at })
}

/// Starts remote reconciliation from a clean account boundary while retaining
/// every local book, placement, annotation, and pending mutation.
pub fn rebind_sync_account(conn: &Connection, user_id: &str) -> Result<(), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM remote_asset", [])?;
    tx.execute("DELETE FROM sync_metadata WHERE key = 'last_pull_server_seq'", [])?;
    tx.execute("DELETE FROM sync_metadata WHERE key IN ('last_pull_state_seq','last_pull_reading_seq')", [])?;
    tx.execute("DELETE FROM sync_metadata WHERE key = 'last_successful_sync_at'", [])?;
    tx.execute("DELETE FROM sync_metadata WHERE key = 'cursor_recovery_pending'", [])?;
    tx.execute("INSERT INTO sync_metadata (key, value) VALUES ('bound_user_id', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [user_id])?;
    tx.commit()
}
/// A placement removed by synchronization so the caller can reconcile its file.
#[derive(Debug, Clone)]
pub struct RemovedBookDir {
    pub dir_id: DirId,
    pub content_hash: ContentHash,
}

/// A filesystem effect an applied pull page still owes the disk.
///
/// Identities and paths are stored as text rather than the domain newtypes:
/// the row outlives the process that wrote it, so it is re-parsed (and so
/// re-validated against the library root) on the way back out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsAction {
    /// Create the directory and its identity marker.
    MaterializeDir { dir_id: String },
    /// Relocate a directory that synchronization moved or renamed.
    MoveDir { dir_id: String, old_path: String, new_path: String },
    /// Reconcile the file of a placement that a synchronized removal retired.
    RetirePlacementFile { dir_id: String, content_hash: String },
    /// Remove a directory synchronization no longer projects.
    RemoveDir { path: String },
}

/// Records the filesystem work owed by a page. Must be called in the same
/// transaction that advances the pull cursor: a cursor that moves past work
/// this replica never performed can never be told to perform it again.
pub fn enqueue_fs_actions(tx: &Transaction<'_>, actions: &[FsAction]) -> Result<(), rusqlite::Error> {
    if actions.is_empty() {
        return Ok(());
    }
    let created_at = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(i64::MAX);
    for action in actions {
        let encoded = sync_common::wire::encode(action).map_err(|error| invalid_text_column(0, error.to_string()))?;
        tx.execute("INSERT INTO pending_fs_action(action, created_at) VALUES(?1, ?2)", rusqlite::params![encoded, created_at])?;
    }
    Ok(())
}

/// Owed filesystem work in the order it was recorded. An entry that cannot be
/// decoded is reported as `None` so the caller can retire it rather than let
/// one unreadable row block every action queued behind it.
pub fn pending_fs_actions(conn: &Connection) -> Result<Vec<(i64, Option<FsAction>)>, rusqlite::Error> {
    let mut statement = conn.prepare("SELECT id, action FROM pending_fs_action ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let encoded: Vec<u8> = row.get(1)?;
        Ok((id, sync_common::wire::decode::<FsAction>(&encoded, sync_common::wire::MAX_DECODED_REQUEST_BYTES).ok()))
    })?;
    rows.collect()
}

pub fn complete_fs_action(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM pending_fs_action WHERE id=?1", [id])?;
    Ok(())
}

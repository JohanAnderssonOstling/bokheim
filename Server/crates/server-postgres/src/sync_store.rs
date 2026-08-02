use serde_json::Value;
use sqlx::postgres::PgArguments;
use sqlx::query::Query;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{singleton_event_types_sql_list, ActorId, ClientId, ContentHash, EventId, Hlc, LibraryId, LocalChange, LocalSeq, PushRejection, PushRejectionReason, ServerChange, ServerSeq, SyncOp};

const HLC_FUTURE_TOLERANCE_MS: u64 = 60 * 60 * 1000;

pub(super) async fn list_libraries(pool: &PgPool, user_id: &str) -> Result<Vec<LibrarySummary>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name FROM libraries WHERE user_id = $1 ORDER BY lower(name), id").bind(user_id).fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            let raw_id: String = row.try_get("id")?;
            let library_id = LibraryId::parse(&raw_id).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
            Ok(LibrarySummary { library_id, library_name: row.try_get("name")? })
        })
        .collect()
}

pub(super) async fn upsert_library(pool: &PgPool, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibrarySummary>, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO libraries (id, user_id, name) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name
         WHERE libraries.user_id = EXCLUDED.user_id
         RETURNING id, name",
    )
    .bind(request.library_id.as_str())
    .bind(user_id)
    .bind(&request.library_name)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        let raw_id: String = row.try_get("id")?;
        let library_id = LibraryId::parse(&raw_id).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        Ok(LibrarySummary { library_id, library_name: row.try_get("name")? })
    })
    .transpose()
}

pub(super) async fn ensure_library(pool: &PgPool, user_id: &str, library_id: &LibraryId) -> Result<bool, sqlx::Error> {
    // Atomic create-or-fetch-owner. On a fresh library this inserts our row; on
    // an existing one the no-op `DO UPDATE` makes RETURNING surface the current
    // owner (a plain `DO NOTHING` returns no row on conflict). Doing it in one
    // statement closes the TOCTOU where a separate SELECT-then-INSERT could let
    // a second user be granted access to a library another user just created.
    let owner = sqlx::query_scalar::<_, String>(
        "insert into libraries (id, user_id) values ($1, $2)
         on conflict (id) do update set user_id = libraries.user_id
         returning user_id",
    )
    .bind(library_id.as_str())
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(owner == user_id)
}

pub(super) struct InsertChangesResult {
    pub handled_local_seqs: Vec<LocalSeq>,
    pub rejected: Vec<PushRejection>,
}

pub(super) async fn insert_changes(tx: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &LibraryId, client_id: &ClientId, changes: &[LocalChange]) -> Result<InsertChangesResult, sqlx::Error> {
    let mut handled_local_seqs = Vec::new();
    let mut rejected = Vec::new();

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let max_allowed_hlc_pt = now_ms.saturating_add(HLC_FUTURE_TOLERANCE_MS);

    for change in changes {
        let local_seq = change.local_seq.get() as i64;
        if change.op.validate().is_err() {
            rejected.push(PushRejection { local_seq: change.local_seq, reason: PushRejectionReason::InvalidPayload });
            continue;
        }
        if let Some(required_dir) = required_directory(&change.op) {
            let exists = required_dir.as_str() == "__root__"
                || sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(
                         SELECT 1 FROM library_events
                         WHERE library_id=$1 AND event_type='dir_state' AND entity_id=$2
                     )",
                )
                .bind(library_id.as_str())
                .bind(required_dir.as_str())
                .fetch_one(&mut **tx)
                .await?;
            if !exists {
                rejected.push(PushRejection { local_seq: change.local_seq, reason: PushRejectionReason::MissingDependency });
                continue;
            }
        }
        if change.hlc.pt > max_allowed_hlc_pt {
            rejected.push(PushRejection { local_seq: change.local_seq, reason: PushRejectionReason::HlcTooFarFuture });
            continue;
        }
        let (Ok(changed_at), Ok(hlc_pt), Ok(hlc_l)) = (i64::try_from(change.changed_at), i64::try_from(change.hlc.pt), i64::try_from(change.hlc.l)) else {
            rejected.push(PushRejection { local_seq: change.local_seq, reason: PushRejectionReason::ValueOutsideStorageRange });
            continue;
        };
        let entity_id = change.op.entity_id();
        let row = EventRow {
            library_id: library_id.as_str(),
            user_id,
            event_type: change.op.event_type(),
            entity_id: entity_id.as_ref(),
            payload: change.op.payload(),
            changed_at,
            client_id: client_id.as_str(),
            local_seq,
            hlc_pt,
            hlc_l,
            version_rank: change.op.conflict_rank() as i16,
            event_id: change.event_id.to_string(),
        };

        if change.op.is_singleton() {
            insert_singleton(tx, &row).await?;
        } else {
            insert_append(tx, &row).await?;
        }

        handled_local_seqs.push(change.local_seq);
    }

    Ok(InsertChangesResult { handled_local_seqs, rejected })
}

fn required_directory(op: &SyncOp) -> Option<&sync_common::DirId> {
    match op {
        SyncOp::DirState { directory, .. } => Some(directory.parent_id()),
        SyncOp::BookDirAdded { dir_id, .. } | SyncOp::BookDirRemoved { dir_id, .. } => Some(dir_id),
        _ => None,
    }
}

/// One `library_events` row's worth of bound columns, shared by both insert paths.
struct EventRow<'a> {
    library_id: &'a str,
    user_id: &'a str,
    event_type: &'a str,
    entity_id: &'a str,
    payload: Value,
    changed_at: i64,
    client_id: &'a str,
    local_seq: i64,
    hlc_pt: i64,
    hlc_l: i64,
    version_rank: i16,
    event_id: String,
}

fn bind_event<'q>(q: Query<'q, Postgres, PgArguments>, row: &'q EventRow<'q>) -> Query<'q, Postgres, PgArguments> {
    q.bind(row.library_id)
        .bind(row.user_id)
        .bind(row.event_type)
        .bind(row.entity_id)
        .bind(sqlx::types::Json(&row.payload))
        .bind(row.changed_at)
        .bind(row.client_id)
        .bind(row.local_seq)
        .bind(row.hlc_pt)
        .bind(row.hlc_l)
        .bind(row.version_rank)
        .bind(&row.event_id)
}

/// Singleton (LWW) insert: one row per `(library, entity, event_type)`, updated
/// only when the incoming total version is strictly greater. The
/// winning update re-draws `server_seq` so it resurfaces to clients that already
/// pulled the previous value.
async fn insert_singleton(tx: &mut Transaction<'_, Postgres>, row: &EventRow<'_>) -> Result<(), sqlx::Error> {
    let sql = format!(
        "INSERT INTO library_events (
            library_id, user_id, event_type, entity_id,
            data, timestamp, client_id, client_seq, hlc_pt, hlc_l, version_rank, event_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        ON CONFLICT (library_id, entity_id, event_type)
        WHERE event_type IN {list}
        DO UPDATE SET
            data = EXCLUDED.data,
            timestamp = EXCLUDED.timestamp,
            client_id = EXCLUDED.client_id,
            client_seq = EXCLUDED.client_seq,
            hlc_pt = EXCLUDED.hlc_pt,
            hlc_l = EXCLUDED.hlc_l,
            version_rank = EXCLUDED.version_rank,
            event_id = EXCLUDED.event_id,
            server_seq = DEFAULT
        WHERE (EXCLUDED.hlc_pt, EXCLUDED.hlc_l, EXCLUDED.version_rank, EXCLUDED.client_id COLLATE \"C\", EXCLUDED.client_seq, EXCLUDED.event_id COLLATE \"C\")
            > (library_events.hlc_pt, library_events.hlc_l, library_events.version_rank, library_events.client_id COLLATE \"C\", library_events.client_seq, library_events.event_id COLLATE \"C\")",
        list = singleton_event_types_sql_list(),
    );
    bind_event(sqlx::query(&sql), row).execute(&mut **tx).await?;
    Ok(())
}

/// Append insert: deduplicated by `(library, event_id)`. Re-sends
/// of an already-stored event are no-ops.
async fn insert_append(tx: &mut Transaction<'_, Postgres>, row: &EventRow<'_>) -> Result<(), sqlx::Error> {
    let sql = "INSERT INTO library_events (
            library_id, user_id, event_type, entity_id,
            data, timestamp, client_id, client_seq, hlc_pt, hlc_l, version_rank, event_id
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        ON CONFLICT (library_id, event_id)
        DO NOTHING";
    bind_event(sqlx::query(sql), row).execute(&mut **tx).await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(super) enum FetchChangesError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("unsupported stored event type '{event_type}' at server_seq {server_seq}")]
    UnsupportedEventType { server_seq: i64, event_type: String },
    #[error("corrupt stored event at server_seq {server_seq}: {reason}")]
    CorruptStoredEvent { server_seq: i64, reason: &'static str },
}

#[derive(Clone, Debug)]
struct StoredChangeRow {
    event_id: String,
    entity_id: String,
    event_type: String,
    payload: Value,
    changed_at: i64,
    actor_id: String,
    client_seq: i64,
    server_seq: i64,
    hlc_pt: i64,
    hlc_l: i64,
}

fn decode_stored_change(row: StoredChangeRow) -> Result<ServerChange, FetchChangesError> {
    let raw_server_seq = row.server_seq;
    let corrupt = |reason| FetchChangesError::CorruptStoredEvent { server_seq: raw_server_seq, reason };
    let changed_at = u64::try_from(row.changed_at).map_err(|_| corrupt("negative timestamp"))?;
    let client_seq = u64::try_from(row.client_seq).map_err(|_| corrupt("negative client sequence"))?;
    let server_seq = u64::try_from(row.server_seq).map_err(|_| corrupt("negative server sequence"))?;
    let hlc_pt = u64::try_from(row.hlc_pt).map_err(|_| corrupt("negative HLC physical component"))?;
    let hlc_l = u64::try_from(row.hlc_l).map_err(|_| corrupt("negative HLC logical component"))?;
    let event_id = EventId::parse(&row.event_id).map_err(|_| corrupt("invalid event identity"))?;

    if !SyncOp::is_known_event_type(&row.event_type) {
        return Err(FetchChangesError::UnsupportedEventType { server_seq: raw_server_seq, event_type: row.event_type });
    }
    let op = SyncOp::from_event_parts(&row.event_type, row.entity_id, &row.payload, changed_at).ok_or_else(|| corrupt("known event has invalid identity or payload"))?;
    let actor_id = ActorId::parse(&row.actor_id).map_err(|_| corrupt("invalid actor identity"))?;
    let local_seq = LocalSeq::new(client_seq).map_err(|_| corrupt("client sequence must be positive"))?;
    let server_seq = ServerSeq::new(server_seq).map_err(|_| corrupt("server sequence must be positive"))?;
    let change = LocalChange { event_id, op, changed_at, hlc: Hlc { pt: hlc_pt, l: hlc_l }, local_seq };
    Ok(ServerChange { change, actor_id, server_seq })
}

pub(super) async fn fetch_changes(pool: &PgPool, library_id: &LibraryId, _client_id: &ClientId, since_server_seq: Option<ServerSeq>) -> Result<Vec<ServerChange>, FetchChangesError> {
    let rows = sqlx::query(
        "SELECT event_id, entity_id, event_type, data, timestamp, client_id, client_seq, server_seq, hlc_pt, hlc_l
         FROM library_events
         WHERE library_id = $1 AND server_seq > $2
         ORDER BY server_seq ASC",
    )
    .bind(library_id.as_str())
    .bind(since_server_seq.map(ServerSeq::get).unwrap_or(0) as i64)
    .fetch_all(pool)
    .await?;

    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        changes.push(decode_stored_change(StoredChangeRow {
            event_id: row.try_get("event_id")?,
            entity_id: row.try_get("entity_id")?,
            event_type: row.try_get("event_type")?,
            payload: row.try_get("data")?,
            changed_at: row.try_get("timestamp")?,
            actor_id: row.try_get("client_id")?,
            client_seq: row.try_get("client_seq")?,
            server_seq: row.try_get("server_seq")?,
            hlc_pt: row.try_get("hlc_pt")?,
            hlc_l: row.try_get("hlc_l")?,
        })?);
    }

    Ok(changes)
}

pub(super) async fn authorized_content_hashes(pool: &PgPool, user_id: &str, library_id: &LibraryId, candidates: &[ContentHash]) -> Result<Option<std::collections::HashSet<ContentHash>>, sqlx::Error> {
    let owns_library = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM libraries WHERE id=$1 AND user_id=$2)").bind(library_id.as_str()).bind(user_id).fetch_one(pool).await?;
    if !owns_library {
        return Ok(None);
    }
    if candidates.is_empty() {
        return Ok(Some(std::collections::HashSet::new()));
    }

    let raw_candidates: Vec<&str> = candidates.iter().map(ContentHash::as_str).collect();
    let rows = sqlx::query_scalar::<_, Option<String>>(
        "WITH latest_per_occupant AS (
             SELECT DISTINCT ON (entity_id, data->>'dir_id', data->>'file_name')
                 data->>'dir_id' AS dir_id,
                 data->>'file_name' AS file_name,
                 event_type,
                 data->>'content_hash' AS content_hash,
                 hlc_pt, hlc_l, version_rank, client_id, client_seq, event_id
             FROM library_events
             WHERE library_id=$1 AND event_type IN ('book_dir_added', 'book_dir_removed')
             ORDER BY entity_id, data->>'dir_id', data->>'file_name',
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC, event_id COLLATE \"C\" DESC
         ), latest_per_path AS (
             SELECT DISTINCT ON (dir_id, file_name) content_hash
             FROM latest_per_occupant
             WHERE event_type='book_dir_added'
             ORDER BY dir_id, file_name,
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC, event_id COLLATE \"C\" DESC
         )
         SELECT DISTINCT content_hash
         FROM latest_per_path
         WHERE content_hash=ANY($2)",
    )
    .bind(library_id.as_str())
    .bind(&raw_candidates)
    .fetch_all(pool)
    .await?;

    Ok(Some(rows.into_iter().flatten().filter_map(|raw| ContentHash::parse(&raw).ok()).collect()))
}

/// Set of content hashes still referenced by the greatest-VersionKey event for
/// each physical `(library_id, dir_id, file_name)` path.
pub(super) async fn live_blob_hashes(tx: &mut Transaction<'_, Postgres>) -> Result<std::collections::HashSet<ContentHash>, sqlx::Error> {
    let rows = sqlx::query_scalar::<_, Option<String>>(
        "WITH latest_per_occupant AS (
             SELECT DISTINCT ON (library_id, entity_id, data->>'dir_id', data->>'file_name')
                 library_id,
                 data->>'dir_id' AS dir_id,
                 data->>'file_name' AS file_name,
                 event_type,
                 data->>'content_hash' AS content_hash,
                 hlc_pt, hlc_l, version_rank, client_id, client_seq, event_id
             FROM library_events
             WHERE event_type IN ('book_dir_added', 'book_dir_removed')
             ORDER BY library_id,
                      entity_id,
                      data->>'dir_id',
                      data->>'file_name',
                      hlc_pt DESC,
                      hlc_l DESC,
                      version_rank DESC,
                      client_id COLLATE \"C\" DESC,
                      client_seq DESC,
                      event_id COLLATE \"C\" DESC
         ), latest_per_path AS (
             SELECT DISTINCT ON (library_id, dir_id, file_name) content_hash
             FROM latest_per_occupant
             WHERE event_type = 'book_dir_added'
             ORDER BY library_id, dir_id, file_name,
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC,
                      event_id COLLATE \"C\" DESC
         )
         SELECT DISTINCT content_hash
         FROM latest_per_path
         WHERE content_hash IS NOT NULL AND content_hash <> ''",
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .flatten()
        .filter_map(|raw| match ContentHash::parse(&raw) {
            Ok(content_hash) => Some(content_hash),
            Err(_) => {
                eprintln!("live_blob_hashes: ignoring malformed content hash '{raw}'");
                None
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{decode_stored_change, required_directory, FetchChangesError, StoredChangeRow};
    use serde_json::json;
    use sync_common::SyncOp;

    fn valid_row() -> StoredChangeRow {
        StoredChangeRow {
            event_id: "00000000-0000-4000-8000-000000000001".to_string(),
            entity_id: "1".to_string(),
            event_type: "book_deleted".to_string(),
            payload: json!({}),
            changed_at: 10,
            actor_id: "actor-a".to_string(),
            client_seq: 1,
            server_seq: 2,
            hlc_pt: 10,
            hlc_l: 0,
        }
    }

    #[test]
    fn stored_change_decoder_accepts_valid_rows() {
        let change = decode_stored_change(valid_row()).expect("valid stored event");
        assert_eq!(change.server_seq.get(), 2);
        assert_eq!(change.actor_id.as_str(), "actor-a");
    }

    #[test]
    fn stored_change_decoder_distinguishes_unknown_event_types() {
        let mut row = valid_row();
        row.event_type = "future_event".to_string();
        assert!(matches!(decode_stored_change(row), Err(FetchChangesError::UnsupportedEventType { server_seq: 2, .. })));
    }

    #[test]
    fn stored_change_decoder_rejects_malformed_known_payloads() {
        let mut row = valid_row();
        row.event_type = "book_dir_added".to_string();
        assert!(matches!(decode_stored_change(row), Err(FetchChangesError::CorruptStoredEvent { server_seq: 2, .. })));
    }

    #[test]
    fn stored_change_decoder_rejects_semantically_invalid_payloads() {
        let mut row = valid_row();
        row.event_type = "reading_position_updated".to_string();
        row.payload = json!({ "cfi": "not-a-cfi" });
        assert!(matches!(decode_stored_change(row), Err(FetchChangesError::CorruptStoredEvent { server_seq: 2, .. })));
    }

    #[test]
    fn relational_events_declare_the_directory_the_server_must_already_know() {
        let directory = sync_common::DirId::parse("parent").unwrap();
        let op = SyncOp::DirState { dir_id: sync_common::DirId::parse("child").unwrap(), directory: sync_common::DirectoryState::Present { parent_id: directory.clone(), name: sync_common::FileName::parse("Child").unwrap() } };
        assert_eq!(required_directory(&op), Some(&directory));
        assert!(required_directory(&SyncOp::BookDeleted { book_id: sync_common::BookId::new(1) }).is_none());
    }

    #[test]
    fn stored_change_decoder_rejects_invalid_actor_identity() {
        let mut row = valid_row();
        row.actor_id.clear();
        assert!(matches!(decode_stored_change(row), Err(FetchChangesError::CorruptStoredEvent { server_seq: 2, .. })));
    }

    #[test]
    fn stored_change_decoder_rejects_invalid_sequences() {
        let mut row = valid_row();
        row.server_seq = 0;
        assert!(matches!(decode_stored_change(row), Err(FetchChangesError::CorruptStoredEvent { server_seq: 0, .. })));
    }
}

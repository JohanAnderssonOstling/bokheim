use serde_json::Value;
use sqlx::postgres::PgArguments;
use sqlx::query::Query;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::{singleton_event_types_sql_list, Change, Hlc, PushRejection, PushRejectionReason, SyncOp};

const HLC_FUTURE_TOLERANCE_MS: u64 = 60 * 60 * 1000;

pub async fn ensure_library(pool: &PgPool, user_id: &str, library_id: &str) -> Result<bool, sqlx::Error> {
    let existing = sqlx::query_scalar::<_, String>("select user_id from libraries where id = $1").bind(library_id).fetch_optional(pool).await?;

    match existing {
        Some(existing_user) => {
            if existing_user != user_id {
                return Ok(false);
            }
        }
        None => {
            sqlx::query(
                "insert into libraries (id, user_id) values ($1, $2)
                 on conflict (id) do nothing",
            )
            .bind(library_id)
            .bind(user_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(true)
}

pub struct InsertChangesResult {
    pub handled_local_seqs: Vec<u64>,
    pub rejected: Vec<PushRejection>,
}

pub async fn insert_changes(tx: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &str, client_id: &str, changes: &[Change]) -> Result<InsertChangesResult, sqlx::Error> {
    let mut handled_local_seqs = Vec::new();
    let mut rejected = Vec::new();

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let max_allowed_hlc_pt = now_ms.saturating_add(HLC_FUTURE_TOLERANCE_MS);

    for change in changes {
        let local_seq = change.local_seq.unwrap_or(0) as i64;
        if local_seq <= 0 {
            rejected.push(PushRejection { local_seq: change.local_seq.unwrap_or(0), reason: PushRejectionReason::MissingLocalSeq });
            continue;
        }
        if change.hlc.pt > max_allowed_hlc_pt {
            rejected.push(PushRejection { local_seq: local_seq as u64, reason: PushRejectionReason::HlcTooFarFuture });
            continue;
        }
        let row = EventRow {
            library_id,
            user_id,
            event_type: change.op.event_type(),
            entity_id: change.op.entity_id(),
            payload: change.op.payload().to_string(),
            changed_at: change.changed_at as i64,
            client_id,
            local_seq,
            hlc_pt: change.hlc.pt as i64,
            hlc_l: change.hlc.l as i64,
        };

        if change.op.is_singleton() {
            insert_singleton(tx, &row).await?;
        } else {
            insert_append(tx, &row).await?;
        }

        handled_local_seqs.push(local_seq as u64);
    }

    Ok(InsertChangesResult { handled_local_seqs, rejected })
}

/// One `library_events` row's worth of bound columns, shared by both insert paths.
struct EventRow<'a> {
    library_id: &'a str,
    user_id: &'a str,
    event_type: &'a str,
    entity_id: &'a str,
    payload: String,
    changed_at: i64,
    client_id: &'a str,
    local_seq: i64,
    hlc_pt: i64,
    hlc_l: i64,
}

fn bind_event<'q>(q: Query<'q, Postgres, PgArguments>, row: &'q EventRow<'q>) -> Query<'q, Postgres, PgArguments> {
    q.bind(row.library_id)
        .bind(row.user_id)
        .bind(row.event_type)
        .bind(row.entity_id)
        .bind(&row.payload)
        .bind(row.changed_at)
        .bind(row.client_id)
        .bind(row.local_seq)
        .bind(row.hlc_pt)
        .bind(row.hlc_l)
}

/// Singleton (LWW) insert: one row per `(library, entity, event_type)`, updated
/// only when the incoming `(hlc_pt, hlc_l, client_id)` is strictly greater. The
/// winning update re-draws `server_seq` so it resurfaces to clients that already
/// pulled the previous value.
async fn insert_singleton(tx: &mut Transaction<'_, Postgres>, row: &EventRow<'_>) -> Result<(), sqlx::Error> {
    let sql = format!(
        "INSERT INTO library_events (
            library_id, user_id, event_type, entity_id,
            data, timestamp, client_id, client_seq, hlc_pt, hlc_l
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        ON CONFLICT (library_id, entity_id, event_type)
        WHERE event_type IN {list}
        DO UPDATE SET
            data = EXCLUDED.data,
            timestamp = EXCLUDED.timestamp,
            client_id = EXCLUDED.client_id,
            client_seq = EXCLUDED.client_seq,
            hlc_pt = EXCLUDED.hlc_pt,
            hlc_l = EXCLUDED.hlc_l,
            server_seq = DEFAULT
        WHERE (EXCLUDED.hlc_pt, EXCLUDED.hlc_l, EXCLUDED.client_id)
            > (library_events.hlc_pt, library_events.hlc_l, library_events.client_id)",
        list = singleton_event_types_sql_list(),
    );
    bind_event(sqlx::query(&sql), row).execute(&mut **tx).await?;
    Ok(())
}

/// Append insert: deduplicated by `(library, client_id, client_seq)`. Re-sends
/// of an already-stored event are no-ops.
async fn insert_append(tx: &mut Transaction<'_, Postgres>, row: &EventRow<'_>) -> Result<(), sqlx::Error> {
    let sql = format!(
        "INSERT INTO library_events (
            library_id, user_id, event_type, entity_id,
            data, timestamp, client_id, client_seq, hlc_pt, hlc_l
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        ON CONFLICT (library_id, client_id, client_seq)
        WHERE event_type NOT IN {list}
        DO NOTHING",
        list = singleton_event_types_sql_list(),
    );
    bind_event(sqlx::query(&sql), row).execute(&mut **tx).await?;
    Ok(())
}

pub async fn fetch_changes(pool: &PgPool, library_id: &str, client_id: &str, since_server_seq: u64) -> Result<Vec<Change>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT entity_id, event_type, data, timestamp, client_seq, server_seq, hlc_pt, hlc_l
         FROM library_events
         WHERE library_id = $1 AND server_seq > $2 AND client_id != $3
         ORDER BY server_seq ASC",
    )
    .bind(library_id)
    .bind(since_server_seq as i64)
    .bind(client_id)
    .fetch_all(pool)
    .await?;

    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let entity_id: String = row.get("entity_id");
        let entity_type: String = row.get("event_type");
        let payload: Value = row.get("data");
        let changed_at: i64 = row.get("timestamp");
        let client_seq: i64 = row.get("client_seq");
        let server_seq: i64 = row.get("server_seq");
        let hlc_pt: i64 = row.get("hlc_pt");
        let hlc_l: i64 = row.get("hlc_l");
        let changed_at = changed_at.max(0) as u64;
        let Some(op) = SyncOp::from_event_parts(&entity_type, entity_id.clone(), &payload, changed_at) else {
            // Malformed or unknown event_type. Surface loudly rather than silently
            // dropping it and advancing the client's cursor past it forever.
            return Err(sqlx::Error::Protocol(format!(
                "fetch_changes: failed to reconstruct SyncOp for server_seq={server_seq} entity_id={entity_id} event_type={entity_type}"
            )));
        };

        changes.push(Change { op, changed_at, hlc: Hlc { pt: hlc_pt.max(0) as u64, l: hlc_l.max(0) as u64 }, local_seq: Some(client_seq.max(0) as u64), server_seq: Some(server_seq.max(0) as u64) });
    }

    Ok(changes)
}

/// Set of content hashes still referenced by some `book_dir_added` event whose
/// (book_id, dir_id, file_name) tuple has not been superseded by a later
/// `book_dir_removed`.
pub async fn live_blob_hashes(pool: &PgPool) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let rows = sqlx::query_scalar::<_, Option<String>>(
        "WITH latest AS (
             SELECT DISTINCT ON (library_id, entity_id, data->>'dir_id', data->>'file_name')
                 event_type,
                 data->>'content_hash' AS content_hash
             FROM library_events
             WHERE event_type IN ('book_dir_added', 'book_dir_removed')
             ORDER BY library_id, entity_id, data->>'dir_id', data->>'file_name', server_seq DESC
         )
         SELECT DISTINCT content_hash
         FROM latest
         WHERE event_type = 'book_dir_added' AND content_hash IS NOT NULL AND content_hash <> ''",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().flatten().collect())
}

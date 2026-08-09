use server_core::LibraryUpsertOutcome;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{ClientId, ContentHash, LibraryId, LibraryRevision, MutationId, MutationRejection, MutationRejectionReason, ReplicaSeq, ServerMutation, UnixMillis, WireMutation, MAX_PULL_CHANGES};

const FETCH_PAGE_SQL: &str = include_str!("sql/sync/fetch_page.sql");
const FETCH_READING_POSITION_PAGE_SQL: &str = include_str!("sql/sync/fetch_reading_position_page.sql");

pub(super) async fn list_libraries(pool: &PgPool, user_id: &str) -> Result<Vec<LibrarySummary>, sqlx::Error> {
    let rows = sqlx::query_file!("src/sql/sync/list_libraries.sql", user_id).fetch_all(pool).await?;
    rows.into_iter().map(|row| Ok(LibrarySummary { library_id: LibraryId::parse(&row.id).map_err(decode_err)?, library_name: row.name })).collect()
}

pub(super) async fn upsert_library(pool: &PgPool, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, sqlx::Error> {
    let library_id = request.library_id.as_str();
    let row = sqlx::query_file!("src/sql/sync/upsert_library.sql", library_id, user_id, request.library_name).fetch_optional(pool).await?;
    row.map(|row| Ok(LibraryUpsertOutcome { library: LibrarySummary { library_id: LibraryId::parse(&row.id.unwrap_or_default()).map_err(decode_err)?, library_name: row.name.unwrap_or_default() }, changed: row.changed.unwrap_or(false) }))
        .transpose()
}

pub(super) async fn ensure_library(pool: &PgPool, user_id: &str, library_id: &LibraryId) -> Result<bool, sqlx::Error> {
    let library_id = library_id.as_str();
    if let Some(owner) = sqlx::query_file_scalar!("src/sql/sync/get_library_owner.sql", library_id).fetch_optional(pool).await? {
        return Ok(owner == user_id);
    }
    let owner = sqlx::query_file_scalar!("src/sql/sync/ensure_library.sql", library_id, user_id).fetch_one(pool).await?;
    Ok(owner == user_id)
}

pub(super) async fn owns_library(pool: &PgPool, user_id: &str, library_id: &LibraryId) -> Result<bool, sqlx::Error> {
    let library_id = library_id.as_str();
    Ok(sqlx::query_file_scalar!("src/sql/sync/owns_library.sql", library_id, user_id).fetch_one(pool).await?.unwrap_or(false))
}

pub(super) struct InsertChangesResult {
    pub accepted: Vec<MutationId>,
    pub rejected: Vec<MutationRejection>,
}

pub(super) fn server_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or(0)
}

pub(super) fn storage_rejection(change: &WireMutation, server_now_ms: u64) -> Option<MutationRejectionReason> {
    (change.changed_at.get() > server_now_ms.saturating_add(sync_common::MAX_LWW_FUTURE_SKEW_MS)).then_some(MutationRejectionReason::TimestampTooFarFuture)
}

pub(super) fn supports_atomic_singleton(change: &WireMutation) -> bool {
    change.kind == "reading_position"
}

/// Stores opaque wire rows. The only routing knowledge is that reading state
/// belongs to the hot table; cold kinds need no server-side enum arm.
pub(super) async fn insert_changes(tx: &mut Transaction<'_, Postgres>, _user_id: &str, library_id: &LibraryId, client_id: &ClientId, changes: &[WireMutation]) -> Result<InsertChangesResult, sqlx::Error> {
    let now = server_now_ms();
    let (accepted, rejected): (Vec<_>, Vec<_>) = changes.iter().partition(|change| storage_rejection(change, now).is_none());
    let rejected = rejected.into_iter().map(|change| MutationRejection { mutation_id: change.mutation_id, reason: storage_rejection(change, now).expect("partitioned rejection") }).collect::<Vec<_>>();

    // The server reads only declared lifecycle facts. It never decodes the
    // opaque value to maintain quota/GC references.
    let mut lifecycle_deltas = Vec::new();
    for change in accepted.iter().copied().filter(|change| change.kind == "book_lifecycle") {
        let existing = sqlx::query(include_str!("sql/sync/get_lifecycle_reference_state.sql")).bind(library_id.as_str()).bind(&change.entity_key).fetch_optional(&mut **tx).await?;
        let incoming = (change.changed_at.storage_i64(), change.conflict_rank as i16, client_id.as_str().to_owned(), change.replica_seq.get() as i64, change.mutation_id.to_string());
        let wins = existing.as_ref().is_none_or(|row| {
            incoming
                > (
                    row.try_get::<i64, _>("changed_at").unwrap_or_default(),
                    row.try_get::<i16, _>("version_rank").unwrap_or_default(),
                    row.try_get::<String, _>("client_id").unwrap_or_default(),
                    row.try_get::<i64, _>("client_seq").unwrap_or_default(),
                    row.try_get::<String, _>("event_id").unwrap_or_default(),
                )
        });
        if !wins {
            continue;
        }
        if existing.as_ref().is_some_and(|row| row.try_get::<bool, _>("present").unwrap_or(false)) {
            if let Some(hash) = existing.as_ref().and_then(|row| row.try_get::<Option<String>, _>("content_hash").ok().flatten()).and_then(|hash| ContentHash::parse(&hash).ok()) {
                lifecycle_deltas.push((hash, -1));
            }
        }
        if let Some(hash) = change.blob_reference.as_ref().filter(|reference| reference.present).and_then(|reference| reference.content_hash.clone()) {
            lifecycle_deltas.push((hash, 1));
        }
    }
    crate::assets::apply_lifecycle_reference_deltas(tx, _user_id, lifecycle_deltas).await?;

    for (table, rows) in [
        ("sync_state", accepted.iter().copied().filter(|change| change.kind != "reading_position").collect::<Vec<_>>()),
        ("sync_reading_state", accepted.iter().copied().filter(|change| change.kind == "reading_position").collect::<Vec<_>>()),
    ] {
        if rows.is_empty() {
            continue;
        }
        let kinds = rows.iter().map(|change| change.kind.clone()).collect::<Vec<_>>();
        let keys = rows.iter().map(|change| change.entity_key.clone()).collect::<Vec<_>>();
        let subkeys = rows.iter().map(|change| change.entity_subkey.clone()).collect::<Vec<_>>();
        let values = rows.iter().map(|change| change.value.clone()).collect::<Vec<_>>();
        let present = rows.iter().map(|change| change.blob_reference.as_ref().map(|reference| reference.present)).collect::<Vec<_>>();
        let hashes = rows.iter().map(|change| change.blob_reference.as_ref().and_then(|reference| reference.content_hash.as_ref()).map(|hash| hash.as_str().to_owned())).collect::<Vec<_>>();
        let changed = rows.iter().map(|change| change.changed_at.storage_i64()).collect::<Vec<_>>();
        let sequences = rows.iter().map(|change| change.replica_seq.get() as i64).collect::<Vec<_>>();
        let ranks = rows.iter().map(|change| change.conflict_rank as i16).collect::<Vec<_>>();
        let events = rows.iter().map(|change| change.mutation_id.to_string()).collect::<Vec<_>>();
        let statement = include_str!("sql/sync/upsert_state_batch.sql").replace("{{TABLE}}", table);
        sqlx::query(&statement)
            .bind(library_id.as_str())
            .bind(kinds)
            .bind(keys)
            .bind(subkeys)
            .bind(values)
            .bind(present)
            .bind(hashes)
            .bind(changed)
            .bind(sequences)
            .bind(ranks)
            .bind(events)
            .bind(client_id.as_str())
            .execute(&mut **tx)
            .await?;
    }
    Ok(InsertChangesResult { accepted: accepted.into_iter().map(|change| change.mutation_id).collect(), rejected })
}

#[derive(Debug, thiserror::Error)]
pub(super) enum FetchChangesError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("client cursor is ahead of server state")]
    CursorAhead,
    #[error("corrupt stored state at revision {revision}: {reason}")]
    CorruptStoredEvent { revision: i64, reason: &'static str },
}
pub(super) struct FetchChangesPage {
    pub changes: Vec<ServerMutation>,
    pub has_more: bool,
}
pub(super) enum FetchChangesResult {
    Unauthorized,
    Page(FetchChangesPage),
}

fn decode_err<E: std::error::Error + Send + Sync + 'static>(error: E) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(error))
}

fn millis(value: i64) -> Result<UnixMillis, sqlx::Error> {
    let value = u64::try_from(value).map_err(|_| sqlx::Error::Decode("negative Unix-millisecond timestamp".into()))?;
    UnixMillis::new(value).map_err(decode_err)
}

fn cursor(value: Option<LibraryRevision>) -> i64 {
    value.map(|revision| i64::try_from(revision.get()).unwrap_or(i64::MAX)).unwrap_or(0)
}

fn cursor_is_ahead(value: Option<LibraryRevision>, current_revision: i64) -> bool {
    cursor(value) > current_revision
}
async fn fetch_projected_page<'e, E: sqlx::Executor<'e, Database = Postgres>>(executor: E, statement: &str, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    let rows = sqlx::query(statement).bind(library.as_str()).bind(cursor(since)).bind((MAX_PULL_CHANGES + 1) as i64).fetch_all(executor).await?;
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let raw: i64 = row.try_get("server_seq")?;
        let corrupt = |reason| FetchChangesError::CorruptStoredEvent { revision: raw, reason };
        let changed_at = millis(row.try_get::<i64, _>("changed_at")?).map_err(|_| corrupt("invalid timestamp"))?;
        let replica_id = ClientId::parse(&row.try_get::<String, _>("client_id")?).map_err(|_| corrupt("invalid replica"))?;
        let replica_seq = ReplicaSeq::new(u64::try_from(row.try_get::<i64, _>("client_seq")?).map_err(|_| corrupt("negative replica sequence"))?).map_err(|_| corrupt("invalid replica sequence"))?;
        let revision = LibraryRevision::new(u64::try_from(raw).map_err(|_| corrupt("negative library revision"))?).map_err(|_| corrupt("invalid library revision"))?;
        let mutation_id = MutationId::parse(&row.try_get::<String, _>("event_id")?).map_err(|_| corrupt("invalid mutation identity"))?;
        let present: Option<bool> = row.try_get("present")?;
        let content_hash: Option<String> = row.try_get("content_hash")?;
        let blob_reference = match (present, content_hash) {
            (Some(present), content_hash) => Some(sync_common::DeclaredBlobReference { present, content_hash: content_hash.map(|hash| ContentHash::parse(&hash)).transpose().map_err(|_| corrupt("invalid content hash"))? }),
            (None, None) => None,
            (None, Some(_)) => return Err(corrupt("hash declaration without presence")),
        };
        let mutation = WireMutation {
            mutation_id,
            kind: row.try_get("kind")?,
            entity_key: row.try_get("entity_key")?,
            entity_subkey: row.try_get("entity_subkey")?,
            value: row.try_get("value")?,
            conflict_rank: u8::try_from(row.try_get::<i16, _>("version_rank")?).map_err(|_| corrupt("invalid conflict rank"))?,
            blob_reference,
            changed_at,
            replica_seq,
        };
        changes.push(ServerMutation { mutation, replica_id, revision });
    }
    let has_more = changes.len() > MAX_PULL_CHANGES;
    changes.truncate(MAX_PULL_CHANGES);
    Ok(FetchChangesPage { changes, has_more })
}

async fn fetch_page<'e, E: sqlx::Executor<'e, Database = Postgres>>(executor: E, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    fetch_projected_page(executor, FETCH_PAGE_SQL, library, since).await
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PullRoute {
    Empty,
    ReadingOnly,
    Full,
}

fn pull_route(since: Option<LibraryRevision>, reading_revision: i64, other_revision: i64) -> PullRoute {
    let since = cursor(since);
    if reading_revision <= since && other_revision <= since {
        PullRoute::Empty
    } else if other_revision <= since {
        PullRoute::ReadingOnly
    } else {
        PullRoute::Full
    }
}

async fn fetch_reading_position_page(pool: &PgPool, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    fetch_projected_page(pool, FETCH_READING_POSITION_PAGE_SQL, library, since).await
}

pub(super) async fn fetch_changes(pool: &PgPool, user: &str, library: &LibraryId, _client: &ClientId, since: Option<LibraryRevision>) -> Result<FetchChangesResult, FetchChangesError> {
    let library_str = library.as_str();
    let heads = sqlx::query_file!("src/sql/sync/get_pull_heads.sql", library_str, user).fetch_optional(pool).await?;
    let Some(heads) = heads else { return Ok(FetchChangesResult::Unauthorized) };
    let reading_revision: i64 = heads.reading_revision.unwrap_or(0);
    let other_revision: i64 = heads.other_revision.unwrap_or(0);
    if cursor_is_ahead(since, std::cmp::max(reading_revision, other_revision)) {
        return Err(FetchChangesError::CursorAhead);
    }
    let page = match pull_route(since, reading_revision, other_revision) {
        PullRoute::Empty => FetchChangesPage { changes: Vec::new(), has_more: false },
        PullRoute::ReadingOnly => fetch_reading_position_page(pool, library, since).await?,
        PullRoute::Full => fetch_page(pool, library, since).await?,
    };
    Ok(FetchChangesResult::Page(page))
}

pub(super) async fn validate_owned_cursor(tx: &mut Transaction<'_, Postgres>, library: &LibraryId, since: Option<LibraryRevision>) -> Result<(), FetchChangesError> {
    let library_str = library.as_str();
    let current: i64 = sqlx::query_file_scalar!("src/sql/sync/validate_owned_cursor.sql", library_str).fetch_one(&mut **tx).await?.unwrap_or(0);
    if cursor_is_ahead(since, current) {
        return Err(FetchChangesError::CursorAhead);
    }
    Ok(())
}

pub(super) async fn fetch_owned_changes(tx: &mut Transaction<'_, Postgres>, library: &LibraryId, since: Option<LibraryRevision>) -> Result<FetchChangesPage, FetchChangesError> {
    fetch_page(&mut **tx, library, since).await
}

pub(super) enum AtomicSingletonExchange {
    Unauthorized,
    CursorAhead,
    Handled(FetchChangesPage),
}
pub(super) async fn exchange_authorized_singleton(pool: &PgPool, user_id: &str, library: &LibraryId, client: &ClientId, change: &WireMutation, since: Option<LibraryRevision>) -> Result<AtomicSingletonExchange, FetchChangesError> {
    if change.kind != "reading_position" {
        unreachable!()
    }
    let library_str = library.as_str();
    let since_cursor = cursor(since);
    let row = sqlx::query(include_str!("sql/sync/exchange_authorized_reading_position.sql"))
        .bind(library_str)
        .bind(user_id)
        .bind(&change.kind)
        .bind(&change.entity_key)
        .bind(&change.entity_subkey)
        .bind(&change.value)
        .bind(change.blob_reference.as_ref().map(|reference| reference.present))
        .bind(change.blob_reference.as_ref().and_then(|reference| reference.content_hash.as_ref()).map(|hash| hash.as_str()))
        .bind(change.changed_at.storage_i64())
        .bind(client.as_str())
        .bind(change.replica_seq.get() as i64)
        .bind(change.conflict_rank as i16)
        .bind(change.mutation_id.to_string())
        .bind(since_cursor)
        .fetch_one(pool)
        .await?;
    if !row.try_get::<Option<bool>, _>("authorized")?.unwrap_or(false) {
        return Ok(AtomicSingletonExchange::Unauthorized);
    }
    if row.try_get::<Option<bool>, _>("cursor_ahead")?.unwrap_or(false) {
        return Ok(AtomicSingletonExchange::CursorAhead);
    }

    let changed_revision = row.try_get::<Option<i64>, _>("server_seq")?;
    if row.try_get::<Option<bool>, _>("caught_up")?.unwrap_or(false) {
        let changes = if let Some(raw) = changed_revision {
            vec![ServerMutation {
                mutation: change.clone(),
                replica_id: ClientId::parse(client.as_str()).map_err(|_| FetchChangesError::CorruptStoredEvent { revision: raw, reason: "invalid replica" })?,
                revision: LibraryRevision::new(raw as u64).map_err(|_| FetchChangesError::CorruptStoredEvent { revision: raw, reason: "invalid revision" })?,
            }]
        } else {
            Vec::new()
        };
        return Ok(AtomicSingletonExchange::Handled(FetchChangesPage { changes, has_more: false }));
    }

    let prior_reading = row.try_get::<Option<i64>, _>("reading_revision")?.unwrap_or(0);
    let prior_other = row.try_get::<Option<i64>, _>("other_revision")?.unwrap_or(0);
    let reading_after_write = changed_revision.map_or(prior_reading, |revision| std::cmp::max(prior_reading, revision));
    let page = match pull_route(since, reading_after_write, prior_other) {
        PullRoute::Empty => FetchChangesPage { changes: Vec::new(), has_more: false },
        PullRoute::ReadingOnly => fetch_reading_position_page(pool, library, since).await?,
        PullRoute::Full => fetch_page(pool, library, since).await?,
    };
    Ok(AtomicSingletonExchange::Handled(page))
}

#[cfg(test)]
mod tests {
    use super::{cursor_is_ahead, pull_route, storage_rejection, PullRoute};
    use sync_common::{ClientId, LibraryRevision, MutationId, MutationRejectionReason, ReplicaSeq, UnixMillis, WireMutation, MAX_LWW_FUTURE_SKEW_MS};

    fn reading_mutation(changed_at: u64) -> WireMutation {
        WireMutation {
            mutation_id: MutationId::new(),
            kind: "reading_position".to_owned(),
            entity_key: "1".to_owned(),
            entity_subkey: String::new(),
            value: vec![1, 2, 3],
            conflict_rank: 0,
            blob_reference: None,
            changed_at: UnixMillis::new(changed_at).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        }
    }

    #[test]
    fn pull_routing_preserves_the_global_cursor_boundary() {
        let cursor = LibraryRevision::new(10).ok();
        assert_eq!(pull_route(cursor, 10, 10), PullRoute::Empty);
        assert_eq!(pull_route(cursor, 11, 10), PullRoute::ReadingOnly);
        assert_eq!(pull_route(cursor, 10, 11), PullRoute::Full);
        assert_eq!(pull_route(cursor, 12, 11), PullRoute::Full);
        assert_eq!(pull_route(None, 1, 0), PullRoute::ReadingOnly);
        assert_eq!(pull_route(None, 0, 1), PullRoute::Full);
    }

    #[test]
    fn a_cursor_may_equal_but_never_exceed_the_server_head() {
        assert!(!cursor_is_ahead(None, 0));
        assert!(!cursor_is_ahead(LibraryRevision::new(10).ok(), 10));
        assert!(cursor_is_ahead(LibraryRevision::new(11).ok(), 10));
    }

    #[test]
    fn server_admission_rejects_future_wall_clock() {
        let server_now = 1_000_000;
        assert_eq!(storage_rejection(&reading_mutation(server_now + MAX_LWW_FUTURE_SKEW_MS + 1), server_now), Some(MutationRejectionReason::TimestampTooFarFuture),);
        assert_eq!(storage_rejection(&reading_mutation(server_now), server_now), None);
    }

    #[test]
    fn postgres_winner_order_begins_with_wall_clock_time() {
        const LWW_TRIGGER_SQL: &str = include_str!("../../server-database/schema/03_sync.sql");
        let comparison = LWW_TRIGGER_SQL.split("IF (").nth(1).expect("LWW predicate");
        assert!(comparison.trim_start().starts_with("NEW.changed_at,"));
        assert!(!LWW_TRIGGER_SQL.contains("hlc_"));
    }
}

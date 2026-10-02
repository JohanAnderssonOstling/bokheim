use std::collections::HashSet;
use crate::sync::LibraryNameOutcome;
use sqlx::{PgConnection, PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::StateAddress;
use sync_common::api::libraries::{LibraryNameRequest, LibrarySummary};
use sync_common::{ConflictRank, ContentHash, LibraryId, LibraryRevision, MAX_PULL_CHANGES, MutationRejection, MutationRejectionReason, ReplicaId, ReplicaSeq, StateDelivery, SyncCursor, UnixMillis, VersionKey, OpaquePublication};

const FETCH_LIFECYCLE_PAGE_SQL: &str = include_str!("sql/sync/fetch_lifecycle_page.sql");
const FETCH_FIELD_PAGE_SQL: &str = include_str!("sql/sync/fetch_field_page.sql");

pub(super) async fn list_libraries(pool: &PgPool, user_id: &str) -> Result<Vec<LibrarySummary>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String)>(include_str!("sql/sync/list_libraries.sql")).bind(user_id).fetch_all(pool).await?;
    rows.into_iter().map(|(id, name)| Ok(LibrarySummary { library_id: LibraryId::parse_str(&id).map_err(decode_err)?, library_name: name })).collect()
}

pub(super) async fn create_library(pool: &PgPool, user_id: &str, request: &LibraryNameRequest) -> Result<Option<LibraryNameOutcome>, sqlx::Error> {
    let library_id = request.library_id.to_string();
    let row = sqlx::query_as::<_, (String, String, bool)>(include_str!("sql/sync/create_library.sql")).bind(library_id).bind(user_id).bind(&request.library_name).fetch_optional(pool).await?;
    row.map(|(id, name, changed)| Ok(LibraryNameOutcome { library: LibrarySummary { library_id: LibraryId::parse_str(&id).map_err(decode_err)?, library_name: name }, changed })).transpose()
}

pub(super) async fn rename_library(pool: &PgPool, user_id: &str, request: &LibraryNameRequest) -> Result<Option<LibraryNameOutcome>, sqlx::Error> {
    let library_id = request.library_id.to_string();
    let row = sqlx::query_as::<_, (String, String, bool)>(include_str!("sql/sync/rename_library.sql")).bind(library_id).bind(user_id).bind(&request.library_name).fetch_optional(pool).await?;
    row.map(|(id, name, changed)| Ok(LibraryNameOutcome { library: LibrarySummary { library_id: LibraryId::parse_str(&id).map_err(decode_err)?, library_name: name }, changed })).transpose()
}

pub(super) async fn owns_library(pool: &PgPool, user_id: &str, library_id: &LibraryId) -> Result<bool, sqlx::Error> {
    let library_id = library_id.to_string();
    Ok(sqlx::query_scalar::<_, bool>(include_str!("sql/sync/owns_library.sql")).bind(library_id).bind(user_id).fetch_one(pool).await?)
}

pub(super) struct InsertChangesResult {
    pub accepted: Vec<ReplicaSeq>,
    pub rejected: Vec<MutationRejection>,
}

pub(super) fn server_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or(0)
}

pub(super) fn storage_rejection(change: &OpaquePublication, server_now_ms: u64) -> Option<MutationRejectionReason> {
    if change.state.version.changed_at > server_now_ms.saturating_add(sync_common::MAX_LWW_FUTURE_SKEW_MS) {
        return Some(MutationRejectionReason::TimestampTooFarFuture);
    }
    // The quota ledger charges a lifecycle's book, so it must be named and be
    // the book's own.
    if change.state.value.address.kind == "book_lifecycle" {
        let identity = change.state.value.address.entity_key.parse::<ContentHash>().ok();
        match change.state.value.book {
            None => return Some(MutationRejectionReason::InvalidPayload),
            Some(book) if identity.is_some_and(|identity| identity != book) => return Some(MutationRejectionReason::InvalidPayload),
            Some(_) => {}
        }
    }
    None
}

#[derive(Debug)]
struct StoredLifecycleReference {
    content_hash: Option<String>,
    changed_at: i64,
    version_rank: i16,
    replica_id: String,
    replica_seq: i64,
}

fn decode_lifecycle_reference(row: PgRow) -> Result<StoredLifecycleReference, sqlx::Error> {
    Ok(StoredLifecycleReference {
        content_hash: row.try_get("content_hash").map_err(decode_err)?,
        changed_at: row.try_get("changed_at").map_err(decode_err)?,
        version_rank: row.try_get("version_rank").map_err(decode_err)?,
        replica_id: row.try_get("replica_id").map_err(decode_err)?,
        replica_seq: row.try_get("replica_seq").map_err(decode_err)?,
    })
}

/// All writes share the library lock held by the caller. Resolve lifecycle
/// first, remove retired fields, then admit ordinary updates against that final
/// existence state. Ignored updates are acknowledged so offline clients drain.
pub(super) async fn insert_changes(tx: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &LibraryId, replica_id: &ReplicaId, changes: &[OpaquePublication]) -> Result<InsertChangesResult, sqlx::Error> {
    let lifecycles = changes.iter().filter(|c| c.state.value.address.kind == "book_lifecycle").cloned().collect::<Vec<_>>();
    let mut result = insert_admitted_changes(tx, user_id, library_id, replica_id, &lifecycles).await?;
    // Purge removes every field owned by the book.
    // A stale purge does nothing because these queries inspect the LWW winner.
    sqlx::query(
        "DELETE FROM sync_state field USING sync_state lifecycle
         WHERE lifecycle.library_id=$1 AND lifecycle.kind='book_lifecycle' AND lifecycle.entity_subkey=''
           AND lifecycle.entity_key=ANY($2) AND lifecycle.version_rank=2
           AND field.library_id=lifecycle.library_id AND field.kind<>'book_lifecycle' AND field.content_hash=lifecycle.entity_key"
    )
    .bind(library_id.to_string())
    .bind(lifecycles.iter().map(|c| c.state.value.address.entity_key.clone()).collect::<Vec<_>>())
    .execute(&mut **tx)
    .await?;
    // A losing purge has already removed the sender's entire local book.
    // Redeliver all surviving fields, even those behind its cursor. Preserve
    // the canonical value/version; only delivery revisions advance. The caller
    // still holds the library lock, including the field update.
    let purged = lifecycles.iter().filter(|c| c.state.version.conflict_rank == ConflictRank::Purge && result.accepted.contains(&c.publisher_seq)).map(|c| c.state.value.address.entity_key.clone()).collect::<Vec<_>>();
    if !purged.is_empty() {
        sqlx::query(
            "UPDATE sync_state field SET server_seq=nextval('sync_server_sequence')
             FROM sync_state lifecycle
             WHERE lifecycle.library_id=$1 AND lifecycle.kind='book_lifecycle' AND lifecycle.entity_subkey=''
               AND lifecycle.entity_key=ANY($2) AND lifecycle.version_rank<>2
               AND field.library_id=lifecycle.library_id AND field.kind<>'book_lifecycle' AND field.content_hash=lifecycle.entity_key"
        )
        .bind(library_id.to_string())
        .bind(&purged)
        .execute(&mut **tx)
        .await?;
    }
    let mut fields = Vec::new();
    let owners = changes.iter().filter_map(|c| c.state.value.book.map(|book| book.to_string())).collect::<Vec<_>>();
    let existing: HashSet<String> = sqlx::query_scalar::<_, String>("SELECT entity_key FROM sync_state WHERE library_id=$1 AND kind='book_lifecycle' AND entity_subkey='' AND version_rank<>2 AND entity_key=ANY($2)")
        .bind(library_id.to_string())
        .bind(owners)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .collect();
    for change in changes.iter().filter(|c| c.state.value.address.kind != "book_lifecycle") {
        if let Some(reason) = storage_rejection(change, server_now_ms()) {
            result.rejected.push(MutationRejection { publisher_seq: change.publisher_seq, reason });
            continue;
        }
        let owner = change.state.value.book.as_ref().map(ContentHash::as_str);
        if owner.is_some_and(|hash| !existing.contains(hash)) {
            let failed_creation = lifecycles.iter().filter(|c| Some(c.state.value.address.entity_key.as_str()) == owner).find_map(|c| result.rejected.iter().find(|r| r.publisher_seq == c.publisher_seq).map(|r| r.reason.clone()));
            if let Some(reason) = failed_creation {
                result.rejected.push(MutationRejection { publisher_seq: change.publisher_seq, reason });
            } else {
                result.accepted.push(change.publisher_seq);
            }
        } else {
            fields.push(change.clone());
        }
    }
    let inserted = insert_admitted_changes(tx, user_id, library_id, replica_id, &fields).await?;
    result.accepted.extend(inserted.accepted);
    result.rejected.extend(inserted.rejected);
    let accepted = result.accepted.into_iter().collect::<HashSet<_>>();
    result.accepted = changes.iter().filter(|c| accepted.contains(&c.publisher_seq)).map(|c| c.publisher_seq).collect();
    Ok(result)
}


/// Stores every synchronized kind as an opaque row in the shared table.
async fn insert_admitted_changes(tx: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &LibraryId, _replica_id: &ReplicaId, changes: &[OpaquePublication]) -> Result<InsertChangesResult, sqlx::Error> {
    if changes.is_empty() {
        return Ok(InsertChangesResult { accepted: Vec::new(), rejected: Vec::new() });
    }
    let now = server_now_ms();
    let (accepted, rejected): (Vec<_>, Vec<_>) = changes.iter().partition(|change| storage_rejection(change, now).is_none());
    let rejected = rejected.into_iter().map(|change| MutationRejection { publisher_seq: change.publisher_seq, reason: storage_rejection(change, now).expect("partitioned rejection") }).collect::<Vec<_>>();

    // The server reads only declared lifecycle facts. It never decodes the
    // opaque value to maintain quota references.
    let mut lifecycle_deltas = Vec::new();
    let mut lifecycle_winners = HashMap::<(&str, &str, &str), &OpaquePublication>::new();
    for change in accepted.iter().copied().filter(|change| change.state.value.address.kind == "book_lifecycle") {
        let cell = (change.state.value.address.kind.as_str(), change.state.value.address.entity_key.as_str(), change.state.value.address.entity_subkey.as_str());
        lifecycle_winners
            .entry(cell)
            .and_modify(|winner| {
                if (change).state.version.clone() > (winner).state.version.clone() {
                    *winner = change;
                }
            })
            .or_insert(change);
    }
    for change in lifecycle_winners.into_values() {
        let existing = sqlx::query(include_str!("sql/sync/get_lifecycle_reference_state.sql"))
            .bind(library_id.to_string())
            .bind(&change.state.value.address.entity_key)
            .bind(&change.state.value.address.entity_subkey)
            .fetch_optional(&mut **tx)
            .await?
            .map(decode_lifecycle_reference)
            .transpose()?;
        let incoming = (change).state.version.clone();
        let wins = match existing.as_ref() {
            None => true,
            Some(stored) => {
                let stored_replica = ReplicaId::parse_str(&stored.replica_id).map_err(decode_err)?;
                let stored_changed_at = u64::try_from(stored.changed_at).map_err(decode_err)?;
                let stored_rank = ConflictRank::try_from(u8::try_from(stored.version_rank).map_err(decode_err)?).map_err(decode_err)?;
                let stored_seq = ReplicaSeq::new(u64::try_from(stored.replica_seq).map_err(decode_err)?).map_err(decode_err)?;
                incoming > VersionKey::new(stored_changed_at, stored_rank, stored_replica, stored_seq)
            }
        };
        if !wins {
            continue;
        }
        // A lifecycle holds its book's bytes unless it is a purge.
        let held = change.state.value.book.filter(|_| change.state.version.conflict_rank != ConflictRank::Purge);
        if let Some(hash) = existing.as_ref().filter(|stored| stored.version_rank != i16::from(u8::from(ConflictRank::Purge))).and_then(|stored| stored.content_hash.as_ref()) {
            lifecycle_deltas.push((ContentHash::new(hash), -1));
            if !held.is_some_and(|next| next.as_str() == hash) {
                sqlx::query(include_str!("sql/cloud_storage/delete_claim.sql")).bind(library_id.to_string()).bind(hash).execute(&mut **tx).await?;
            }
        }
        if let Some(hash) = held {
            lifecycle_deltas.push((hash, 1));
        }
    }
    let cloud_enabled: bool = sqlx::query_scalar(include_str!("sql/cloud_storage/enabled.sql")).bind(library_id.to_string()).fetch_one(&mut **tx).await?;
    if cloud_enabled {
        crate::assets::apply_lifecycle_reference_deltas(tx, user_id, lifecycle_deltas).await?;
    }

    {
        let rows = &accepted;
        if rows.is_empty() {
            return Ok(InsertChangesResult { accepted: Vec::new(), rejected });
        }
        let kinds = rows.iter().map(|change| change.state.value.address.kind.clone()).collect::<Vec<_>>();
        let keys = rows.iter().map(|change| change.state.value.address.entity_key.clone()).collect::<Vec<_>>();
        let subkeys = rows.iter().map(|change| change.state.value.address.entity_subkey.clone()).collect::<Vec<_>>();
        let values = rows.iter().map(|change| change.state.value.bytes.clone()).collect::<Vec<_>>();
        let books = rows.iter().map(|change| change.state.value.book.map(|book| book.to_string())).collect::<Vec<_>>();
        let changed = rows.iter().map(|change| i64::try_from(change.state.version.changed_at)).collect::<Result<Vec<_>, _>>().map_err(decode_err)?;
        let versions = rows.iter().map(|change| (change).state.version.clone()).collect::<Vec<_>>();
        let sequences = versions.iter().map(|version| i64::try_from(version.replica_seq.get())).collect::<Result<Vec<_>, _>>().map_err(decode_err)?;
        let ranks = rows.iter().map(|change| i16::from(u8::from(change.state.version.conflict_rank))).collect::<Vec<_>>();
        let replicas = versions.iter().map(|version| version.replica_id.to_string()).collect::<Vec<_>>();
        let statement = include_str!("sql/sync/upsert_state_batch.sql");
        sqlx::query(statement).bind(library_id.to_string()).bind(kinds).bind(keys).bind(subkeys).bind(values).bind(books).bind(changed).bind(sequences).bind(ranks).bind(replicas).execute(&mut **tx).await?;
    }
    Ok(InsertChangesResult { accepted: accepted.into_iter().map(|change| change.publisher_seq).collect(), rejected })
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
    pub changes: Vec<StateDelivery>,
    pub next_cursor: SyncCursor,
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
    u64::try_from(value).map_err(|_| sqlx::Error::Decode("negative Unix-millisecond timestamp".into()))
}

fn revision(value: Option<LibraryRevision>) -> i64 {
    value.map(|revision| i64::try_from(revision.get()).unwrap_or(i64::MAX)).unwrap_or(0)
}

/// A cursor is valid at or below the committed head, with its field component
/// never past its lifecycle component.
fn cursor_is_ahead(cursor: SyncCursor, head: i64) -> bool {
    revision(cursor.lifecycle_revision) > head || revision(cursor.field_revision) > revision(cursor.lifecycle_revision)
}

fn protocol_revision(value: i64) -> Result<Option<LibraryRevision>, FetchChangesError> {
    if value == 0 {
        Ok(None)
    } else {
        LibraryRevision::new(u64::try_from(value).map_err(|_| FetchChangesError::CorruptStoredEvent { revision: value, reason: "negative channel revision" })?)
            .map(Some)
            .map_err(|_| FetchChangesError::CorruptStoredEvent { revision: value, reason: "invalid channel revision" })
    }
}

fn decode_change(row: &PgRow) -> Result<StateDelivery, FetchChangesError> {
    let raw: i64 = row.try_get("server_seq")?;
    let corrupt = |reason| FetchChangesError::CorruptStoredEvent { revision: raw, reason };
    let changed_at = millis(row.try_get::<i64, _>("changed_at")?).map_err(|_| corrupt("invalid timestamp"))?;
    let replica_id = ReplicaId::parse_str(&row.try_get::<String, _>("replica_id")?).map_err(|_| corrupt("invalid replica identity"))?;
    let replica_seq = ReplicaSeq::new(u64::try_from(row.try_get::<i64, _>("replica_seq")?).map_err(|_| corrupt("negative replica sequence"))?).map_err(|_| corrupt("invalid replica sequence"))?;
    let revision = LibraryRevision::new(u64::try_from(raw).map_err(|_| corrupt("negative library revision"))?).map_err(|_| corrupt("invalid library revision"))?;
    let book = row.try_get::<Option<String>, _>("content_hash")?.map(|hash| hash.parse::<ContentHash>().map_err(|_| corrupt("invalid book hash"))).transpose()?;
    let conflict_rank = u8::try_from(row.try_get::<i16, _>("version_rank")?).ok().and_then(|rank| ConflictRank::try_from(rank).ok()).ok_or_else(|| corrupt("invalid conflict rank"))?;
    let version = VersionKey::new(changed_at, conflict_rank, replica_id, replica_seq);
    let value = sync_common::OpaqueStateEntry {
        address: StateAddress {
            kind: row.try_get("kind")?,
            entity_key: row.try_get("entity_key")?,
            entity_subkey: row.try_get("entity_subkey")?,
        },
        bytes: row.try_get("value")?, book,
    };
    Ok(StateDelivery { state: sync_common::Versioned { value, version }, revision })
}

/// Delivers every lifecycle after the lifecycle cursor before any field, and
/// fields only at or below the lifecycle cursor, so a replica always holds a
/// field's book before the field. Callers read `head` and both pages from one
/// consistent view: under the library lock, or in one snapshot.
async fn fetch_page(connection: &mut PgConnection, library: &LibraryId, since: SyncCursor, head: i64) -> Result<FetchChangesPage, FetchChangesError> {
    let mut changes = Vec::new();
    let mut next_cursor = since;
    let lifecycle_since = revision(since.lifecycle_revision);
    if lifecycle_since < head {
        let rows = sqlx::query(FETCH_LIFECYCLE_PAGE_SQL).bind(library.to_string()).bind(lifecycle_since).bind(head).bind(MAX_PULL_CHANGES as i64).fetch_all(&mut *connection).await?;
        changes = rows.iter().map(decode_change).collect::<Result<Vec<_>, _>>()?;
        if changes.len() == MAX_PULL_CHANGES {
            next_cursor.lifecycle_revision = changes.last().map(|change| change.revision);
            return Ok(FetchChangesPage { changes, next_cursor, has_more: true });
        }
    }
    next_cursor.lifecycle_revision = protocol_revision(head)?;
    let field_since = revision(since.field_revision);
    if field_since < head {
        let remaining = MAX_PULL_CHANGES - changes.len();
        let rows = sqlx::query(FETCH_FIELD_PAGE_SQL).bind(library.to_string()).bind(field_since).bind(head).bind(remaining as i64 + 1).fetch_all(&mut *connection).await?;
        let has_more = rows.len() > remaining;
        for row in rows.iter().take(remaining) {
            changes.push(decode_change(row)?);
        }
        if has_more {
            next_cursor.field_revision = changes.last().map(|change| change.revision);
            return Ok(FetchChangesPage { changes, next_cursor, has_more });
        }
    }
    next_cursor.field_revision = protocol_revision(head)?;
    Ok(FetchChangesPage { changes, next_cursor, has_more: false })
}

pub(super) async fn fetch_changes(pool: &PgPool, user: &str, library: &LibraryId, _replica_id: &ReplicaId, since: SyncCursor) -> Result<FetchChangesResult, FetchChangesError> {
    // Without the library lock, one snapshot keeps a concurrent lifecycle move
    // from hiding a book whose field this page still delivers.
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").execute(&mut *transaction).await?;
    let head = sqlx::query_scalar::<_, i64>(include_str!("sql/sync/get_pull_heads.sql")).bind(library.to_string()).bind(user).fetch_optional(&mut *transaction).await?;
    let Some(head) = head else { return Ok(FetchChangesResult::Unauthorized) };
    if cursor_is_ahead(since, head) {
        return Err(FetchChangesError::CursorAhead);
    }
    let page = fetch_page(&mut transaction, library, since, head).await?;
    transaction.commit().await?;
    Ok(FetchChangesResult::Page(page))
}

pub(super) async fn validate_owned_cursor(tx: &mut Transaction<'_, Postgres>, library: &LibraryId, since: SyncCursor) -> Result<(), FetchChangesError> {
    let head = sqlx::query_scalar(include_str!("sql/sync/validate_owned_cursor.sql")).bind(library.to_string()).fetch_one(&mut **tx).await?;
    if cursor_is_ahead(since, head) {
        return Err(FetchChangesError::CursorAhead);
    }
    Ok(())
}

pub(super) async fn fetch_owned_changes(tx: &mut Transaction<'_, Postgres>, library: &LibraryId, since: SyncCursor) -> Result<FetchChangesPage, FetchChangesError> {
    let head = sqlx::query_scalar(include_str!("sql/sync/validate_owned_cursor.sql")).bind(library.to_string()).fetch_one(&mut **tx).await?;
    fetch_page(tx, library, since, head).await
}

#[cfg(test)]
mod tests {
    use crate::test_support::isolated_pool;
    use super::{cursor_is_ahead, decode_lifecycle_reference, storage_rejection};
    use sync_common::{LibraryRevision, MAX_LWW_FUTURE_SKEW_MS, MutationRejectionReason, ReplicaSeq, SyncCursor, OpaquePublication};

    fn cursor(lifecycle: u64, field: u64) -> SyncCursor {
        SyncCursor { lifecycle_revision: LibraryRevision::new(lifecycle).ok(), field_revision: LibraryRevision::new(field).ok() }
    }

    fn reading_mutation(changed_at: u64) -> OpaquePublication {

        sync_common::Publication { publisher_seq: ReplicaSeq::new(1).unwrap(), state: sync_common::Versioned { version: sync_common::VersionKey::new(changed_at, sync_common::ConflictRank::Update, sync_common::ReplicaId::nil(), ReplicaSeq::new(1).unwrap()), value: sync_common::OpaqueStateEntry { address: sync_common::StateAddress { kind: "reading_position".to_owned(), entity_key: "1".to_owned(), entity_subkey: String::new() }, bytes: vec![1, 2, 3], book: None } } }
    }

    #[test]
    fn a_cursor_may_equal_but_never_exceed_the_server_head() {
        assert!(!cursor_is_ahead(SyncCursor::default(), 0));
        assert!(!cursor_is_ahead(cursor(10, 10), 10));
        assert!(!cursor_is_ahead(cursor(10, 4), 10));
        assert!(cursor_is_ahead(cursor(11, 10), 10));
        assert!(cursor_is_ahead(cursor(8, 9), 10), "fields cannot pass the lifecycle cursor");
    }

    #[test]
    fn server_admission_rejects_future_wall_clock() {
        let server_now = 1_000_000;
        assert_eq!(storage_rejection(&reading_mutation(server_now + MAX_LWW_FUTURE_SKEW_MS + 1), server_now), Some(MutationRejectionReason::TimestampTooFarFuture),);
        assert_eq!(storage_rejection(&reading_mutation(server_now), server_now), None);
    }

    /// The declared reference is the whole of what the service knows about a
    /// book's bytes, so a lifecycle mutation that omits it cannot be stored:
    /// its row would leave the quota ledger unable to say what the cell holds.
    /// Retrying cannot fix a malformed payload, so the rejection is permanent.
    #[test]
    fn server_admission_requires_a_lifecycle_to_name_its_own_book() {
        let server_now = 1_000_000;
        let mut lifecycle = reading_mutation(server_now);
        lifecycle.state.value.address.kind = "book_lifecycle".to_owned();
        lifecycle.state.value.address.entity_key = "a".repeat(64);

        assert_eq!(storage_rejection(&lifecycle, server_now), Some(MutationRejectionReason::InvalidPayload));
        assert!(!MutationRejectionReason::InvalidPayload.is_transient(), "a client cannot make a malformed payload valid by retrying it");

        lifecycle.state.value.book = Some(sync_common::ContentHash::new(&"b".repeat(64)));
        assert_eq!(storage_rejection(&lifecycle, server_now), Some(MutationRejectionReason::InvalidPayload), "the ledger must not charge another book's hash");
        lifecycle.state.value.book = Some(sync_common::ContentHash::new(&"a".repeat(64)));
        assert_eq!(storage_rejection(&lifecycle, server_now), None);

        // Every other kind is opaque and carries no accounting at all.
        assert_eq!(storage_rejection(&reading_mutation(server_now), server_now), None);
    }

    const LWW_TRIGGER_SQL: &str = include_str!("../schema/03_sync.sql");

    #[test]
    fn postgres_winner_order_matches_the_shared_version_key() {
        let comparison = LWW_TRIGGER_SQL.split("IF (").nth(1).expect("LWW predicate");
        let comparison = comparison.split(')').next().expect("LWW tuple").split_whitespace().collect::<String>();
        assert_eq!(comparison, "NEW.changed_at,NEW.version_rank,NEW.replica_idCOLLATE\"C\",NEW.replica_seq");
        assert!(!LWW_TRIGGER_SQL.contains("hlc_"));
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn malformed_lifecycle_reference_metadata_is_not_defaulted() {
        let Some(pool) = crate::test_support::isolated_pool("lifecycle_decode", 2).await else { return };
        let row = sqlx::query(
            "SELECT NULL::text AS content_hash,
                    'not-a-timestamp'::text AS changed_at, 0::smallint AS version_rank,
                    'replica'::text AS replica_id, 1::bigint AS replica_seq,
                    'event'::text AS event_id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(matches!(decode_lifecycle_reference(row), Err(sqlx::Error::Decode(_))), "invalid persisted winner metadata must abort accounting instead of becoming a default value");
    }
}

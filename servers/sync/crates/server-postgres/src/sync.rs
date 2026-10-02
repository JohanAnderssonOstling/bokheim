use std::time::Instant;
use crate::{PostgresDatabase, assets, sync_store};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;
use sync_common::api::libraries::{LibraryNameRequest, LibrarySummary};
use sync_common::{PullStateQuery, PullStateResponse, PushMutationsResponse, SyncExchangeRequest, SyncExchangeResponse};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("authenticated user does not match the request")]
    Forbidden,
    #[error("library belongs to a different user")]
    LibraryOwnershipConflict,
    #[error("invalid library name")]
    InvalidLibraryName,
    #[error("synchronization request contains too many mutations")]
    TooManyMutations,
    #[error("synchronization request contains an ambiguous mutation identity")]
    AmbiguousMutationIdentity,
    #[error("stored synchronization data requires a newer client")]
    UpgradeRequired,
    #[error("client synchronization cursor is ahead of server state")]
    CursorAhead,
    #[error("synchronization persistence failed: {0}")]
    Internal(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryNameOutcome {
    pub library: LibrarySummary,
    pub changed: bool,
}

#[derive(Clone)]
pub struct PostgresSyncRepository {
    pub(super) pool: PgPool,
}

impl PostgresSyncRepository {
    pub fn new(database: PostgresDatabase) -> Self {
        Self { pool: database.pool().clone() }
    }

    async fn authorize_owned_library(&self, transaction: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, SyncError> {
        let row = sqlx::query(include_str!("sql/sync/authorize_owned_library.sql")).bind(library_id.to_string()).bind(user_id).fetch_optional(&mut **transaction).await.map_err(internal)?;
        Ok(row.is_some())
    }
}

fn pull_response(page: sync_store::FetchChangesPage) -> PullStateResponse {
    PullStateResponse { mutations: page.changes, next_cursor: page.next_cursor, has_more: page.has_more }
}

fn internal(error: impl std::fmt::Display) -> SyncError {
    SyncError::Internal(error.to_string())
}

fn fetch_error(error: sync_store::FetchChangesError) -> SyncError {
    match error {
        sync_store::FetchChangesError::CursorAhead => SyncError::CursorAhead,
        other => internal(other),
    }
}

impl PostgresSyncRepository {
    pub async fn list_libraries(&self, user_id: &str) -> Result<Vec<LibrarySummary>, SyncError> {
        sync_store::list_libraries(&self.pool, user_id).await.map_err(internal)
    }

    pub async fn list_deleted_libraries(&self, user_id: &str) -> Result<Vec<sync_common::LibraryId>, SyncError> {
        let ids = sqlx::query_scalar::<_, String>("SELECT id FROM libraries WHERE user_id=$1 AND deleted_at IS NOT NULL ORDER BY deleted_at, id").bind(user_id).fetch_all(&self.pool).await.map_err(internal)?;
        ids.into_iter().map(|id| sync_common::LibraryId::parse_str(&id).map_err(internal)).collect()
    }

    pub async fn create_library(&self, user_id: &str, request: &LibraryNameRequest) -> Result<Option<LibraryNameOutcome>, SyncError> {
        let name = request.library_name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(SyncError::InvalidLibraryName);
        }
        sync_store::create_library(&self.pool, user_id, &LibraryNameRequest { library_name: name.to_owned(), ..request.clone() }).await.map_err(internal)
    }

    pub async fn rename_library(&self, user_id: &str, request: &LibraryNameRequest) -> Result<Option<LibraryNameOutcome>, SyncError> {
        let name = request.library_name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(SyncError::InvalidLibraryName);
        }
        sync_store::rename_library(&self.pool, user_id, &LibraryNameRequest { library_name: name.to_owned(), ..request.clone() }).await.map_err(internal)
    }

    /// Deletes server-side library and synchronization state while retaining
    /// physical book and thumbnail objects in the asset store.
    pub async fn delete_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, SyncError> {
        let phase_started = Instant::now();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        tracing::debug!(target: "sync_performance", phase = "pool_acquire_and_begin", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let library_id = library_id.to_string();
        let owned = sqlx::query("SELECT 1 FROM libraries WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL FOR UPDATE").bind(&library_id).bind(user_id).fetch_optional(&mut *transaction).await.map_err(internal)?.is_some();
        if !owned {
            transaction.rollback().await.map_err(internal)?;
            return Ok(false);
        }

        // Retain the UUID row as a tombstone. Concurrent and stale upserts will
        // conflict with it and are forbidden from clearing deleted_at.
        sqlx::query("UPDATE libraries SET deleted_at=now(), name='Deleted library' WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL").bind(&library_id).bind(user_id).execute(&mut *transaction).await.map_err(internal)?;

        sqlx::query(include_str!("sql/cloud_storage/delete_claims.sql")).bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        // Release unfinished upload quota before deleting its reservations.
        let reserved: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size_bytes), 0)::BIGINT
             FROM book_upload_reservation WHERE library_id=$1 AND user_id=$2",
        )
        .bind(&library_id)
        .bind(user_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(internal)?;
        if reserved > 0 {
            sqlx::query("UPDATE user_storage_account SET reserved_bytes=GREATEST(reserved_bytes-$2, 0) WHERE user_id=$1").bind(user_id).bind(reserved).execute(&mut *transaction).await.map_err(internal)?;
        }
        let lifecycle_hashes = sqlx::query_scalar::<_, String>("SELECT content_hash FROM sync_state WHERE library_id=$1 AND kind='book_lifecycle' AND version_rank<>2 AND content_hash IS NOT NULL")
            .bind(&library_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(internal)?;
        let cloud_enabled: bool = sqlx::query_scalar(include_str!("sql/cloud_storage/enabled.sql")).bind(&library_id).fetch_one(&mut *transaction).await.map_err(internal)?;
        assets::apply_lifecycle_reference_deltas(&mut transaction, user_id, lifecycle_hashes.into_iter().filter(|_| cloud_enabled).map(|hash| (sync_common::ContentHash::new(&hash), -1))).await.map_err(internal)?;
        sqlx::query(include_str!("sql/cloud_storage/release_unreferenced.sql")).bind(user_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM book_upload_reservation WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM sync_state WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM sync_library_revision WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        Ok(true)
    }

    pub(crate) async fn load_changes(&self, user_id: &str, query: &PullStateQuery) -> Result<PullStateResponse, SyncError> {
        match sync_store::fetch_changes(&self.pool, user_id, &query.library_id, &query.replica_id, query.cursor).await.map_err(fetch_error)? {
            sync_store::FetchChangesResult::Page(page) => Ok(pull_response(page)),
            sync_store::FetchChangesResult::Unauthorized => Err(SyncError::LibraryOwnershipConflict),
        }
    }

    pub async fn exchange(&self, user_id: &str, request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
        if request.mutations.len() > sync_common::MAX_PUSH_MUTATIONS {
            return Err(SyncError::TooManyMutations);
        }
        let mut identities = HashSet::with_capacity(request.mutations.len());
        if !request.mutations.iter().all(|mutation| identities.insert(mutation.publisher_seq)) {
            return Err(SyncError::AmbiguousMutationIdentity);
        }
        if request.mutations.is_empty() {
            let query = PullStateQuery { library_id: request.library_id.clone(), replica_id: request.replica_id.clone(), cursor: request.cursor };
            return Ok(SyncExchangeResponse { push: PushMutationsResponse { rejected: Vec::new() }, pull: self.load_changes(user_id, &query).await? });
        }

        let phase_started = Instant::now();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        tracing::debug!(target: "sync_performance", phase = "pool_acquire_and_begin", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let phase_started = Instant::now();
        if !self.authorize_owned_library(&mut transaction, user_id, &request.library_id).await? {
            transaction.rollback().await.map_err(internal)?;
            return Err(SyncError::LibraryOwnershipConflict);
        }
        tracing::debug!(target: "sync_performance", phase = "library_lock_and_authorize", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let phase_started = Instant::now();
        sync_store::validate_owned_cursor(&mut transaction, &request.library_id, request.cursor).await.map_err(fetch_error)?;
        tracing::debug!(target: "sync_performance", phase = "validate_cursor", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let phase_started = Instant::now();
        let inserted = sync_store::insert_changes(&mut transaction, user_id, &request.library_id, &request.replica_id, &request.mutations).await.map_err(internal)?;
        tracing::debug!(target: "sync_performance", phase = "insert_changes", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let phase_started = Instant::now();
        let page = sync_store::fetch_owned_changes(&mut transaction, &request.library_id, request.cursor).await.map_err(fetch_error)?;
        tracing::debug!(target: "sync_performance", phase = "fetch_page", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let phase_started = Instant::now();
        transaction.commit().await.map_err(internal)?;
        tracing::debug!(target: "sync_performance", phase = "commit", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        return Ok(SyncExchangeResponse { push: PushMutationsResponse { rejected: inserted.rejected }, pull: pull_response(page) });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use super::test_sequence;
    use crate::test_support::isolated_pool;
    use super::{PostgresSyncRepository, SyncError};
    use sqlx::PgPool;
    use sync_common::api::libraries::LibraryNameRequest;
    use sync_common::{MutationRejection, MutationRejectionReason, ReplicaSeq, SyncCursor, SyncExchangeRequest, OpaquePublication};

    #[tokio::test]
    async fn cloud_storage_disabled_accepts_metadata_without_upload_and_preserves_it() {
        let pool = isolated_pool("cloud_metadata", 4).await.expect("SYNC_E2E_DATABASE_URL is required");
        let (user, id) = seed_account(&pool).await;
        let repo = PostgresSyncRepository { pool: pool.clone() };
        repo.set_cloud_storage(&user, &id, uuid::Uuid::new_v4(), false).await.unwrap();
        let hash = sync_common::ContentHash::new(&"c".repeat(64));
        let mut change = wire_mutation("book_lifecycle", hash.as_str(), 10, 1);
        change.state.value.book = Some(hash);
        let response = repo.exchange(&user, &exchange_request(id, vec![change.clone()])).await.unwrap();
        assert!(response.push.rejected.is_empty());
        assert!(!response.pull.mutations.is_empty());
        repo.set_cloud_storage(&user, &id, uuid::Uuid::new_v4(), true).await.unwrap();
        repo.set_cloud_storage(&user, &id, uuid::Uuid::new_v4(), false).await.unwrap();
        let response = repo.exchange(&user, &exchange_request(id, vec![])).await.unwrap();
        assert!(response.pull.mutations.iter().any(|m| m.state.version.replica_seq == change.publisher_seq));
        assert_eq!(crate::assets::storage_usage(&pool, &user).await.unwrap().used_bytes, 0);
        assert!(repo.delete_library(&user, &id).await.unwrap(), "deleting a disabled library must not decrement its references twice");
    }

    #[tokio::test]
    async fn recovery_preserves_origin_in_batch_and_singleton_exchanges() {
        let pool = isolated_pool("recovery_origin", 4).await.expect("SYNC_E2E_DATABASE_URL is required");
        let (user, library) = seed_account(&pool).await;
        let repo = PostgresSyncRepository { pool: pool.clone() };
        repo.set_cloud_storage(&user, &library, uuid::Uuid::new_v4(), false).await.unwrap();
        let publisher = uuid::Uuid::from_u128(999);
        let key = "a".repeat(64);
        for kind in ["directory_name", "reading_position"] {
            let mut older = wire_mutation(kind, &key, 100, 500);
            older.state.value.book = None;
            older.state.value.bytes = vec![1];
            older.state.version = sync_common::VersionKey::new(100, sync_common::ConflictRank::Update, uuid::Uuid::from_u128(1), ReplicaSeq::new(90).unwrap());
            let mut winner = wire_mutation(kind, &key, 100, 1);
            winner.state.value.book = None;
            winner.state.value.bytes = vec![2];
            winner.state.version = sync_common::VersionKey::new(100, sync_common::ConflictRank::Update, uuid::Uuid::from_u128(2), ReplicaSeq::new(1).unwrap());
            let expected = (winner).state.version.clone();
            let table = "sync_state";
            for batch in [vec![older.clone(), winner.clone()], vec![winner.clone(), older.clone()]] {
                sqlx::query(&format!("DELETE FROM {table} WHERE library_id=$1")).bind(library.to_string()).execute(&pool).await.unwrap();
                if kind == "reading_position" {
                    let mut creation = wire_mutation("book_lifecycle", &key, 1, 900);
                    creation.state.value.book = Some(sync_common::ContentHash::new(&key));
                    repo.exchange(&user, &exchange_request(library, vec![creation])).await.unwrap();
                }
                let mut request = exchange_request(library, batch);
                request.replica_id = publisher;
                let response = repo.exchange(&user, &request).await.unwrap();
                assert!(response.push.rejected.is_empty());
                for changes in [vec![older.clone()], vec![]] {
                    let mut request = exchange_request(library, changes);
                    request.replica_id = publisher;
                    let response = repo.exchange(&user, &request).await.unwrap();
                    assert!(response.push.rejected.is_empty());
                    let stored = response.pull.mutations.iter().find(|m| m.state.value.address.kind == kind).unwrap();
                    assert_eq!(stored.state.value.bytes, vec![2]);
                    assert_eq!(stored.state.version.clone(), expected);
                }
            }
            // Restoring a missing cell must preserve provenance too, including
            // the reading singleton's direct-response fast path.
            sqlx::query(&format!("DELETE FROM {table} WHERE library_id=$1")).bind(library.to_string()).execute(&pool).await.unwrap();
            let mut request = exchange_request(library, vec![winner.clone()]);
            request.replica_id = publisher;
            let response = repo.exchange(&user, &request).await.unwrap();
            assert!(response.push.rejected.is_empty());
            let restored = response.pull.mutations.iter().find(|m| m.state.value.address.kind == kind).unwrap();
            assert_eq!(restored.state.version.clone(), expected);
        }
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_book_lifecycle_reaches_every_pull_before_its_fields() {
        let pool = isolated_pool("lifecycle_first", 4).await.expect("test database");
        let (user, library) = seed_account(&pool).await;
        let repo = PostgresSyncRepository { pool: pool.clone() };
        repo.set_cloud_storage(&user, &library, uuid::Uuid::new_v4(), false).await.unwrap();
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let mut addition = wire_mutation("book_lifecycle", hash.as_str(), 10, 1);
        addition.state.value.book = Some(hash);
        let first = repo.exchange(&user, &exchange_request(library, vec![addition.clone()])).await.unwrap();
        assert_eq!(first.pull.mutations.iter().map(|change| change.state.version.replica_seq).collect::<Vec<_>>(), vec![addition.publisher_seq]);
        sync_common::validate_pull_batch(&first.pull, SyncCursor::default()).unwrap();
        let mut cursor = first.pull.next_cursor;
        for (kind, time) in [("metadata", 20), ("reading_position", 30)] {
            let mut update = wire_mutation(kind, hash.as_str(), time, time);
            update.state.value.book = Some(hash);
            let mut request = exchange_request(library, vec![update]);
            request.cursor = cursor;
            let page = repo.exchange(&user, &request).await.unwrap().pull;
            assert_eq!(page.mutations.len(), 1);
            assert_eq!(page.mutations[0].state.value.address.kind, kind);
            sync_common::validate_pull_batch(&page, cursor).unwrap();
            cursor = page.next_cursor;
        }
        // Trash moves the lifecycle after its fields in server order. A fresh
        // replica must still receive the lifecycle first.
        let mut trash = addition.clone();
        trash.publisher_seq = test_sequence();
        trash.state.version.replica_seq = trash.publisher_seq;
        trash.state.version.changed_at = 40;
        trash.state.version.conflict_rank = sync_common::ConflictRank::Delete;
        repo.exchange(&user, &exchange_request(library, vec![trash.clone()])).await.unwrap();
        let snapshot = repo.exchange(&user, &exchange_request(library, vec![])).await.unwrap().pull;
        sync_common::validate_pull_batch(&snapshot, SyncCursor::default()).unwrap();
        assert_eq!(snapshot.mutations.iter().map(|change| change.state.value.address.kind.as_str()).collect::<Vec<_>>(), vec!["book_lifecycle", "metadata", "reading_position"]);
        assert_eq!(snapshot.mutations[0].state.version.replica_seq, trash.publisher_seq);
        let mut purge = addition;
        purge.publisher_seq = test_sequence();
        purge.state.version.replica_seq = purge.publisher_seq;
        purge.state.version.changed_at = 50;
        purge.state.version.conflict_rank = sync_common::ConflictRank::Purge;
        purge.state.value.book = Some(hash);
        repo.exchange(&user, &exchange_request(library, vec![purge.clone()])).await.unwrap();
        let snapshot = repo.exchange(&user, &exchange_request(library, vec![])).await.unwrap().pull;
        assert_eq!(snapshot.mutations.iter().map(|change| change.state.version.replica_seq).collect::<Vec<_>>(), vec![purge.publisher_seq], "a purge leaves only its lifecycle");
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_lifecycle_after_a_full_page_of_fields_still_comes_first() {
        let pool = isolated_pool("lifecycle_page_boundary", 4).await.unwrap();
        let (user, library) = seed_account(&pool).await;
        let repo = PostgresSyncRepository { pool: pool.clone() };
        repo.set_cloud_storage(&user, &library, uuid::Uuid::new_v4(), false).await.unwrap();
        let hash = sync_common::ContentHash::new(&"b".repeat(64));
        let mut initial = wire_mutation("book_lifecycle", hash.as_str(), 0, 3000);
        initial.state.value.book = Some(hash);
        repo.exchange(&user, &exchange_request(library, vec![initial])).await.unwrap();
        let mut updates = vec![wire_mutation("metadata", hash.as_str(), 1, 1)];
        updates[0].state.value.book = Some(hash);
        updates.extend((1..sync_common::MAX_PULL_CHANGES).map(|i| wire_mutation("directory_name", &format!("folder-{i}"), i as u64 + 1, i as u64 + 1)));
        for batch in updates.chunks(sync_common::MAX_PUSH_MUTATIONS) {
            repo.exchange(&user, &exchange_request(library, batch.to_vec())).await.unwrap();
        }
        // The rewritten lifecycle now follows every field in server order.
        let mut addition = wire_mutation("book_lifecycle", hash.as_str(), 2000, 2000);
        addition.state.value.book = Some(hash);
        repo.exchange(&user, &exchange_request(library, vec![addition.clone()])).await.unwrap();
        let first = repo.exchange(&user, &exchange_request(library, vec![])).await.unwrap().pull;
        assert!(first.has_more);
        assert_eq!(first.mutations.len(), sync_common::MAX_PULL_CHANGES);
        assert_eq!(first.mutations[0].state.version.replica_seq, addition.publisher_seq);
        sync_common::validate_pull_batch(&first, SyncCursor::default()).unwrap();
        let mut request = exchange_request(library, vec![]);
        request.cursor = first.next_cursor;
        let second = repo.exchange(&user, &request).await.unwrap().pull;
        assert!(!second.has_more);
        assert_eq!(second.mutations.len(), 1);
        sync_common::validate_pull_batch(&second, first.next_cursor).unwrap();
        let fields = first.mutations[1..].iter().chain(&second.mutations).map(|change| change.state.version.replica_seq).collect::<std::collections::HashSet<_>>();
        assert_eq!(fields, updates.iter().map(|update| update.publisher_seq).collect(), "every field arrives once, after the lifecycle");
    }

    #[tokio::test]
    async fn unified_book_lifetime_discards_fields_and_accepts_old_updates_after_readd() {
        let Some(pool) = isolated_pool("unified_lifetime", 4).await else { return };
        let (user, library) = seed_account(&pool).await;
        let repo = PostgresSyncRepository { pool: pool.clone() };
        repo.set_cloud_storage(&user, &library, uuid::Uuid::new_v4(), false).await.unwrap();
        let hash = "a".repeat(64);
        let lifecycle = |time, seq, present| {
            let mut value = wire_mutation("book_lifecycle", &hash, time, seq);
            value.state.value.book = Some(sync_common::ContentHash::new(&hash));
            value.state.version.conflict_rank = if present { sync_common::ConflictRank::Update } else { sync_common::ConflictRank::Purge };
            value
        };
        let field = |kind, time, seq| {
            let mut value = wire_mutation(kind, &hash, time, seq);
            value.state.value.book = Some(sync_common::ContentHash::new(&hash));
            if kind == "annotation" {
                value.state.value.address.entity_key = "annotation-id".into();
            }
            value
        };
        let fields = vec![field("metadata", 10, 1), field("annotation", 10, 2), field("reading_position", 10, 3)];
        // Existing library is local-only, so logical existence is the only gate.
        for mutation in &fields {
            let response = repo.exchange(&user, &exchange_request(library, vec![mutation.clone()])).await.unwrap();
            assert!(response.push.rejected.is_empty());
            assert!(response.pull.mutations.is_empty());
        }
        let mut initial = fields.clone();
        initial.push(lifecycle(1, 4, true)); // creation can be last in the batch
        let created = repo.exchange(&user, &exchange_request(library, initial)).await.unwrap();
        assert!(created.push.rejected.is_empty());
        assert_eq!(created.pull.mutations.len(), 4);
        let mut deletion = field("annotation", 11, 5);
        deletion.state.version.conflict_rank = sync_common::ConflictRank::Delete;
        assert!(repo.exchange(&user, &exchange_request(library, vec![deletion])).await.unwrap().push.rejected.is_empty());
        let mut purge_request = exchange_request(library, vec![lifecycle(20, 6, false), field("metadata", 100, 7)]);
        purge_request.cursor = created.pull.next_cursor;
        let purged = repo.exchange(&user, &purge_request).await.unwrap();
        assert!(purged.push.rejected.is_empty());
        assert_eq!(purged.pull.mutations.len(), 1);
        for table in ["sync_state"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE library_id=$1 AND kind!='book_lifecycle'")).bind(library.to_string()).fetch_one(&pool).await.unwrap();
            assert_eq!(count, 0, "purge clears {table}");
        }
        // An old cursor remains valid even though all reading rows were deleted.
        let mut delayed = exchange_request(library, vec![fields[2].clone()]);
        delayed.cursor = purged.pull.next_cursor;
        let ignored = repo.exchange(&user, &delayed).await.unwrap();
        assert!(ignored.push.rejected.is_empty());
        assert!(ignored.pull.mutations.is_empty());
        let mut readd = fields.clone();
        readd.push(lifecycle(30, 8, true));
        let response = repo.exchange(&user, &exchange_request(library, readd)).await.unwrap();
        assert!(response.push.rejected.is_empty());
        assert_eq!(response.pull.mutations.len(), 4);
        // A delayed losing purge cannot delete fields from the current readd.
        let stale = repo.exchange(&user, &purge_request).await.unwrap();
        assert!(stale.push.rejected.is_empty());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_state WHERE library_id=$1 AND kind!='book_lifecycle'").bind(library.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn losing_purge_redelivers_all_owned_fields_without_changing_versions() {
        let pool = isolated_pool("losing_purge", 4).await.expect("test database");
        let (user, library) = seed_account(&pool).await;
        let repo = PostgresSyncRepository { pool: pool.clone() };
        repo.set_cloud_storage(&user, &library, uuid::Uuid::new_v4(), false).await.unwrap();
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let other = sync_common::ContentHash::new(&"b".repeat(64));
        let mut creation = wire_mutation("book_lifecycle", hash.as_str(), 100, 1);
        creation.state.value.book = Some(hash);
        let mut other_creation = creation.clone();
        other_creation.publisher_seq = test_sequence();
        other_creation.state.version.replica_seq = other_creation.publisher_seq;
        other_creation.state.value.address.entity_key = other.to_string();
        other_creation.state.value.book = Some(other);
        let mut values = vec![creation.clone(), other_creation];
        for (i, kind) in ["book_facts", "metadata", "description", "placement", "reading_position", "pdf_reader_metadata", "book_toc", "annotation"].into_iter().enumerate() {
            let mut field = wire_mutation(kind, hash.as_str(), 10, i as u64 + 2);
            field.state.value.bytes = vec![i as u8];
            field.state.value.book = Some(hash);
            if kind == "annotation" {
                field.state.value.address.entity_key = "deleted-note".into();
            }
            if kind == "placement" {
                field.state.value.address.entity_subkey = sync_common::ROOT_DIR_ID.to_string();
            }
            values.push(field);
        }
        let mut unrelated = wire_mutation("metadata", other.as_str(), 10, 20);
        unrelated.state.value.book = Some(other);
        values.push(unrelated);
        let initial_request = exchange_request(library, values);
        let initial = repo.exchange(&user, &initial_request).await.unwrap();
        assert!(initial.push.rejected.is_empty());
        assert_eq!(initial.pull.mutations.len(), 11);
        let mut purge = creation;
        purge.publisher_seq = test_sequence();
        purge.state.version.replica_seq = purge.publisher_seq;
        purge.state.version.changed_at = 50; // loses to the existing creation
        purge.state.version.conflict_rank = sync_common::ConflictRank::Purge;
        purge.state.value.book = Some(hash);
        let mut request = exchange_request(library, vec![purge.clone()]);
        request.cursor = initial.pull.next_cursor;
        // Even rejected purge attempts must not cause field republication.
        request.mutations[0].state.version.changed_at = crate::sync_store::server_now_ms() + sync_common::MAX_LWW_FUTURE_SKEW_MS + 60_000;
        let rejected = repo.exchange(&user, &request).await.unwrap();
        assert_eq!(rejected.push.rejected.len(), 1);
        assert!(rejected.pull.mutations.is_empty());
        assert_eq!(rejected.pull.next_cursor, request.cursor);
        request.mutations = vec![purge];
        let repaired = repo.exchange(&user, &request).await.unwrap();
        assert!(repaired.push.rejected.is_empty());
        sync_common::validate_pull_batch(&repaired.pull, request.cursor).unwrap();
        assert_eq!(repaired.pull.mutations.len(), 9, "lifecycle plus every owned field, including deleted annotations");
        for row in &repaired.pull.mutations {
            let old = initial.pull.mutations.iter().find(|old| old.state.version.replica_seq == row.state.version.replica_seq).unwrap();
            assert_eq!(row.state, old.state);
            assert_eq!(row.state.version.replica_id, old.state.version.replica_id);
            assert!(row.revision > old.revision);
            assert_ne!(row.state.value.address.entity_key, other.as_str(), "other books must not be republished");
        }
        assert!(repaired.pull.next_cursor.lifecycle_revision > request.cursor.lifecycle_revision);
        assert!(repaired.pull.next_cursor.field_revision > request.cursor.field_revision);
        // Exact retries of normal values must still be revision-stable; the
        // explicit revision-only repair must not weaken duplicate suppression.
        let mut duplicate = initial_request;
        duplicate.cursor = repaired.pull.next_cursor;
        let stable = repo.exchange(&user, &duplicate).await.unwrap();
        assert!(stable.pull.mutations.is_empty());
        assert_eq!(stable.pull.next_cursor, duplicate.cursor);
    }

    async fn seed_account(pool: &PgPool) -> (String, sync_common::LibraryId) {
        let user_id = uuid::Uuid::new_v4().to_string();
        let library_id = sync_common::LibraryId::new_v4();
        sqlx::query("INSERT INTO users(id, email) VALUES($1, $2)").bind(&user_id).bind(format!("{user_id}@example.com")).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES($1, $2, 'Books')").bind(library_id.to_string()).bind(&user_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes) VALUES($1, 1000)").bind(&user_id).execute(pool).await.unwrap();
        (user_id, library_id)
    }

    fn wire_mutation(kind: &str, entity_key: &str, changed_at: u64, replica_seq: u64) -> OpaquePublication {
        let fixture_id = test_sequence();
        sync_common::Publication { publisher_seq: ReplicaSeq::new(replica_seq).unwrap(), state: sync_common::Versioned { version: sync_common::VersionKey::new(changed_at, sync_common::ConflictRank::Update, sync_common::ReplicaId::nil(), ReplicaSeq::new(replica_seq).unwrap()), value: sync_common::OpaqueStateEntry { address: sync_common::StateAddress { kind: kind.to_owned(), entity_key: entity_key.to_owned(), entity_subkey: String::new() }, bytes: Vec::new(), book: matches!(kind, "metadata" | "reading_position").then(|| sync_common::ContentHash::new(blake3::hash(entity_key.as_bytes()).to_hex().as_str())) } } }
    }

    async fn seed_uploaded_books(pool: &PgPool, user: &str, keys: &[&str]) {
        for key in keys {
            let hash = blake3::hash(key.as_bytes()).to_hex().to_string();
            sqlx::query("INSERT INTO blob_object(content_hash,size_bytes) VALUES($1,10)").bind(&hash).execute(pool).await.unwrap();
            sqlx::query("INSERT INTO user_blob_charge(user_id,content_hash) VALUES($1,$2)").bind(user).bind(&hash).execute(pool).await.unwrap();
            let library: String = sqlx::query_scalar("SELECT id FROM libraries WHERE user_id=$1").bind(user).fetch_one(pool).await.unwrap();
            let mut creation = wire_mutation("book_lifecycle", &hash, 1, 1);
            creation.state.value.book = Some(sync_common::ContentHash::new(&hash));
            let response = PostgresSyncRepository { pool: pool.clone() }.exchange(user, &exchange_request(sync_common::LibraryId::parse_str(&library).unwrap(), vec![creation])).await.unwrap();
            assert!(response.push.rejected.is_empty());
        }
    }

    fn exchange_request(library_id: sync_common::LibraryId, mutations: Vec<OpaquePublication>) -> SyncExchangeRequest {
        SyncExchangeRequest { library_id, replica_id: sync_common::ReplicaId::new_v4(), mutations, cursor: SyncCursor::default() }
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn sync_requires_explicit_named_library_creation() {
        let Some(pool) = isolated_pool("explicit_library", 2).await else { return };
        let (user, _) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };
        let id = sync_common::LibraryId::new_v4();
        // Empty exchange (pull), singleton fast path, and transactional batch.
        for mutations in [Vec::new(), vec![wire_mutation("directory_name", "a", 1, 1)], vec![wire_mutation("directory_name", "a", 1, 1), wire_mutation("directory_name", "b", 2, 2)]] {
            assert!(matches!(repository.exchange(&user, &exchange_request(id, mutations)).await, Err(SyncError::LibraryOwnershipConflict)));
        }
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM libraries WHERE id=$1").bind(id.to_string()).fetch_one(&pool).await.unwrap(), 0);
        assert!(matches!(repository.create_library(&user, &LibraryNameRequest { library_id: id, library_name: "   ".into() }).await, Err(SyncError::InvalidLibraryName)));
        repository.create_library(&user, &LibraryNameRequest { library_id: id, library_name: "Humaniora".into() }).await.unwrap().unwrap();
        repository.exchange(&user, &exchange_request(id, vec![wire_mutation("directory_name", "a", 1, 1)])).await.unwrap();
        repository.exchange(&user, &exchange_request(id, vec![wire_mutation("directory_name", "b", 2, 2), wire_mutation("directory_name", "c", 3, 3)])).await.unwrap();
        repository.exchange(&user, &exchange_request(id, Vec::new())).await.unwrap();
        let (other, _) = seed_account(&pool).await;
        assert!(matches!(repository.exchange(&other, &exchange_request(id, Vec::new())).await, Err(SyncError::LibraryOwnershipConflict)));
        assert_eq!(sqlx::query_scalar::<_, String>("SELECT name FROM libraries WHERE id=$1").bind(id.to_string()).fetch_one(&pool).await.unwrap(), "Humaniora");
        let missing_name = sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(sync_common::LibraryId::new_v4().to_string()).bind(&user).execute(&pool).await.unwrap_err();
        assert_eq!(missing_name.as_database_error().unwrap().code().as_deref(), Some("23502"));
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn stale_creation_cannot_rename_and_rename_cannot_create() {
        let Some(pool) = isolated_pool("create_rename", 2).await else { return };
        let (user, _) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };
        let id = sync_common::LibraryId::new_v4();
        let old = LibraryNameRequest { library_id: id, library_name: "Folder name".into() };
        assert!(repository.rename_library(&user, &old).await.unwrap().is_none());
        assert!(repository.create_library(&user, &old).await.unwrap().unwrap().changed);
        let renamed = LibraryNameRequest { library_id: id, library_name: "New name".into() };
        assert!(repository.rename_library(&user, &renamed).await.unwrap().unwrap().changed);
        let stale = repository.create_library(&user, &old).await.unwrap().unwrap();
        assert!(!stale.changed);
        assert_eq!(stale.library.library_name, "New name");
        assert!(!repository.rename_library(&user, &renamed).await.unwrap().unwrap().changed);
        let (other, _) = seed_account(&pool).await;
        assert!(repository.create_library(&other, &old).await.unwrap().is_none());
        assert!(repository.rename_library(&other, &old).await.unwrap().is_none());
        assert!(repository.delete_library(&user, &id).await.unwrap());
        assert!(repository.rename_library(&user, &renamed).await.unwrap().is_none());
        assert!(repository.create_library(&user, &old).await.unwrap().is_none());
    }

    /// The client pages its outbox by `MAX_PUSH_MUTATIONS`, but nothing on the
    /// wire obliges it to. The service must impose its own published bound
    /// rather than admitting an arbitrarily large single transaction.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn exchange_refuses_a_push_page_beyond_the_published_maximum() {
        let Some(pool) = isolated_pool("push_limit", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        let mutations = (1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 1).map(|sequence| wire_mutation("directory_name", &format!("dir-{sequence}"), 1, sequence)).collect::<Vec<_>>();
        let error = repository.exchange(&user_id, &exchange_request(library_id, mutations)).await.err();

        assert!(matches!(error, Some(SyncError::TooManyMutations)), "a batch past MAX_PUSH_MUTATIONS must be refused, got {error:?}");
    }

    /// Mutation identity is the acknowledgement key on both sides, so a batch
    /// that reuses one identity is uninterpretable. It must be refused as a
    /// client error, not collide inside `UNIQUE (library_id, event_id)` and
    /// fail the whole batch as an internal fault.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn exchange_refuses_a_batch_that_reuses_one_mutation_identity() {
        let Some(pool) = isolated_pool("ambiguous_identity", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        let shared_identity = test_sequence();
        let mut first = wire_mutation("directory_name", "dir-a", 1, 1);
        let mut second = wire_mutation("directory_name", "dir-b", 1, 2);
        first.publisher_seq = shared_identity;
        second.publisher_seq = shared_identity;
        let error = repository.exchange(&user_id, &exchange_request(library_id, vec![first, second])).await.err();

        assert!(matches!(error, Some(SyncError::AmbiguousMutationIdentity)), "a reused mutation identity must be a client error, got {error:?}");
    }

    /// The delta loop in `insert_changes` evaluates every lifecycle mutation
    /// against the stored row independently, while `upsert_state_batch`
    /// collapses them to one winner. Both must agree on which mutation was
    /// applied, or blob references drift from the state they account for.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn repeated_lifecycle_mutations_for_one_book_charge_only_the_winning_reference() {
        let Some(pool) = isolated_pool("lifecycle_delta", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };
        let (original, superseded, winner) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));

        for hash in [&original, &superseded, &winner] {
            sqlx::query("INSERT INTO blob_object(content_hash, size_bytes) VALUES($1, 10)").bind(hash).execute(&pool).await.unwrap();
        }
        for hash in [&superseded, &winner] {
            sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES($1,$2)").bind(&user_id).bind(hash).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES($1, $2)").bind(&user_id).bind(&original).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_blob_reference(user_id, content_hash, reference_count) VALUES($1, $2, 1)").bind(&user_id).bind(&original).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO sync_state(library_id,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank)
             VALUES($1,'book_lifecycle','book','',''::bytea,$2,1,$3,1,0)",
        )
        .bind(library_id.to_string())
        .bind(&original)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&pool)
        .await
        .unwrap();

        // Both target the same cell and both beat the stored row. Only the
        // later one survives the batch's DISTINCT ON.
        let mut earlier = wire_mutation("book_lifecycle", "book", 10, 1);
        let mut later = wire_mutation("book_lifecycle", "book", 20, 2);
        earlier.state.value.book = Some(sync_common::ContentHash::new(&superseded));
        later.state.value.book = Some(sync_common::ContentHash::new(&winner));
        repository.exchange(&user_id, &exchange_request(library_id, vec![earlier, later])).await.expect("a batch repeating one cell must apply cleanly");

        let stored: String = sqlx::query_scalar("SELECT content_hash FROM sync_state WHERE library_id=$1 AND kind='book_lifecycle' AND entity_key='book'").bind(library_id.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, winner, "the later mutation owns the cell");

        let reference = |hash: String| {
            let pool = pool.clone();
            let user_id = user_id.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(reference_count), 0)::BIGINT FROM user_blob_reference WHERE user_id=$1 AND content_hash=$2").bind(user_id).bind(hash).fetch_one(&pool).await.unwrap() }
        };
        assert_eq!(reference(winner.clone()).await, 1, "the applied hash is charged exactly once");
        assert_eq!(reference(superseded.clone()).await, 0, "a hash that never reached storage is never charged");
        assert_eq!(reference(original.clone()).await, 0, "the replaced hash is released exactly once");
    }

    /// `book` is optional on the wire, but the quota ledger charges a lifecycle's
    /// book. A lifecycle without one must be refused up front, permanently.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn exchange_refuses_a_lifecycle_mutation_without_a_book() {
        let Some(pool) = isolated_pool("lifecycle_undeclared", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        let undeclared = wire_mutation("book_lifecycle", "book", 10, 1);
        assert!(undeclared.state.value.book.is_none());
        let response = repository.exchange(&user_id, &exchange_request(library_id, vec![undeclared.clone()])).await.expect("the batch itself is well formed");

        assert_eq!(response.push.rejected.len(), 1);
        assert_eq!(response.push.rejected, vec![MutationRejection { publisher_seq: undeclared.publisher_seq, reason: MutationRejectionReason::InvalidPayload }]);
        assert!(!MutationRejectionReason::InvalidPayload.is_transient(), "the client must stop retrying rather than queue it forever");
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_state WHERE library_id=$1").bind(library_id.to_string()).fetch_one(&pool).await.unwrap(), 0, "a refused mutation leaves no row behind to poison the cell");
    }

    /// Lifecycle accounting must inspect the exact logical cell that the
    /// mutation will update. A non-empty subkey is unusual for today's client,
    /// but it is legal on the wire and must not make the ledger compare against
    /// the unrelated empty-subkey row.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn lifecycle_accounting_uses_the_mutations_entity_subkey() {
        let Some(pool) = isolated_pool("lifecycle_subkey", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };
        let (original, replacement) = ("d".repeat(64), "e".repeat(64));

        for hash in [&original, &replacement] {
            sqlx::query("INSERT INTO blob_object(content_hash, size_bytes) VALUES($1, 10)").bind(hash).execute(&pool).await.unwrap();
        }
        for hash in [&replacement] {
            sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES($1,$2)").bind(&user_id).bind(hash).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES($1, $2)").bind(&user_id).bind(&original).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_blob_reference(user_id, content_hash, reference_count) VALUES($1, $2, 1)").bind(&user_id).bind(&original).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO sync_state(library_id,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank)
             VALUES($1,'book_lifecycle','book','edition-a',''::bytea,$2,1,$3,1,0)",
        )
        .bind(library_id.to_string())
        .bind(&original)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&pool)
        .await
        .unwrap();

        let mut replacement_mutation = wire_mutation("book_lifecycle", "book", 10, 2);
        replacement_mutation.state.value.address.entity_subkey = "edition-a".to_owned();
        replacement_mutation.state.value.book = Some(sync_common::ContentHash::new(&replacement));
        repository.exchange(&user_id, &exchange_request(library_id, vec![replacement_mutation])).await.expect("the targeted lifecycle cell is replaced");

        let reference_count = |hash: String| {
            let pool = pool.clone();
            let user_id = user_id.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(reference_count), 0)::BIGINT FROM user_blob_reference WHERE user_id=$1 AND content_hash=$2").bind(user_id).bind(hash).fetch_one(&pool).await.unwrap() }
        };
        assert_eq!(reference_count(original).await, 0, "the reference held by the targeted subkey is released");
        assert_eq!(reference_count(replacement).await, 1, "the replacement reference is charged once");
    }

    /// A published head must never cover an uncommitted lower sequence. If it
    /// does, a reader can persist the newer cursor and permanently skip the
    /// earlier mutation when its transaction eventually commits.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_committed_reading_head_never_skips_an_in_flight_lower_sequence() {
        let Some(pool) = isolated_pool("reading_head_gap", 6).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        seed_uploaded_books(&pool, &user_id, &["slow-a", "slow-b", "fast"]).await;
        const DELAY_LOCK: i64 = 7_341_991;
        sqlx::query(
            "CREATE FUNCTION delay_selected_reading_insert() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.entity_key = 'slow-a' THEN PERFORM pg_advisory_lock(7341991); END IF;
                 RETURN NEW;
             END $$",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TRIGGER delay_selected_reading_insert BEFORE INSERT ON sync_state FOR EACH ROW EXECUTE FUNCTION delay_selected_reading_insert()").execute(&pool).await.unwrap();
        let mut delay_connection = pool.acquire().await.unwrap();
        sqlx::query("SELECT pg_advisory_lock($1)").bind(DELAY_LOCK).execute(&mut *delay_connection).await.unwrap();
        let sequence_before = sqlx::query_as::<_, (i64, bool)>("SELECT last_value, is_called FROM sync_server_sequence").fetch_one(&pool).await.unwrap();

        let repository = PostgresSyncRepository { pool: pool.clone() };
        let mut slow_a = wire_mutation("reading_position", "slow-a", 10, 1);
        let mut slow_b = wire_mutation("reading_position", "slow-b", 10, 2);
        slow_a.state.value.bytes = b"slow-a".to_vec();
        slow_b.state.value.bytes = b"slow-b".to_vec();
        let batch_repository = repository.clone();
        let batch_user = user_id.clone();
        let batch_request = exchange_request(library_id, vec![slow_a, slow_b]);
        let batch = tokio::spawn(async move { batch_repository.exchange(&batch_user, &batch_request).await });

        let mut lower_sequence_allocated = false;
        for _ in 0..100 {
            let sequence_after = sqlx::query_as::<_, (i64, bool)>("SELECT last_value, is_called FROM sync_server_sequence").fetch_one(&pool).await.unwrap();
            if sequence_after.0 > sequence_before.0 || (!sequence_before.1 && sequence_after.1) {
                lower_sequence_allocated = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(lower_sequence_allocated, "the delayed batch must allocate its lower sequence before the racing singleton");

        let fast = wire_mutation("reading_position", "fast", 20, 1);
        let fast_repository = repository.clone();
        let fast_user = user_id.clone();
        let fast_request = exchange_request(library_id, vec![fast]);
        let mut fast_exchange = tokio::spawn(async move { fast_repository.exchange(&fast_user, &fast_request).await });
        assert!(tokio::time::timeout(Duration::from_millis(100), &mut fast_exchange).await.is_err(), "the singleton must wait for the lower-sequence transaction");

        assert!(sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)").bind(DELAY_LOCK).fetch_one(&mut *delay_connection).await.unwrap());
        batch.await.unwrap().expect("the delayed batch eventually commits");
        let fast_response = fast_exchange.await.unwrap().expect("the singleton commits after the lower sequences");
        let delivered_with_head = fast_response.pull.mutations.iter().map(|change| change.state.value.address.entity_key.as_str()).collect::<Vec<_>>();
        assert!(
            delivered_with_head.contains(&"slow-a") && delivered_with_head.contains(&"slow-b") && delivered_with_head.contains(&"fast"),
            "the response that advertises the head must include every lower sequence; got {delivered_with_head:?}"
        );
        let reader_cursor = fast_response.pull.next_cursor;

        let after_gap =
            repository.exchange(&user_id, &SyncExchangeRequest { library_id, replica_id: sync_common::ReplicaId::new_v4(), mutations: Vec::new(), cursor: reader_cursor }).await.expect("the reader continues from the published cursor");
        assert!(after_gap.pull.mutations.is_empty(), "nothing committed at or below the advertised head may appear later");
    }

    /// A push that loses the stored row's last-writer-wins comparison is still
    /// reported as accepted -- the protocol has no "merged and discarded"
    /// acknowledgement -- and the submitting replica has already applied the
    /// value locally. The winning row sits at or below that replica's cursor,
    /// so cancelling the update silently would strand it forever. The guard
    /// must republish the winner above the cursor instead.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_superseded_push_republishes_the_winner_above_the_losers_cursor() {
        let Some(pool) = isolated_pool("lww_republish", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        seed_uploaded_books(&pool, &user_id, &["book"]).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        let mut winner = wire_mutation("metadata", "book", 2_000, 1);
        winner.state.value.bytes = b"winner".to_vec();
        let published = repository.exchange(&user_id, &exchange_request(library_id, vec![winner.clone()])).await.expect("publish the winning version");
        let cursor = published.pull.next_cursor;

        // A replica whose clock runs behind writes the same cell honestly and
        // loses. It pushes from the cursor that already covers the winner.
        let mut loser = wire_mutation("metadata", "book", 1_000, 2);
        loser.state.value.bytes = b"loser".to_vec();
        let request = SyncExchangeRequest { library_id, replica_id: sync_common::ReplicaId::new_v4(), mutations: vec![loser.clone()], cursor };
        let response = repository.exchange(&user_id, &request).await.expect("a losing push is still accepted");

        assert!(response.push.rejected.is_empty());
        let stored: Vec<u8> = sqlx::query_scalar("SELECT value FROM sync_state WHERE library_id=$1 AND kind='metadata' AND entity_key='book'").bind(library_id.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, b"winner", "the stale write must not take the cell");

        let delivered = response.pull.mutations.iter().map(|change| change.state.value.bytes.as_slice()).collect::<Vec<_>>();
        assert_eq!(delivered, vec![b"winner".as_slice()], "the same exchange must hand the loser the authoritative value");
        assert!(response.pull.next_cursor.field_revision.unwrap().get() > cursor.field_revision.unwrap().get(), "the republished winner advances the field cursor");
    }

    /// Republishing must be reserved for a genuine conflict loss. A push
    /// retried after a lost response carries the identity that already owns the
    /// cell, and must not churn the channel head for every other replica.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_retried_push_of_the_stored_mutation_does_not_advance_the_head() {
        let Some(pool) = isolated_pool("lww_retry", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        seed_uploaded_books(&pool, &user_id, &["book"]).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        // A retry replays the identical envelope, replica identity included:
        // the authoritative actor comes from the request, not the mutation.
        let replica_id = sync_common::ReplicaId::new_v4();
        let mutation = wire_mutation("metadata", "book", 2_000, 1);
        let publish = SyncExchangeRequest { library_id, replica_id, mutations: vec![mutation.clone()], cursor: SyncCursor::default() };
        let published = repository.exchange(&user_id, &publish).await.expect("publish once");
        let cursor = published.pull.next_cursor;

        let retry = SyncExchangeRequest { library_id, replica_id, mutations: vec![mutation.clone()], cursor };
        let response = repository.exchange(&user_id, &retry).await.expect("a retried push is accepted");

        assert!(response.push.rejected.is_empty());
        assert!(response.pull.mutations.is_empty(), "an idempotent retry has nothing to redeliver");
        assert_eq!(response.pull.next_cursor, cursor, "an idempotent retry leaves the channel head where it was");
    }

    /// The atomic reading-position exchange answers from the row it just wrote
    /// rather than re-reading a page. When that write lost, the row holds the
    /// republished winner, so the response must carry the winner -- never echo
    /// back the caller's own discarded position as though it had been stored.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_superseded_reading_position_is_answered_with_the_stored_winner() {
        let Some(pool) = isolated_pool("lww_reading_republish", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        seed_uploaded_books(&pool, &user_id, &["book"]).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        let mut winner = wire_mutation("reading_position", "book", 2_000, 1);
        winner.state.value.bytes = b"winner".to_vec();
        let published = repository.exchange(&user_id, &exchange_request(library_id, vec![winner.clone()])).await.expect("publish the winning position");
        let cursor = published.pull.next_cursor;

        let mut loser = wire_mutation("reading_position", "book", 1_000, 2);
        loser.state.value.bytes = b"loser".to_vec();
        let request = SyncExchangeRequest { library_id, replica_id: sync_common::ReplicaId::new_v4(), mutations: vec![loser.clone()], cursor };
        let response = repository.exchange(&user_id, &request).await.expect("a losing reading position is still accepted");

        assert!(response.push.rejected.is_empty());
        let delivered = response.pull.mutations.iter().map(|change| change.state.value.bytes.as_slice()).collect::<Vec<_>>();
        assert_eq!(delivered, vec![b"winner".as_slice()], "the caught-up fast path must not echo a discarded mutation");
        let stored: Vec<u8> = sqlx::query_scalar("SELECT value FROM sync_state WHERE library_id=$1 AND entity_key='book'").bind(library_id.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, b"winner");
    }

    /// A server whose state has regressed (a restore from an older backup)
    /// leaves every device holding a cursor the server can no longer honour.
    /// The service must say so distinctly rather than silently replaying.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn a_regressed_library_head_reports_the_stale_client_cursor() {
        let Some(pool) = isolated_pool("cursor_ahead", 2).await else { return };
        let (user_id, library_id) = seed_account(&pool).await;
        let repository = PostgresSyncRepository { pool: pool.clone() };

        let published = repository.exchange(&user_id, &exchange_request(library_id, vec![wire_mutation("directory_name", "dir-a", 1, 1)])).await.expect("publish one mutation");
        let cursor = published.pull.next_cursor;
        assert!(cursor.field_revision.is_some(), "the published mutation advances the field cursor");

        // Restore-from-backup: the library's rows and its head both regress.
        sqlx::query("DELETE FROM sync_state WHERE library_id=$1").bind(library_id.to_string()).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM sync_library_revision WHERE library_id=$1").bind(library_id.to_string()).execute(&pool).await.unwrap();

        let request = SyncExchangeRequest { library_id, replica_id: sync_common::ReplicaId::new_v4(), mutations: Vec::new(), cursor };
        let error = repository.exchange(&user_id, &request).await.err();

        assert!(matches!(error, Some(SyncError::CursorAhead)), "a cursor past the regressed head must be reported distinctly, got {error:?}");
    }

    #[test]
    fn opaque_wire_fixture_has_no_domain_payload() {
        let fixture_id = test_sequence();
        let mutation = sync_common::Publication { publisher_seq: ReplicaSeq::new(1).unwrap(), state: sync_common::Versioned { version: sync_common::VersionKey::new(1, sync_common::ConflictRank::Update, sync_common::ReplicaId::nil(), ReplicaSeq::new(1).unwrap()), value: sync_common::OpaqueStateEntry { address: sync_common::StateAddress { kind: "future_kind".to_owned(), entity_key: "opaque-key".to_owned(), entity_subkey: String::new() }, bytes: vec![0xde, 0xad, 0xbe, 0xef], book: None } } };
        assert_eq!(mutation.state.value.bytes, [0xde, 0xad, 0xbe, 0xef]);
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn deleting_library_keeps_blob_object_and_releases_library_quota_state() {
        let Some(pool) = isolated_pool("delete_library", 2).await else { return };
        let user_id = uuid::Uuid::new_v4().to_string();
        let library_id = sync_common::LibraryId::new_v4();
        let hash = "a".repeat(64);
        sqlx::query("INSERT INTO users(id, email) VALUES($1, $2)").bind(&user_id).bind(format!("{user_id}@example.com")).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES($1, $2, 'Books')").bind(library_id.to_string()).bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes, used_bytes, reserved_bytes) VALUES($1, 100, 10, 5)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO blob_object(content_hash, size_bytes) VALUES($1, 10)").bind(&hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES($1, $2)").bind(&user_id).bind(&hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_blob_reference(user_id, content_hash, reference_count) VALUES($1, $2, 1)").bind(&user_id).bind(&hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO sync_state(library_id, kind, entity_key, value, content_hash, changed_at, replica_id, replica_seq, version_rank) VALUES($1, 'book_lifecycle', $2, ''::bytea, $2, 1, $3, 1, 0)")
            .bind(library_id.to_string())
            .bind(&hash)
            .bind(uuid::Uuid::new_v4().to_string())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO book_upload_reservation(id, user_id, library_id, content_hash, size_bytes, expires_at) VALUES($1, $2, $3, $4, 5, now() + interval '1 hour')")
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&user_id)
            .bind(library_id.to_string())
            .bind("b".repeat(64))
            .execute(&pool)
            .await
            .unwrap();
        let repository = PostgresSyncRepository { pool: pool.clone() };
        assert!(repository.delete_library(&user_id, &library_id).await.unwrap());
        assert!(!repository.delete_library(&user_id, &library_id).await.unwrap(), "deletion is idempotent");

        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM libraries WHERE id=$1").bind(library_id.to_string()).fetch_one(&pool).await.unwrap(), 1, "the UUID tombstone is retained");
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM libraries WHERE id=$1 AND deleted_at IS NOT NULL").bind(library_id.to_string()).fetch_one(&pool).await.unwrap(), 1);
        assert_eq!(repository.list_deleted_libraries(&user_id).await.unwrap(), vec![library_id]);
        assert!(repository.list_libraries(&user_id).await.unwrap().is_empty());
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_state WHERE library_id=$1").bind(library_id.to_string()).fetch_one(&pool).await.unwrap(), 0);
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM book_upload_reservation WHERE library_id=$1").bind(library_id.to_string()).fetch_one(&pool).await.unwrap(), 0);
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blob_object WHERE content_hash=$1").bind(&hash).fetch_one(&pool).await.unwrap(), 1);
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_blob_charge WHERE user_id=$1 AND content_hash=$2").bind(&user_id).bind(&hash).fetch_one(&pool).await.unwrap(), 0);
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_blob_reference WHERE user_id=$1 AND content_hash=$2").bind(&user_id).bind(&hash).fetch_one(&pool).await.unwrap(), 0);
        let usage = sqlx::query_as::<_, (i64, i64)>("SELECT used_bytes, reserved_bytes FROM user_storage_account WHERE user_id=$1").bind(&user_id).fetch_one(&pool).await.unwrap();
        assert_eq!(usage, (0, 0));

        let stale_upsert = repository.create_library(&user_id, &LibraryNameRequest { library_id, library_name: "Recreated stale copy".to_owned() }).await.unwrap();
        assert!(stale_upsert.is_none(), "a deleted UUID cannot be recreated by a stale device");
        assert!(matches!(repository.exchange(&user_id, &exchange_request(library_id, Vec::new())).await, Err(SyncError::LibraryOwnershipConflict)));

        let replacement_id = sync_common::LibraryId::new_v4();
        assert!(repository.create_library(&user_id, &LibraryNameRequest { library_id: replacement_id, library_name: "Replacement".to_owned() }).await.unwrap().is_some());
    }
}

#[cfg(test)]
fn test_sequence() -> sync_common::ReplicaSeq {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000_000);
    sync_common::ReplicaSeq::new(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)).unwrap()
}

use crate::{assets, sync_store, PostgresDatabase};
use sqlx::{PgPool, Postgres, Transaction};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{PullStateQuery, PullStateResponse, PushMutationsResponse, StateInventoryRequest, StateInventoryResponse, SyncExchangeRequest, SyncExchangeResponse};

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
pub struct LibraryUpsertOutcome {
    pub library: LibrarySummary,
    pub changed: bool,
}

#[derive(Clone)]
pub struct PostgresSyncRepository {
    pool: PgPool,
}

impl PostgresSyncRepository {
    pub fn new(database: PostgresDatabase) -> Self {
        Self { pool: database.pool().clone() }
    }

    async fn ensure_compatible_library_binding(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<(), SyncError> {
        if sync_store::ensure_library(&self.pool, user_id, library_id).await.map_err(internal)? {
            Ok(())
        } else {
            Err(SyncError::LibraryOwnershipConflict)
        }
    }

    async fn authorize_owned_library(&self, transaction: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, SyncError> {
        let row = sqlx::query(include_str!("sql/sync/authorize_owned_library.sql")).bind(library_id.to_string()).bind(user_id).fetch_optional(&mut **transaction).await.map_err(internal)?;
        Ok(row.is_some())
    }

    async fn exchange_atomic_singleton(&self, user_id: &str, request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
        let change = &request.mutations[0];
        for attempt in 0..2 {
            match sync_store::exchange_authorized_singleton(&self.pool, user_id, &request.library_id, &request.replica_id, change, request.cursor).await.map_err(fetch_error)? {
                sync_store::AtomicSingletonExchange::Handled(page) => return Ok(SyncExchangeResponse { push: PushMutationsResponse { accepted: vec![change.mutation_id], rejected: Vec::new() }, pull: pull_response(page, request.cursor) }),
                sync_store::AtomicSingletonExchange::CursorAhead => return Err(SyncError::CursorAhead),
                sync_store::AtomicSingletonExchange::Unauthorized if attempt == 0 => self.ensure_compatible_library_binding(user_id, &request.library_id).await?,
                sync_store::AtomicSingletonExchange::Unauthorized => return Err(SyncError::LibraryOwnershipConflict),
            }
        }
        unreachable!("atomic exchange binding retry must return")
    }
}

fn pull_response(page: sync_store::FetchChangesPage, current_cursor: Option<sync_common::LibraryRevision>) -> PullStateResponse {
    let next_cursor = page.changes.last().map(|mutation| mutation.revision).or(current_cursor);
    PullStateResponse { mutations: page.changes, next_cursor, has_more: page.has_more }
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

fn is_serialization_failure(error: &sqlx::Error) -> bool {
    error.as_database_error().and_then(|database| database.code()).as_deref() == Some("40001")
}

fn is_fetch_serialization_failure(error: &sync_store::FetchChangesError) -> bool {
    matches!(error, sync_store::FetchChangesError::Database(database) if is_serialization_failure(database))
}

impl PostgresSyncRepository {
    pub async fn list_libraries(&self, user_id: &str) -> Result<Vec<LibrarySummary>, SyncError> {
        sync_store::list_libraries(&self.pool, user_id).await.map_err(internal)
    }

    pub async fn list_deleted_libraries(&self, user_id: &str) -> Result<Vec<sync_common::LibraryId>, SyncError> {
        let ids = sqlx::query_scalar::<_, String>("SELECT id FROM libraries WHERE user_id=$1 AND deleted_at IS NOT NULL ORDER BY deleted_at, id").bind(user_id).fetch_all(&self.pool).await.map_err(internal)?;
        ids.into_iter().map(|id| sync_common::LibraryId::parse_str(&id).map_err(internal)).collect()
    }

    pub async fn upsert_library(&self, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, SyncError> {
        let name = request.library_name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(SyncError::InvalidLibraryName);
        }
        sync_store::upsert_library(&self.pool, user_id, &UpsertLibraryRequest { library_name: name.to_owned(), ..request.clone() }).await.map_err(internal)
    }

    pub async fn compare_state_inventory(&self, user_id: &str, request: &StateInventoryRequest) -> Result<StateInventoryResponse, SyncError> {
        if request.cells.len() > sync_common::MAX_STATE_INVENTORY_CELLS {
            return Err(SyncError::TooManyMutations);
        }
        if !sync_store::owns_library(&self.pool, user_id, &request.library_id).await.map_err(internal)? {
            return Err(SyncError::LibraryOwnershipConflict);
        }
        let missing = sync_store::missing_state_cells(&self.pool, &request.library_id, &request.cells).await.map_err(internal)?;
        Ok(StateInventoryResponse { missing })
    }

    /// Deletes server-side catalogue and synchronization state while retaining
    /// physical book and thumbnail objects in the asset store.
    pub async fn delete_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, SyncError> {
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let library_id = library_id.to_string();
        let owned = sqlx::query("SELECT 1 FROM libraries WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL FOR UPDATE").bind(&library_id).bind(user_id).fetch_optional(&mut *transaction).await.map_err(internal)?.is_some();
        if !owned {
            transaction.rollback().await.map_err(internal)?;
            return Ok(false);
        }

        // Retain the UUID row as a tombstone. Concurrent and stale upserts will
        // conflict with it and are forbidden from clearing deleted_at.
        sqlx::query("UPDATE libraries SET deleted_at=now(), name='Deleted library' WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL").bind(&library_id).bind(user_id).execute(&mut *transaction).await.map_err(internal)?;

        // Release unfinished upload quota before deleting its reservations.
        let reserved: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(size_bytes), 0)::BIGINT FROM book_upload_reservation WHERE library_id=$1 AND user_id=$2").bind(&library_id).bind(user_id).fetch_one(&mut *transaction).await.map_err(internal)?;
        if reserved > 0 {
            sqlx::query("UPDATE user_storage_account SET reserved_bytes=GREATEST(reserved_bytes-$2, 0) WHERE user_id=$1").bind(user_id).bind(reserved).execute(&mut *transaction).await.map_err(internal)?;
        }
        let lifecycle_hashes = sqlx::query_scalar::<_, String>("SELECT content_hash FROM sync_state WHERE library_id=$1 AND kind='book_lifecycle' AND present IS TRUE AND content_hash IS NOT NULL")
            .bind(&library_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(internal)?;
        assets::apply_lifecycle_reference_deltas(&mut transaction, user_id, lifecycle_hashes.into_iter().map(|hash| (sync_common::ContentHash::new(&hash), -1))).await.map_err(internal)?;
        sqlx::query("DELETE FROM book_upload_reservation WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM sync_reading_state WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM sync_state WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM sync_library_revision WHERE library_id=$1").bind(&library_id).execute(&mut *transaction).await.map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        Ok(true)
    }

    pub(crate) async fn load_changes(&self, user_id: &str, query: &PullStateQuery) -> Result<PullStateResponse, SyncError> {
        for attempt in 0..2 {
            let fetched = sync_store::fetch_changes(&self.pool, user_id, &query.library_id, &query.replica_id, query.cursor).await.map_err(fetch_error)?;
            match fetched {
                sync_store::FetchChangesResult::Page(page) => return Ok(pull_response(page, query.cursor)),
                sync_store::FetchChangesResult::Unauthorized if attempt == 0 => self.ensure_compatible_library_binding(user_id, &query.library_id).await?,
                sync_store::FetchChangesResult::Unauthorized => return Err(SyncError::LibraryOwnershipConflict),
            }
        }
        unreachable!("pull binding retry must return")
    }

    pub async fn exchange(&self, user_id: &str, request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
        if request.mutations.is_empty() {
            let query = PullStateQuery { library_id: request.library_id.clone(), replica_id: request.replica_id.clone(), cursor: request.cursor };
            return Ok(SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull: self.load_changes(user_id, &query).await? });
        }

        let server_now_ms = sync_store::server_now_ms();
        if request.mutations.len() == 1 && sync_store::supports_atomic_singleton(&request.mutations[0]) && sync_store::storage_rejection(&request.mutations[0], server_now_ms).is_none() {
            return self.exchange_atomic_singleton(user_id, request).await;
        }

        for attempt in 0..3 {
            let mut transaction = self.pool.begin().await.map_err(internal)?;
            sqlx::query(include_str!("sql/sync/set_serializable.sql")).execute(&mut *transaction).await.map_err(internal)?;
            if !self.authorize_owned_library(&mut transaction, user_id, &request.library_id).await? {
                transaction.rollback().await.map_err(internal)?;
                if attempt == 0 {
                    self.ensure_compatible_library_binding(user_id, &request.library_id).await?;
                    continue;
                }
                return Err(SyncError::LibraryOwnershipConflict);
            }
            if let Err(error) = sync_store::validate_owned_cursor(&mut transaction, &request.library_id, request.cursor).await {
                if attempt < 2 && is_fetch_serialization_failure(&error) {
                    let _ = transaction.rollback().await;
                    continue;
                }
                return Err(fetch_error(error));
            }
            let inserted = match sync_store::insert_changes(&mut transaction, user_id, &request.library_id, &request.replica_id, &request.mutations).await {
                Ok(inserted) => inserted,
                Err(error) if attempt < 2 && is_serialization_failure(&error) => {
                    let _ = transaction.rollback().await;
                    continue;
                }
                Err(error) => return Err(internal(error)),
            };
            let page = match sync_store::fetch_owned_changes(&mut transaction, &request.library_id, request.cursor).await {
                Ok(page) => page,
                Err(error) if attempt < 2 && is_fetch_serialization_failure(&error) => {
                    let _ = transaction.rollback().await;
                    continue;
                }
                Err(error) => return Err(fetch_error(error)),
            };
            if let Err(error) = transaction.commit().await {
                if attempt < 2 && is_serialization_failure(&error) {
                    continue;
                }
                return Err(internal(error));
            }
            return Ok(SyncExchangeResponse { push: PushMutationsResponse { accepted: inserted.accepted, rejected: inserted.rejected }, pull: pull_response(page, request.cursor) });
        }
        unreachable!("transactional exchange binding retry must return")
    }
}

#[cfg(test)]
mod tests {
    use super::PostgresSyncRepository;
    use crate::sync_store;
    use sync_common::api::libraries::UpsertLibraryRequest;
    use sync_common::{MutationId, ReplicaSeq, WireMutation};

    #[test]
    fn opaque_wire_fixture_has_no_domain_payload() {
        let mutation = WireMutation {
            mutation_id: MutationId::new(),
            kind: "future_kind".to_owned(),
            entity_key: "opaque-key".to_owned(),
            entity_subkey: String::new(),
            value: vec![0xde, 0xad, 0xbe, 0xef],
            conflict_rank: 0,
            blob_reference: None,
            changed_at: 1,
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        assert_eq!(mutation.value, [0xde, 0xad, 0xbe, 0xef]);
    }

    #[tokio::test]
    async fn deleting_library_keeps_blob_object_and_releases_library_quota_state() {
        let Some(pool) = crate::test_support::isolated_pool("delete_library", 2).await else { return };
        let user_id = uuid::Uuid::new_v4().to_string();
        let library_id = sync_common::LibraryId::new_v4();
        let hash = "a".repeat(64);
        sqlx::query("INSERT INTO users(id, email) VALUES($1, $2)").bind(&user_id).bind(format!("{user_id}@example.com")).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES($1, $2, 'Books')").bind(library_id.to_string()).bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes, used_bytes, reserved_bytes) VALUES($1, 100, 10, 5)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO blob_object(content_hash, size_bytes) VALUES($1, 10)").bind(&hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES($1, $2)").bind(&user_id).bind(&hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_blob_reference(user_id, content_hash, reference_count) VALUES($1, $2, 1)").bind(&user_id).bind(&hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO sync_state(library_id, kind, entity_key, value, present, content_hash, changed_at, replica_id, replica_seq, version_rank, event_id) VALUES($1, 'book_lifecycle', $2, ''::bytea, TRUE, $2, 1, $3, 1, 0, $4)")
            .bind(library_id.to_string())
            .bind(&hash)
            .bind(uuid::Uuid::new_v4().to_string())
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

        let stale_upsert = repository.upsert_library(&user_id, &UpsertLibraryRequest { library_id, library_name: "Recreated stale copy".to_owned() }).await.unwrap();
        assert!(stale_upsert.is_none(), "a deleted UUID cannot be recreated by a stale device");
        assert!(!sync_store::ensure_library(&pool, &user_id, &library_id).await.unwrap());

        let replacement_id = sync_common::LibraryId::new_v4();
        assert!(repository.upsert_library(&user_id, &UpsertLibraryRequest { library_id: replacement_id, library_name: "Replacement".to_owned() }).await.unwrap().is_some());
    }
}

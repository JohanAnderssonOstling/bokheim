use crate::{assets, sync_store, PostgresDatabase};
use async_trait::async_trait;
use server_core::{AssetError, AssetRepository, BookUploadAdmission, BookUploadReservation, GcLease, LibraryUpsertOutcome, LiveAssets, MaintenanceCoordinator, SyncError, SyncRepository};
use sqlx::{PgPool, Postgres, Transaction};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
use sync_common::{ContentHash, PullStateResponse, PullStateQuery, PushMutationsRequest, PushMutationsResponse, SyncExchangeRequest, SyncExchangeResponse};

const MAINTENANCE_LOCK_NAME: &str = "bokheim:global-maintenance";

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

    async fn lock_owned_library(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        user_id: &str,
        library_id: &sync_common::LibraryId,
        coordinate_gc: bool,
    ) -> Result<bool, SyncError> {
        let row = if coordinate_gc {
            sqlx::query(
                "SELECT pg_advisory_xact_lock_shared(hashtextextended($3, 0)),
                        pg_advisory_xact_lock(hashtextextended($1, 0))
                 FROM libraries WHERE id=$1 AND user_id=$2",
            )
            .bind(library_id.as_str())
            .bind(user_id)
            .bind(MAINTENANCE_LOCK_NAME)
            .fetch_optional(&mut **transaction)
            .await
        } else {
            sqlx::query(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))
                 FROM libraries WHERE id=$1 AND user_id=$2",
            )
            .bind(library_id.as_str())
            .bind(user_id)
            .fetch_optional(&mut **transaction)
            .await
        }
        .map_err(internal)?;
        Ok(row.is_some())
    }

    async fn store_atomic_singleton(&self, user_id: &str, request: &PushMutationsRequest) -> Result<PushMutationsResponse, SyncError> {
        let change = &request.mutations[0];
        for attempt in 0..2 {
            match sync_store::insert_authorized_singleton(&self.pool, user_id, &request.library_id, &request.replica_id, change).await.map_err(internal)? {
                sync_store::AtomicSingletonInsert::Handled => return Ok(PushMutationsResponse { accepted: vec![change.mutation_id], rejected: Vec::new() }),
                sync_store::AtomicSingletonInsert::Unauthorized if attempt == 0 => self.ensure_compatible_library_binding(user_id, &request.library_id).await?,
                sync_store::AtomicSingletonInsert::Unauthorized => return Err(SyncError::LibraryOwnershipConflict),
            }
        }
        unreachable!("atomic singleton binding retry must return")
    }

    async fn exchange_atomic_singleton(&self, user_id: &str, request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
        let change = &request.mutations[0];
        for attempt in 0..2 {
            match sync_store::exchange_authorized_singleton(&self.pool, user_id, &request.library_id, &request.replica_id, change, request.cursor).await.map_err(fetch_error)? {
                sync_store::AtomicSingletonExchange::Handled(page) => return Ok(SyncExchangeResponse {
                    push: PushMutationsResponse { accepted: vec![change.mutation_id], rejected: Vec::new() },
                    pull: pull_response(page, request.cursor),
                }),
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

fn asset_internal(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

#[async_trait]
impl AssetRepository for PostgresSyncRepository {
    async fn owns_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, AssetError> {
        sync_store::owns_library(&self.pool, user_id, library_id).await.map_err(asset_internal)
    }

    async fn negotiate_books(&self, user_id: &str, library_id: &sync_common::LibraryId, blobs: &[BlobManifestEntry]) -> Result<BlobManifestResponse, AssetError> {
        assets::negotiate_books(&self.pool, user_id, library_id, blobs).await
    }

    async fn claim_existing_book(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<(), AssetError> {
        assets::claim_existing_book(&self.pool, user_id, library_id, hash, size_bytes).await
    }

    async fn reserve_book_upload(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError> {
        assets::reserve_book_upload(&self.pool, user_id, library_id, hash, size_bytes).await
    }

    async fn finish_book_upload(&self, reservation: &BookUploadReservation, actual_size_bytes: u64) -> Result<(), AssetError> {
        assets::finish_book_upload(&self.pool, reservation, actual_size_bytes).await
    }

    async fn abort_book_upload(&self, reservation: &BookUploadReservation) -> Result<(), AssetError> {
        assets::abort_book_upload(&self.pool, reservation).await
    }
}

#[async_trait]
impl SyncRepository for PostgresSyncRepository {
    async fn list_libraries(&self, user_id: &str) -> Result<Vec<LibrarySummary>, SyncError> {
        sync_store::list_libraries(&self.pool, user_id).await.map_err(internal)
    }

    async fn upsert_library(&self, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, SyncError> {
        sync_store::upsert_library(&self.pool, user_id, request).await.map_err(internal)
    }

    async fn store_changes(&self, user_id: &str, request: &PushMutationsRequest) -> Result<PushMutationsResponse, SyncError> {
        let server_now_ms = sync_store::server_now_ms();
        if request.mutations.len() == 1 && sync_store::supports_atomic_singleton(&request.mutations[0]) && sync_store::storage_rejection(&request.mutations[0], server_now_ms).is_none() {
            return self.store_atomic_singleton(user_id, request).await;
        }
        let coordinate_gc = request.mutations.iter().any(|change| matches!(change.body, sync_common::MutationBody::Placement { .. }));
        for attempt in 0..2 {
            let mut transaction = self.pool.begin().await.map_err(internal)?;
            // Authorization and lock acquisition share one query. The global
            // shared lock is needed only when a push can change blob liveness;
            // GC takes its exclusive counterpart while deleting files.
            if !self.lock_owned_library(&mut transaction, user_id, &request.library_id, coordinate_gc).await? {
                transaction.rollback().await.map_err(internal)?;
                if attempt == 0 {
                    self.ensure_compatible_library_binding(user_id, &request.library_id).await?;
                    continue;
                }
                return Err(SyncError::LibraryOwnershipConflict);
            }
            let result = sync_store::insert_changes(&mut transaction, user_id, &request.library_id, &request.replica_id, &request.mutations).await.map_err(internal)?;
            assets::apply_placement_deltas(&mut transaction, user_id, &result.placement_deltas).await.map_err(internal)?;
            transaction.commit().await.map_err(internal)?;
            return Ok(PushMutationsResponse { accepted: result.accepted, rejected: result.rejected });
        }
        unreachable!("transactional binding retry must return")
    }

    async fn load_changes(&self, user_id: &str, query: &PullStateQuery) -> Result<PullStateResponse, SyncError> {
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

    async fn exchange(&self, user_id: &str, request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
        if request.mutations.is_empty() {
            let query = PullStateQuery {
                library_id: request.library_id.clone(),
                replica_id: request.replica_id.clone(),
                cursor: request.cursor,
            };
            return Ok(SyncExchangeResponse {
                push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                pull: self.load_changes(user_id, &query).await?,
            });
        }

        let server_now_ms = sync_store::server_now_ms();
        if request.mutations.len() == 1 && sync_store::supports_atomic_singleton(&request.mutations[0]) && sync_store::storage_rejection(&request.mutations[0], server_now_ms).is_none() {
            return self.exchange_atomic_singleton(user_id, request).await;
        }

        let coordinate_gc = request.mutations.iter().any(|change| matches!(change.body, sync_common::MutationBody::Placement { .. }));
        for attempt in 0..2 {
            let mut transaction = self.pool.begin().await.map_err(internal)?;
            if !self.lock_owned_library(&mut transaction, user_id, &request.library_id, coordinate_gc).await? {
                transaction.rollback().await.map_err(internal)?;
                if attempt == 0 {
                    self.ensure_compatible_library_binding(user_id, &request.library_id).await?;
                    continue;
                }
                return Err(SyncError::LibraryOwnershipConflict);
            }
            sync_store::validate_owned_cursor(&mut transaction, &request.library_id, request.cursor).await.map_err(fetch_error)?;
            let inserted = sync_store::insert_changes(&mut transaction, user_id, &request.library_id, &request.replica_id, &request.mutations).await.map_err(internal)?;
            assets::apply_placement_deltas(&mut transaction, user_id, &inserted.placement_deltas).await.map_err(internal)?;
            let page = sync_store::fetch_owned_changes(&mut transaction, &request.library_id, request.cursor).await.map_err(fetch_error)?;
            transaction.commit().await.map_err(internal)?;
            return Ok(SyncExchangeResponse {
                push: PushMutationsResponse { accepted: inserted.accepted, rejected: inserted.rejected },
                pull: pull_response(page, request.cursor),
            });
        }
        unreachable!("transactional exchange binding retry must return")
    }
}

struct PostgresGcLease {
    transaction: Option<Transaction<'static, Postgres>>,
    live: LiveAssets,
}

#[async_trait]
impl GcLease for PostgresGcLease {
    fn live_assets(&self) -> &LiveAssets {
        &self.live
    }

    async fn release(mut self: Box<Self>) -> Result<(), SyncError> {
        if let Some(transaction) = self.transaction.take() {
            transaction.commit().await.map_err(internal)?;
        }
        Ok(())
    }
}

#[async_trait]
impl MaintenanceCoordinator for PostgresSyncRepository {
    async fn begin_gc(&self) -> Result<Box<dyn GcLease>, SyncError> {
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(MAINTENANCE_LOCK_NAME).execute(&mut *transaction).await.map_err(internal)?;
        // Query through the same transaction that owns the exclusive lock. This
        // remains correct even when the pool has only one connection.
        let live = sync_store::live_assets(&mut transaction).await.map_err(internal)?;
        Ok(Box::new(PostgresGcLease { transaction: Some(transaction), live }))
    }
}

#[cfg(test)]
mod tests {
    use super::{PostgresSyncRepository, MAINTENANCE_LOCK_NAME};
    use crate::PostgresDatabase;
    use server_core::{MaintenanceCoordinator, SyncError, SyncRepository};
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;
    use sync_common::api::libraries::UpsertLibraryRequest;
    use sync_common::{AnnotationAnchor, AnnotationState, AnnotationStyle, AuthorCredit, BookId, BookLifecycleState, BookLinkState, BookMetadataState, BookmarkState, ClientId, ContentHash, DirId, DirectoryState, MutationId, MutationRejectionReason, FileName, LibraryId, LibraryRevision, StateMutation, ReplicaSeq, PdfAnnotationAnchor, PdfAnnotationRect, PlacementState, PullStateQuery, PushMutationsRequest, ReadingPositionState, SyncExchangeRequest, MutationBody, UnixMillis, MAX_LWW_FUTURE_SKEW_MS, MAX_PULL_CHANGES};

    fn millis(value: u64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }

    fn author(value: &str) -> AuthorCredit {
        AuthorCredit::from_name(value).unwrap()
    }

    fn metadata(title: impl Into<String>, authors: Vec<AuthorCredit>, description: impl Into<String>) -> BookMetadataState {
        BookMetadataState {
            title: title.into(),
            authors,
            description: description.into(),
            publication: Default::default(),
        }
    }

    fn change(sequence: u64, body: MutationBody) -> StateMutation {
        StateMutation { mutation_id: MutationId::new(), body, changed_at: millis(1), replica_seq: ReplicaSeq::new(sequence).unwrap() }
    }

    fn push(_user_id: &str, library_id: LibraryId, replica_id: &str, change: StateMutation) -> PushMutationsRequest {
        PushMutationsRequest { library_id, replica_id: ClientId::parse(replica_id).unwrap(), mutations: vec![change] }
    }

    fn reading_body(book_id: u64, location: &str, progress: f32) -> MutationBody {
        MutationBody::ReadingPosition { book_id: BookId::new(book_id), value: ReadingPositionState { location: location.to_string(), progress } }
    }

    fn placement(book_id: u64, dir_id: DirId, file_name: FileName, content_hash: ContentHash) -> MutationBody {
        MutationBody::Placement { dir_id, file_name, value: PlacementState::Present { book_id: BookId::new(book_id), content_hash } }
    }

    fn vacancy(book_id: u64, dir_id: DirId, file_name: FileName) -> MutationBody {
        MutationBody::Placement { dir_id, file_name, value: PlacementState::Vacant { removed_book_id: BookId::new(book_id) } }
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn typed_state_schema_has_no_json_payload_and_enforces_one_identity() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let json_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns
             WHERE table_schema=current_schema()
               AND table_name LIKE 'sync_%state%'
               AND data_type IN ('json','jsonb')",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(json_columns, 0, "synchronized state must not contain JSON payload columns");
        let old_table: Option<String> = sqlx::query_scalar("SELECT to_regclass('library_events')::TEXT").fetch_one(&pool).await.unwrap();
        assert!(old_table.is_none(), "the legacy event archive must be removed");
        let envelope_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT table_name) FROM information_schema.columns
             WHERE table_schema=current_schema() AND table_name LIKE 'sync_%_state' AND column_name='server_seq'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(envelope_tables, 9, "every typed state table must own its synchronization envelope");
        let obsolete_clock_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns
             WHERE table_schema=current_schema()
               AND table_name LIKE 'sync_%_state'
               AND column_name IN ('hlc_pt','hlc_l')",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(obsolete_clock_columns, 0, "canonical synchronized state must store only wall-clock LWW versions");
        let old_envelope: Option<String> = sqlx::query_scalar("SELECT to_regclass('sync_state')::TEXT").fetch_one(&pool).await.unwrap();
        assert!(old_envelope.is_none(), "the shared synchronization envelope must be removed");
        let old_placement_projection: Option<String> = sqlx::query_scalar("SELECT to_regclass('live_book_path')::TEXT").fetch_one(&pool).await.unwrap();
        assert!(old_placement_projection.is_none(), "canonical placement state must not be duplicated in a quota projection");

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("integrity-user-{suffix}");
        let library_id = format!("integrity-library-{suffix}");
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(&library_id).bind(&user_id).execute(&pool).await.unwrap();
        let invalid_lifecycle = sqlx::query(
            "INSERT INTO sync_book_lifecycle_state(library_id,book_id,title,added_at,deleted,changed_at,client_id,client_seq,version_rank,event_id)
             VALUES($1,'1',NULL,NULL,FALSE,1,'integrity-client',1,0,$2)",
        ).bind(&library_id).bind(uuid::Uuid::new_v4().to_string()).execute(&pool).await;
        assert!(invalid_lifecycle.is_err(), "a present book without its required typed payload must be unrepresentable");
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn exclusive_maintenance_lock_blocks_push_lock_across_pools() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let first_pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        let second_pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(first_pool.clone())).await.unwrap();

        let mut maintenance = first_pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(MAINTENANCE_LOCK_NAME).execute(&mut *maintenance).await.unwrap();

        let mut push_waiter = tokio::spawn(async move {
            let mut push = second_pool.begin().await.unwrap();
            sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1, 0))").bind(MAINTENANCE_LOCK_NAME).execute(&mut *push).await.unwrap();
            push.commit().await.unwrap();
        });

        assert!(tokio::time::timeout(Duration::from_millis(150), &mut push_waiter).await.is_err());
        maintenance.commit().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), push_waiter).await.unwrap().unwrap();

        let one_connection_pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(one_connection_pool));
        let lease = tokio::time::timeout(Duration::from_secs(2), repository.begin_gc()).await.expect("GC must not request a second pool connection").unwrap();
        lease.release().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn steady_state_sync_authorizes_without_rewriting_the_library() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("steady-user-{suffix}");
        let library_id = LibraryId::parse(&format!("steady-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES ($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES ($1, $2, 'Steady')").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let before: (String, String) = sqlx::query_as("SELECT xmin::TEXT, ctid::TEXT FROM libraries WHERE id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();

        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let catalogue = repository.upsert_library(&user_id, &UpsertLibraryRequest { library_id: library_id.clone(), library_name: "Steady".to_string() }).await.unwrap().unwrap();
        assert_eq!(catalogue.library.library_name, "Steady");
        assert!(!catalogue.changed);
        let after_catalogue: (String, String) = sqlx::query_as("SELECT xmin::TEXT, ctid::TEXT FROM libraries WHERE id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(after_catalogue, before, "republishing an unchanged library name must not rewrite the row");
        let reading = change(1, reading_body(1, "epubcfi(/6/2)", 10.0));
        let response = repository.store_changes(&user_id, &push(&user_id, library_id.clone(), "steady-writer", reading)).await.unwrap();
        assert_eq!(response.accepted.len(), 1);
        let pulled = repository
            .load_changes(&user_id, &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("steady-reader").unwrap(), cursor: None })
            .await
            .unwrap();
        assert_eq!(pulled.mutations.len(), 1);

        let after: (String, String) = sqlx::query_as("SELECT xmin::TEXT, ctid::TEXT FROM libraries WHERE id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(after, before, "steady-state push and pull must not create a no-op library row version");

        let foreign_user = format!("foreign-user-{suffix}");
        sqlx::query("INSERT INTO users(id) VALUES ($1)").bind(&foreign_user).execute(&pool).await.unwrap();
        let foreign = change(1, reading_body(1, "epubcfi(/6/4)", 20.0));
        assert!(matches!(repository.store_changes(&foreign_user, &push(&foreign_user, library_id.clone(), "foreign-writer", foreign)).await, Err(SyncError::LibraryOwnershipConflict)));
        let after_foreign: (String, String) = sqlx::query_as("SELECT xmin::TEXT, ctid::TEXT FROM libraries WHERE id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(after_foreign, before, "foreign synchronization must not rewrite the owned library row");

        let compatible_library = LibraryId::parse(&format!("compatible-library-{suffix}")).unwrap();
        let first = change(1, reading_body(2, "epubcfi(/6/2)", 10.0));
        repository.store_changes(&user_id, &push(&user_id, compatible_library.clone(), "compatible-writer", first)).await.unwrap();
        let owner: String = sqlx::query_scalar("SELECT user_id FROM libraries WHERE id=$1").bind(compatible_library.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(owner, user_id, "a legacy client's first synchronization must still bind its missing library once");

        sqlx::query("DELETE FROM users WHERE id=ANY($1)").bind(vec![user_id, foreign_user]).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn lww_admission_rejects_future_wall_time() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("clock-policy-user-{suffix}");
        let library_id = LibraryId::parse(&format!("clock-policy-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let server_now = crate::sync_store::server_now_ms();

        let mut future = change(1, reading_body(1, "epubcfi(/6/2)", 10.0));
        future.changed_at = millis(server_now + MAX_LWW_FUTURE_SKEW_MS + 60_000);
        let future_id = future.mutation_id;
        let rejected = repository.store_changes(&user_id, &push(&user_id, library_id.clone(), "clock-writer", future)).await.unwrap();
        assert!(rejected.accepted.is_empty());
        assert_eq!(rejected.rejected.len(), 1);
        assert_eq!(rejected.rejected[0].mutation_id, future_id);
        assert_eq!(rejected.rejected[0].reason, MutationRejectionReason::TimestampTooFarFuture);

        let mut valid = change(2, reading_body(1, "epubcfi(/6/4)", 20.0));
        valid.changed_at = millis(server_now);
        let valid_id = valid.mutation_id;
        let accepted = repository.store_changes(&user_id, &push(&user_id, library_id.clone(), "clock-writer", valid)).await.unwrap();
        assert_eq!(accepted.accepted, vec![valid_id]);
        assert!(accepted.rejected.is_empty());
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_reading_position_state WHERE library_id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, 1);
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn atomic_exchange_returns_its_committed_winner_and_retries_idempotently() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("exchange-user-{suffix}");
        let library_id = LibraryId::parse(&format!("exchange-library-{suffix}")).unwrap();
        let client_id = ClientId::parse("exchange-writer").unwrap();
        sqlx::query("INSERT INTO users(id) VALUES ($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES ($1, $2, 'Exchange')").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let reading = change(1, reading_body(42, "epubcfi(/6/2)", 10.0));

        let first = repository
            .exchange(
                &user_id,
                &SyncExchangeRequest {
                    library_id: library_id.clone(),
                    replica_id: client_id.clone(),
                    mutations: vec![reading.clone()],
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(first.push.accepted, vec![reading.mutation_id]);
        assert!(first.push.rejected.is_empty());
        assert_eq!(first.pull.mutations.len(), 1, "the exchange page must observe its own winning write");
        assert_eq!(first.pull.mutations[0].mutation_id, reading.mutation_id);
        let cursor = first.pull.mutations[0].revision;
        let (first_watermark, first_reading_watermark, first_other_watermark): (i64, i64, i64) = sqlx::query_as(
            "SELECT current_revision, reading_revision, other_revision FROM sync_library_revision WHERE library_id=$1",
        )
            .bind(library_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(first_watermark, cursor.get() as i64, "the library watermark must commit with the winning state row");
        assert_eq!(first_reading_watermark, first_watermark, "a reading mutation must advance the reading channel");
        assert_eq!(first_other_watermark, 0, "a reading mutation must not advance the general channel");

        let lost_response_retry = repository
            .exchange(
                &user_id,
                &SyncExchangeRequest {
                    library_id: library_id.clone(),
                    replica_id: client_id.clone(),
                    mutations: vec![reading.clone()],
                    cursor: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(lost_response_retry.push.accepted, vec![reading.mutation_id]);
        assert_eq!(lost_response_retry.pull.mutations.len(), 1, "retrying after a lost response must replay the committed page from the unchanged cursor");
        assert_eq!(lost_response_retry.pull.mutations[0].revision, cursor);

        let checkpointed_retry = repository
            .exchange(
                &user_id,
                &SyncExchangeRequest {
                    library_id: library_id.clone(),
                    replica_id: client_id,
                    mutations: vec![reading.clone()],
                    cursor: Some(cursor),
                },
            )
            .await
            .unwrap();
        assert_eq!(checkpointed_retry.push.accepted, vec![reading.mutation_id]);
        assert!(checkpointed_retry.pull.mutations.is_empty(), "an idempotent retry must not redraw the singleton server sequence");
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_reading_position_state WHERE library_id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, 1);
        let retry_watermark: i64 = sqlx::query_scalar("SELECT current_revision FROM sync_library_revision WHERE library_id=$1")
            .bind(library_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retry_watermark, first_watermark, "an idempotent retry must not advance the library watermark");

        let foreign_user = format!("exchange-foreign-{suffix}");
        sqlx::query("INSERT INTO users(id) VALUES ($1)").bind(&foreign_user).execute(&pool).await.unwrap();
        let foreign = repository
            .exchange(
                &foreign_user,
                &SyncExchangeRequest {
                    library_id: library_id.clone(),
                    replica_id: ClientId::parse("exchange-intruder").unwrap(),
                    mutations: vec![change(1, reading_body(42, "epubcfi(/6/4)", 20.0))],
                    cursor: None,
                },
            )
            .await;
        assert!(matches!(foreign, Err(SyncError::LibraryOwnershipConflict)));
        let still_stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_reading_position_state WHERE library_id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(still_stored, 1, "an unauthorized exchange must not modify the library stream");

        sqlx::query("DELETE FROM users WHERE id=ANY($1)").bind(vec![user_id, foreign_user]).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn split_pull_watermarks_preserve_pagination_and_mixed_order() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("split-pull-user-{suffix}");
        let library_id = LibraryId::parse(&format!("split-pull-library-{suffix}")).unwrap();
        let replica_id = ClientId::parse("split-pull-writer").unwrap();
        sqlx::query("INSERT INTO users(id) VALUES ($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES ($1, $2, 'Split pull')").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));

        let lifecycle = change(1, MutationBody::BookLifecycle {
            book_id: BookId::new(1),
            value: BookLifecycleState::Present {
                metadata: metadata("Seed", Vec::new(), ""),
                added_at: millis(1),
            },
        });
        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), replica_id.as_str(), lifecycle)).await.unwrap();
        let baseline = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("split-pull-reader").unwrap(), cursor: None },
        ).await.unwrap();
        let baseline_cursor = baseline.next_cursor.expect("seed pull has a cursor");

        let readings = (0..=MAX_PULL_CHANGES)
            .map(|index| change(
                index as u64 + 2,
                reading_body(index as u64 + 10, &format!("epubcfi(/6/{})", index * 2 + 2), (index % 101) as f32),
            ))
            .collect::<Vec<_>>();
        let stored = repository.store_changes(
            &user_id,
            &PushMutationsRequest { library_id: library_id.clone(), replica_id: replica_id.clone(), mutations: readings },
        ).await.unwrap();
        assert_eq!(stored.accepted.len(), MAX_PULL_CHANGES + 1);
        assert!(stored.rejected.is_empty());

        let first_page = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("split-pull-reader").unwrap(), cursor: Some(baseline_cursor) },
        ).await.unwrap();
        assert_eq!(first_page.mutations.len(), MAX_PULL_CHANGES);
        assert!(first_page.has_more);
        assert!(first_page.mutations.iter().all(|mutation| matches!(&mutation.body, MutationBody::ReadingPosition { .. })));
        let first_cursor = first_page.next_cursor.expect("first reading page has a cursor");

        let second_page = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("split-pull-reader").unwrap(), cursor: Some(first_cursor) },
        ).await.unwrap();
        assert_eq!(second_page.mutations.len(), 1);
        assert!(!second_page.has_more);
        assert!(matches!(&second_page.mutations[0].body, MutationBody::ReadingPosition { .. }));
        let caught_up_cursor = second_page.next_cursor.expect("second reading page has a cursor");

        let mixed = vec![
            change(MAX_PULL_CHANGES as u64 + 3, reading_body(10, "epubcfi(/8/2)", 50.0)),
            change(MAX_PULL_CHANGES as u64 + 4, MutationBody::Metadata {
                book_id: BookId::new(1),
                value: metadata("Changed", vec![author("Author")], "Description"),
            }),
        ];
        repository.store_changes(
            &user_id,
            &PushMutationsRequest { library_id: library_id.clone(), replica_id, mutations: mixed },
        ).await.unwrap();
        let mixed_page = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("split-pull-reader").unwrap(), cursor: Some(caught_up_cursor) },
        ).await.unwrap();
        assert_eq!(mixed_page.mutations.len(), 2);
        assert!(mixed_page.mutations.windows(2).all(|pair| pair[0].revision < pair[1].revision));
        assert!(mixed_page.mutations.iter().any(|mutation| matches!(&mutation.body, MutationBody::ReadingPosition { .. })));
        assert!(mixed_page.mutations.iter().any(|mutation| matches!(&mutation.body, MutationBody::Metadata { .. })));
        let mixed_cursor = mixed_page.next_cursor.expect("mixed page has a cursor");

        let empty = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("split-pull-reader").unwrap(), cursor: Some(mixed_cursor) },
        ).await.unwrap();
        assert!(empty.mutations.is_empty());
        assert!(!empty.has_more);
        assert_eq!(empty.next_cursor, Some(mixed_cursor));

        let (current, reading, other): (i64, i64, i64) = sqlx::query_as(
            "SELECT current_revision, reading_revision, other_revision FROM sync_library_revision WHERE library_id=$1",
        ).bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(current, std::cmp::max(reading, other));
        assert!(reading > baseline_cursor.get() as i64);
        assert!(other > baseline_cursor.get() as i64);
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn canonical_path_state_does_not_resurrect_displaced_books() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("path-state-user-{suffix}");
        let library_id = LibraryId::parse(&format!("path-state-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let dir_id = DirId::parse("__root__").unwrap();
        let file_name = FileName::parse("shared.epub").unwrap();
        let first_hash = ContentHash::parse(&"1".repeat(64)).unwrap();
        let second_hash = ContentHash::parse(&"2".repeat(64)).unwrap();
        let client_id = ClientId::parse("path-writer").unwrap();
        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), client_id.as_str(), change(1, placement(1, dir_id.clone(), file_name.clone(), first_hash.clone())))).await.unwrap();
        let first_reference: i64 = sqlx::query_scalar("SELECT reference_count FROM user_blob_reference WHERE user_id=$1 AND content_hash=$2").bind(&user_id).bind(first_hash.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(first_reference, 1);

        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), client_id.as_str(), change(2, placement(2, dir_id.clone(), file_name.clone(), second_hash.clone())))).await.unwrap();
        let replaced_references: Vec<(String, i64)> = sqlx::query_as("SELECT content_hash,reference_count FROM user_blob_reference WHERE user_id=$1 ORDER BY content_hash").bind(&user_id).fetch_all(&pool).await.unwrap();
        assert_eq!(replaced_references, vec![(second_hash.as_str().to_owned(), 1)], "a winning replacement must move the reference exactly once");

        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), client_id.as_str(), change(1, placement(1, dir_id.clone(), file_name.clone(), first_hash)))).await.unwrap();
        let after_losing_write: Vec<(String, i64)> = sqlx::query_as("SELECT content_hash,reference_count FROM user_blob_reference WHERE user_id=$1 ORDER BY content_hash").bind(&user_id).fetch_all(&pool).await.unwrap();
        assert_eq!(after_losing_write, replaced_references, "a losing LWW placement must not produce a quota delta");

        let fresh = repository
            .load_changes(&user_id, &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("fresh-path-reader").unwrap(), cursor: None })
            .await
            .unwrap();
        assert_eq!(fresh.mutations.len(), 1, "only canonical physical-path state should remain");
        assert!(matches!(fresh.mutations[0].body, MutationBody::Placement { value: PlacementState::Present { book_id, .. }, .. } if book_id == BookId::new(2)));

        let stale_other_occupant_removal = change(4, vacancy(1, dir_id.clone(), file_name.clone()));
        repository
            .store_changes(&user_id, &PushMutationsRequest { library_id: library_id.clone(), replica_id: client_id, mutations: vec![stale_other_occupant_removal] })
            .await
            .unwrap();
        let after = repository
            .load_changes(&user_id, &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("second-path-reader").unwrap(), cursor: None })
            .await
            .unwrap();
        assert_eq!(after.mutations.len(), 1);
        assert!(matches!(after.mutations[0].body, MutationBody::Placement { value: PlacementState::Present { book_id, .. }, .. } if book_id == BookId::new(2)), "a removal for a displaced book must not alter current path state");
        let after_displaced_removal: Vec<(String, i64)> = sqlx::query_as("SELECT content_hash,reference_count FROM user_blob_reference WHERE user_id=$1 ORDER BY content_hash").bind(&user_id).fetch_all(&pool).await.unwrap();
        assert_eq!(after_displaced_removal, replaced_references, "a rejected occupant transition must not alter blob references");

        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), "path-writer", change(4, vacancy(2, dir_id, file_name)))).await.unwrap();
        let live_references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_blob_reference WHERE user_id=$1").bind(&user_id).fetch_one(&pool).await.unwrap();
        assert_eq!(live_references, 0);
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn conditional_placement_batch_matches_replica_sequence_serial_execution() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("conditional-batch-user-{suffix}");
        let batch_library = LibraryId::parse(&format!("conditional-batch-{suffix}")).unwrap();
        let serial_library = LibraryId::parse(&format!("conditional-serial-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        for library in [&batch_library, &serial_library] {
            sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        }
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let actor = ClientId::parse("conditional-writer").unwrap();
        let dir_id = DirId::parse("__root__").unwrap();
        let file_name = FileName::parse("conditional.epub").unwrap();
        let hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        let mut claim = change(1, placement(1, dir_id.clone(), file_name.clone(), hash.clone()));
        claim.changed_at = millis(10);
        let mut mismatched_release = change(2, vacancy(2, dir_id.clone(), file_name.clone()));
        mismatched_release.changed_at = millis(20);

        let batch_response = repository.store_changes(
            &user_id,
            &PushMutationsRequest { library_id: batch_library.clone(), replica_id: actor.clone(), mutations: vec![mismatched_release.clone(), claim.clone()] },
        ).await.unwrap();
        assert_eq!(batch_response.accepted.len(), 2);

        repository.store_changes(&user_id, &push(&user_id, serial_library.clone(), actor.as_str(), claim)).await.unwrap();
        repository.store_changes(&user_id, &push(&user_id, serial_library.clone(), actor.as_str(), mismatched_release)).await.unwrap();

        for library in [&batch_library, &serial_library] {
            let pulled = repository.load_changes(
                &user_id,
                &PullStateQuery { library_id: library.clone(), replica_id: ClientId::parse("conditional-reader").unwrap(), cursor: None },
            ).await.unwrap();
            assert_eq!(pulled.mutations.len(), 1);
            assert!(matches!(&pulled.mutations[0].body, MutationBody::Placement { value: PlacementState::Present { book_id, .. }, .. } if *book_id == BookId::new(1)));
        }
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn same_millisecond_placement_removal_then_claim_follows_replica_sequence() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("same-ms-placement-user-{suffix}");
        let library_id = LibraryId::parse(&format!("same-ms-placement-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let replica = ClientId::parse("same-ms-scanner").unwrap();
        let dir_id = DirId::parse("__root__").unwrap();
        let file_name = FileName::parse("replacement.epub").unwrap();
        let first_hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        let second_hash = ContentHash::parse(&"b".repeat(64)).unwrap();
        let mut first_claim = change(1, placement(1, dir_id.clone(), file_name.clone(), first_hash));
        first_claim.changed_at = millis(10);
        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), replica.as_str(), first_claim)).await.unwrap();

        let mut removal = change(2, vacancy(1, dir_id.clone(), file_name.clone()));
        removal.changed_at = millis(20);
        let mut replacement = change(3, placement(2, dir_id, file_name, second_hash.clone()));
        replacement.changed_at = millis(20);
        repository.store_changes(
            &user_id,
            &PushMutationsRequest { library_id: library_id.clone(), replica_id: replica, mutations: vec![replacement, removal] },
        ).await.unwrap();

        let pulled = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("same-ms-reader").unwrap(), cursor: None },
        ).await.unwrap();
        assert_eq!(pulled.mutations.len(), 1);
        assert!(matches!(&pulled.mutations[0].body, MutationBody::Placement { value: PlacementState::Present { book_id, content_hash }, .. } if *book_id == BookId::new(2) && content_hash == &second_hash));
        let references: Vec<(String, i64)> = sqlx::query_as("SELECT content_hash,reference_count FROM user_blob_reference WHERE user_id=$1 ORDER BY content_hash").bind(&user_id).fetch_all(&pool).await.unwrap();
        assert_eq!(references, vec![(second_hash.into_string(), 1)]);
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn cursor_ahead_fails_loudly_and_rolls_back_every_exchange_path() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("cursor-ahead-user-{suffix}");
        let library_id = LibraryId::parse(&format!("cursor-ahead-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let replica = ClientId::parse("cursor-ahead-writer").unwrap();
        repository.store_changes(&user_id, &push(&user_id, library_id.clone(), replica.as_str(), change(1, reading_body(1, "epubcfi(/6/2)", 10.0)))).await.unwrap();
        let head: i64 = sqlx::query_scalar("SELECT current_revision FROM sync_library_revision WHERE library_id=$1").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        let ahead = LibraryRevision::new(head as u64 + 1).unwrap();

        let pull = repository.load_changes(
            &user_id,
            &PullStateQuery { library_id: library_id.clone(), replica_id: replica.clone(), cursor: Some(ahead) },
        ).await;
        assert!(matches!(pull, Err(SyncError::CursorAhead)));

        let atomic = repository.exchange(
            &user_id,
            &SyncExchangeRequest {
                library_id: library_id.clone(),
                replica_id: replica.clone(),
                mutations: vec![change(2, reading_body(2, "epubcfi(/6/4)", 20.0))],
                cursor: Some(ahead),
            },
        ).await;
        assert!(matches!(atomic, Err(SyncError::CursorAhead)), "unexpected atomic result: {atomic:?}");
        let atomic_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_reading_position_state WHERE library_id=$1 AND book_id='2'").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(atomic_rows, 0, "the atomic fast path must reject before writing");

        let transactional = repository.exchange(
            &user_id,
            &SyncExchangeRequest {
                library_id: library_id.clone(),
                replica_id: replica,
                mutations: vec![change(
                    3,
                    MutationBody::Metadata {
                        book_id: BookId::new(3),
                        value: metadata("Rolled back", Vec::new(), ""),
                    },
                )],
                cursor: Some(ahead),
            },
        ).await;
        assert!(matches!(transactional, Err(SyncError::CursorAhead)), "unexpected transactional result: {transactional:?}");
        let transactional_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_book_metadata_state WHERE library_id=$1 AND book_id='3'").bind(library_id.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!(transactional_rows, 0, "the general exchange transaction must roll back");

        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn scanner_sized_page_is_batched_and_duplicate_keys_converge() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("batch-user-{suffix}");
        let library_id = LibraryId::parse(&format!("batch-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let child = DirId::parse("batch-child").unwrap();
        let content_hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        let mut mutations = vec![change(
            1,
            MutationBody::Directory {
                dir_id: child.clone(),
                value: DirectoryState::Present { parent_id: DirId::parse("__root__").unwrap(), name: FileName::parse("Batch").unwrap() },
            },
        )];
        for index in 1..=120_u64 {
            let base = mutations.len() as u64 + 1;
            mutations.push(change(
                base,
                MutationBody::BookLifecycle {
                    book_id: BookId::new(index),
                    value: BookLifecycleState::Present {
                        metadata: metadata(format!("Book {index}"), vec![author("Author")], ""),
                        added_at: millis(1),
                    },
                },
            ));
            mutations.push(change(base + 1, placement(index, child.clone(), FileName::parse(&format!("book-{index}.epub")).unwrap(), content_hash.clone())));
        }
        let first_reading = mutations.len() as u64 + 1;
        mutations.push(change(first_reading, reading_body(1, "epubcfi(/6/2)", 10.0)));
        mutations.push(change(first_reading + 1, reading_body(1, "epubcfi(/6/4)", 20.0)));
        let mutation_count = mutations.len();

        let response = repository
            .store_changes(
                &user_id,
                &PushMutationsRequest { library_id: library_id.clone(), replica_id: ClientId::parse("batch-writer").unwrap(), mutations },
            )
            .await
            .unwrap();
        assert!(response.rejected.is_empty());
        assert_eq!(response.accepted.len(), mutation_count);

        let (lifecycles, placements, metadata, readings): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
                (SELECT COUNT(*) FROM sync_book_lifecycle_state WHERE library_id=$1),
                (SELECT COUNT(*) FROM sync_book_placement_state WHERE library_id=$1),
                (SELECT COUNT(*) FROM sync_book_metadata_state WHERE library_id=$1),
                (SELECT COUNT(*) FROM sync_reading_position_state WHERE library_id=$1)",
        )
        .bind(library_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((lifecycles, placements, metadata, readings), (120, 120, 0, 1));
        let (location, progress): (String, f32) = sqlx::query_as("SELECT cfi,progress FROM sync_reading_position_state WHERE library_id=$1 AND book_id='1'")
            .bind(library_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(location, "epubcfi(/6/4)");
        assert_eq!(progress, 20.0);
        let reference_count: i64 = sqlx::query_scalar("SELECT reference_count FROM user_blob_reference WHERE user_id=$1 AND content_hash=$2")
            .bind(&user_id)
            .bind(content_hash.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(reference_count, 120);
        let (current, reading, other): (i64, i64, i64) = sqlx::query_as("SELECT current_revision,reading_revision,other_revision FROM sync_library_revision WHERE library_id=$1")
            .bind(library_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        let (maximum_reading, maximum_other): (i64, i64) = sqlx::query_as(
            "SELECT
                COALESCE((SELECT MAX(server_seq) FROM sync_reading_position_state WHERE library_id=$1), 0),
                GREATEST(
                    COALESCE((SELECT MAX(server_seq) FROM sync_directory_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_book_lifecycle_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_book_placement_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_annotation_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_bookmark_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_book_link_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_book_opened_state WHERE library_id=$1), 0),
                    COALESCE((SELECT MAX(server_seq) FROM sync_book_metadata_state WHERE library_id=$1), 0)
                )",
        )
            .bind(library_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((reading, other), (maximum_reading, maximum_other), "statement triggers must preserve exact channel heads");
        assert_eq!(current, std::cmp::max(reading, other));
        assert!(reading > 0 && other > 0);
        let pulled = repository
            .load_changes(
                &user_id,
                &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("batch-reader").unwrap(), cursor: None },
            )
            .await
            .unwrap();
        assert!(!pulled.has_more);
        assert_eq!(pulled.next_cursor.map(|revision| revision.get() as i64), Some(current), "a full pull must checkpoint the exact statement-maintained head");
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn typed_storage_round_trips_every_operation_family() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("typed-roundtrip-user-{suffix}");
        let library_id = LibraryId::parse(&format!("typed-roundtrip-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES($1)").bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,user_id) VALUES($1,$2)").bind(library_id.as_str()).bind(&user_id).execute(&pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(pool.clone()));
        let child = DirId::parse("typed-child").unwrap();
        let root = DirId::parse("__root__").unwrap();
        let file = FileName::parse("typed.epub").unwrap();
        let annotation_id = uuid::Uuid::new_v4().to_string();
        let bookmark_id = uuid::Uuid::new_v4().to_string();
        let link_id = uuid::Uuid::new_v4().to_string();
        let operations = vec![
            MutationBody::Directory { dir_id: child.clone(), value: DirectoryState::Present { parent_id: root, name: FileName::parse("Typed").unwrap() } },
            MutationBody::BookLifecycle {
                book_id: BookId::new(1),
                value: BookLifecycleState::Present {
                    metadata: metadata("First", vec![author("First Author"), author("Second Contributor")], ""),
                    added_at: millis(1),
                },
            },
            MutationBody::BookLifecycle {
                book_id: BookId::new(2),
                value: BookLifecycleState::Present {
                    metadata: metadata("Second", vec![author("Second Author")], ""),
                    added_at: millis(1),
                },
            },
            MutationBody::BookLifecycle { book_id: BookId::new(3), value: BookLifecycleState::Deleted },
            placement(1, child, file, ContentHash::parse(&"a".repeat(64)).unwrap()),
            reading_body(1, "epubcfi(/6/2)", 12.5),
            MutationBody::Annotation {
                annotation_id,
                value: AnnotationState {
                    book_id: BookId::new(1),
                    anchor: AnnotationAnchor::pdf(PdfAnnotationAnchor { page_index: 2, rects: vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.4).unwrap()], fallback_cfi: None }).unwrap(),
                    exact_text: "quoted text".to_string(),
                    prefix: Some("before".to_string()),
                    suffix: Some("after".to_string()),
                    style: AnnotationStyle::Highlight,
                    color: "yellow".to_string(),
                    note: "note".to_string(),
                    created_at: millis(1),
                    modified_at: millis(2),
                    deleted: false,
                },
            },
            MutationBody::Bookmark { bookmark_id, value: BookmarkState { book_id: BookId::new(1), cfi: "epubcfi(/6/4)".to_string(), label: "Here".to_string(), created_at: millis(1), modified_at: millis(1), deleted: false } },
            MutationBody::BookLink { link_id, value: BookLinkState { source_book_id: BookId::new(1), target_book_id: BookId::new(2), relation: "responds to".to_string(), note: String::new(), created_at: millis(1), modified_at: millis(1), deleted: false } },
            MutationBody::LastOpened { book_id: BookId::new(1), last_read: millis(10) },
            MutationBody::Metadata {
                book_id: BookId::new(1),
                value: metadata(
                    "Renamed",
                    vec![author("Le Guin, Ursula K."), author("Jane Doe")],
                    "Updated description",
                ),
            },
        ];
        let mutations: Vec<_> = operations.iter().cloned().enumerate().map(|(index, op)| change(index as u64 + 1, op)).collect();
        let response = repository
            .store_changes(&user_id, &PushMutationsRequest { library_id: library_id.clone(), replica_id: ClientId::parse("typed-writer").unwrap(), mutations })
            .await
            .unwrap();
        assert!(response.rejected.is_empty());
        assert_eq!(response.accepted.len(), operations.len());
        let pulled = repository
            .load_changes(&user_id, &PullStateQuery { library_id: library_id.clone(), replica_id: ClientId::parse("typed-reader").unwrap(), cursor: None })
            .await
            .unwrap();
        assert_eq!(pulled.mutations.iter().map(|change| change.body.clone()).collect::<Vec<_>>(), operations);
        let json_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema=current_schema() AND table_name LIKE 'sync_%' AND data_type IN ('json','jsonb')",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(json_columns, 0);
        let obsolete_clock_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema=current_schema() AND table_name LIKE 'sync_%_state' AND column_name IN ('hlc_pt','hlc_l')",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(obsolete_clock_columns, 0);
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn populated_legacy_event_history_migrates_to_canonical_typed_state() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(2).connect(&database_url).await.unwrap();
        let schema = format!("typed_migration_{}", uuid::Uuid::new_v4().simple());
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql(&format!("CREATE SCHEMA {schema}; SET search_path TO {schema}, public;")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE users(id TEXT PRIMARY KEY);
             CREATE TABLE libraries(id TEXT PRIMARY KEY,user_id TEXT NOT NULL REFERENCES users(id));
             CREATE TABLE library_events(
                 library_id TEXT NOT NULL REFERENCES libraries(id),
                 user_id TEXT NOT NULL REFERENCES users(id),
                 event_type TEXT NOT NULL,
                 entity_id TEXT NOT NULL,
                 data JSONB NOT NULL,
                 client_id TEXT NOT NULL,
                 timestamp BIGINT NOT NULL,
                 client_seq BIGINT NOT NULL,
                 hlc_pt BIGINT NOT NULL,
                 hlc_l BIGINT NOT NULL,
                 version_rank SMALLINT NOT NULL,
                 event_id TEXT NOT NULL,
                 server_seq BIGSERIAL PRIMARY KEY
             );
             INSERT INTO users VALUES('migration-user');
             INSERT INTO libraries VALUES('migration-library','migration-user');
             INSERT INTO library_events(library_id,user_id,event_type,entity_id,data,client_id,timestamp,client_seq,hlc_pt,hlc_l,version_rank,event_id) VALUES
               ('migration-library','migration-user','book_dir_added','1','{\"dir_id\":\"root\",\"file_name\":\"same.epub\",\"content_hash\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}','legacy-a',1,1,1,0,0,'00000000-0000-4000-8000-000000000001'),
               ('migration-library','migration-user','book_dir_added','2','{\"dir_id\":\"root\",\"file_name\":\"same.epub\",\"content_hash\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"}','legacy-b',2,2,2,0,0,'00000000-0000-4000-8000-000000000002'),
               ('migration-library','migration-user','book_dir_removed','2','{\"dir_id\":\"root\",\"file_name\":\"same.epub\"}','legacy-b',3,3,3,0,1,'00000000-0000-4000-8000-000000000003'),
               ('migration-library','migration-user','book_added','9','{\"title\":\"Legacy\",\"added_at\":1}','legacy-a',4,4,4,0,0,'00000000-0000-4000-8000-000000000004'),
               ('migration-library','migration-user','book_deleted','9','{}','legacy-b',5,5,5,0,1,'00000000-0000-4000-8000-000000000005');",
        ).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0023_typed_sync_state.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0024_sync_payload_integrity.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0025_efficient_sync_payload_integrity.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0026_flatten_typed_sync_state.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0027_wall_clock_versions.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0028_library_sync_revision.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0029_split_sync_revision_channels.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0030_statement_revision_heads.sql")).execute(&mut *connection).await.unwrap();

        let lifecycle: (String, bool) = sqlx::query_as("SELECT book_id,deleted FROM sync_book_lifecycle_state").fetch_one(&mut *connection).await.unwrap();
        assert_eq!(lifecycle,("9".to_string(),true));
        let placement: (String, String, String, Option<String>, bool) = sqlx::query_as(
            "SELECT book_id,dir_id,file_name,content_hash,present FROM sync_book_placement_state",
        ).fetch_one(&mut *connection).await.unwrap();
        assert_eq!(placement, ("2".to_string(), "root".to_string(), "same.epub".to_string(), None, false));
        let migrated_revision: i64 = sqlx::query_scalar("SELECT current_revision FROM sync_library_revision WHERE library_id='migration-library'").fetch_one(&mut *connection).await.unwrap();
        let maximum_state_revision: i64 = sqlx::query_scalar(
            "SELECT GREATEST(
                COALESCE((SELECT MAX(server_seq) FROM sync_book_lifecycle_state), 0),
                COALESCE((SELECT MAX(server_seq) FROM sync_book_placement_state), 0)
             )",
        ).fetch_one(&mut *connection).await.unwrap();
        assert_eq!(migrated_revision, maximum_state_revision, "migration must backfill the exact committed library watermark");
        let (reading_revision, other_revision): (i64, i64) = sqlx::query_as(
            "SELECT reading_revision, other_revision FROM sync_library_revision WHERE library_id='migration-library'",
        ).fetch_one(&mut *connection).await.unwrap();
        assert_eq!(reading_revision, 0);
        assert_eq!(other_revision, maximum_state_revision, "legacy non-reading state must backfill the general channel");
        let (row_revision_triggers, statement_revision_triggers): (i64, i64) = sqlx::query_as(
            "SELECT
                COUNT(*) FILTER (WHERE (trigger.tgtype & 1) = 1),
                COUNT(*) FILTER (WHERE (trigger.tgtype & 1) = 0)
             FROM pg_trigger AS trigger
             JOIN pg_class AS relation ON relation.oid=trigger.tgrelid
             JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace
             WHERE namespace.nspname=$1
               AND relation.relname LIKE 'sync_%_state'
               AND trigger.tgname LIKE 'advance_library_sync_revision_%'
               AND NOT trigger.tgisinternal",
        )
            .bind(&schema)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(row_revision_triggers, 0, "the migration must remove all per-row revision triggers");
        assert_eq!(statement_revision_triggers, 18, "each typed state table needs separate INSERT and UPDATE statement triggers");
        sqlx::raw_sql(
            "CREATE TABLE live_book_path(marker INTEGER);
             CREATE TABLE user_blob_reference(
                 user_id TEXT NOT NULL,
                 content_hash TEXT NOT NULL,
                 reference_count BIGINT NOT NULL,
                 PRIMARY KEY(user_id,content_hash)
             );
             UPDATE sync_book_placement_state
             SET content_hash=repeat('c',64),present=TRUE
             WHERE library_id='migration-library';
             INSERT INTO user_blob_reference VALUES('migration-user',repeat('c',64),99);",
        ).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0031_canonical_placement_references.sql")).execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0032_book_lifecycle_initial_metadata.sql")).execute(&mut *connection).await.unwrap();
        sqlx::query("UPDATE sync_book_lifecycle_state SET title='Legacy',author='',added_at=1700000000000000,deleted=FALSE,changed_at=1700000000000000 WHERE library_id='migration-library'").execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0033_normalize_sync_timestamps.sql")).execute(&mut *connection).await.unwrap();
        sqlx::query("UPDATE sync_book_placement_state SET version_rank=1 WHERE library_id='migration-library'").execute(&mut *connection).await.unwrap();
        sqlx::raw_sql(include_str!("../../server-database/migrations/0034_conditional_placement_rank.sql")).execute(&mut *connection).await.unwrap();
        let normalized_times: (i64, i64) = sqlx::query_as("SELECT added_at,changed_at FROM sync_book_lifecycle_state WHERE library_id='migration-library'").fetch_one(&mut *connection).await.unwrap();
        assert_eq!(normalized_times, (1_700_000_000_000, 1_700_000_000_000));
        let placement_rank: i16 = sqlx::query_scalar("SELECT version_rank FROM sync_book_placement_state WHERE library_id='migration-library'").fetch_one(&mut *connection).await.unwrap();
        assert_eq!(placement_rank, 0);
        let rebuilt_reference_count: i64 = sqlx::query_scalar("SELECT reference_count FROM user_blob_reference WHERE user_id='migration-user' AND content_hash=repeat('c',64)").fetch_one(&mut *connection).await.unwrap();
        assert_eq!(rebuilt_reference_count, 1, "the migration must rebuild reference counts from canonical placement state");
        let obsolete_placement_projection: Option<String> = sqlx::query_scalar("SELECT to_regclass('live_book_path')::TEXT").fetch_one(&mut *connection).await.unwrap();
        assert!(obsolete_placement_projection.is_none());
        let old_envelope: Option<String> = sqlx::query_scalar("SELECT to_regclass('sync_state')::TEXT").fetch_one(&mut *connection).await.unwrap();
        assert!(old_envelope.is_none());
        let json_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema=$1 AND table_name LIKE 'sync_%' AND data_type IN ('json','jsonb')",
        ).bind(&schema).fetch_one(&mut *connection).await.unwrap();
        assert_eq!(json_columns, 0);
        let obsolete_clock_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema=$1 AND table_name LIKE 'sync_%_state' AND column_name IN ('hlc_pt','hlc_l')",
        ).bind(&schema).fetch_one(&mut *connection).await.unwrap();
        assert_eq!(obsolete_clock_columns, 0);
        sqlx::raw_sql(&format!("SET search_path TO public; DROP SCHEMA {schema} CASCADE;")).execute(&mut *connection).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn garbage_collection_coordination_is_limited_to_placement_changes() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let maintenance_pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        let sync_pool = PgPoolOptions::new().max_connections(4).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(sync_pool.clone())).await.unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user_id = format!("gc-scope-user-{suffix}");
        let library_id = LibraryId::parse(&format!("gc-scope-library-{suffix}")).unwrap();
        sqlx::query("INSERT INTO users(id) VALUES ($1)").bind(&user_id).execute(&sync_pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES ($1, $2, 'GC scope')").bind(library_id.as_str()).bind(&user_id).execute(&sync_pool).await.unwrap();
        let repository = PostgresSyncRepository::new(PostgresDatabase::from_pool(sync_pool.clone()));

        let mut maintenance = maintenance_pool.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(MAINTENANCE_LOCK_NAME).execute(&mut *maintenance).await.unwrap();

        let reading = change(1, reading_body(1, "epubcfi(/6/2)", 10.0));
        tokio::time::timeout(Duration::from_secs(2), repository.store_changes(&user_id, &push(&user_id, library_id.clone(), "gc-scope-reading", reading)))
            .await
            .expect("reading position must not wait for blob GC")
            .unwrap();

        let placement = change(
            1,
            placement(
                1,
                DirId::parse("__root__").unwrap(),
                FileName::parse("book.epub").unwrap(),
                ContentHash::parse(&"a".repeat(64)).unwrap(),
            ),
        );
        let placement_repository = repository.clone();
        let placement_user = user_id.clone();
        let placement_library = library_id.clone();
        let mut waiter = tokio::spawn(async move { placement_repository.store_changes(&placement_user, &push(&placement_user, placement_library, "gc-scope-placement", placement)).await });
        assert!(tokio::time::timeout(Duration::from_millis(150), &mut waiter).await.is_err(), "placement push must wait while GC owns the liveness snapshot");
        maintenance.commit().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), waiter).await.unwrap().unwrap().unwrap();

        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&sync_pool).await.unwrap();
    }
}

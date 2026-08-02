use crate::{assets, sync_store, PostgresDatabase};
use async_trait::async_trait;
use server_core::{AssetError, AssetRepository, BookUploadAdmission, BookUploadReservation, GcLease, MaintenanceCoordinator, SyncError, SyncRepository};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{ContentHash, PullChangesResponse, PullQuery, PushChangesRequest, PushChangesResponse};

const MAINTENANCE_LOCK_NAME: &str = "bokheim:global-maintenance";

#[derive(Clone)]
pub struct PostgresSyncRepository {
    pool: PgPool,
}

impl PostgresSyncRepository {
    pub fn new(database: PostgresDatabase) -> Self {
        Self { pool: database.pool().clone() }
    }
}

fn internal(error: impl std::fmt::Display) -> SyncError {
    SyncError::Internal(error.to_string())
}

fn asset_internal(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

#[async_trait]
impl AssetRepository for PostgresSyncRepository {
    async fn authorized_content_hashes(&self, user_id: &str, library_id: &sync_common::LibraryId, candidates: &[ContentHash]) -> Result<Option<HashSet<ContentHash>>, AssetError> {
        sync_store::authorized_content_hashes(&self.pool, user_id, library_id, candidates).await.map_err(asset_internal)
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

    async fn upsert_library(&self, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibrarySummary>, SyncError> {
        sync_store::upsert_library(&self.pool, user_id, request).await.map_err(internal)
    }

    async fn ensure_library_owner(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, SyncError> {
        sync_store::ensure_library(&self.pool, user_id, library_id).await.map_err(internal)
    }

    async fn store_changes(&self, user_id: &str, request: &PushChangesRequest) -> Result<PushChangesResponse, SyncError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        // Shared across every server process. GC takes the exclusive form and
        // therefore holds a stable live-reference set through file deletion.
        sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1, 0))").bind(MAINTENANCE_LOCK_NAME).execute(&mut *tx).await.map_err(internal)?;
        // Serialize server-sequence allocation and commit order per library.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(request.library_id.as_str()).execute(&mut *tx).await.map_err(internal)?;
        let result = sync_store::insert_changes(&mut tx, user_id, &request.library_id, &request.client_id, &request.changes).await.map_err(internal)?;
        assets::reconcile_user_book_charges(&mut tx, user_id).await.map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(PushChangesResponse { handled_local_seqs: result.handled_local_seqs, rejected: result.rejected })
    }

    async fn load_changes(&self, query: &PullQuery) -> Result<PullChangesResponse, SyncError> {
        let changes = sync_store::fetch_changes(&self.pool, &query.library_id, &query.client_id, query.since_server_seq).await.map_err(|error| match error {
            sync_store::FetchChangesError::UnsupportedEventType { .. } => SyncError::UpgradeRequired,
            other => internal(other),
        })?;
        Ok(PullChangesResponse { changes })
    }
}

struct PostgresGcLease {
    transaction: Option<Transaction<'static, Postgres>>,
    live: HashSet<ContentHash>,
}

#[async_trait]
impl GcLease for PostgresGcLease {
    fn live_content(&self) -> &HashSet<ContentHash> {
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
        let live = sync_store::live_blob_hashes(&mut transaction).await.map_err(internal)?;
        Ok(Box::new(PostgresGcLease { transaction: Some(transaction), live }))
    }
}

#[cfg(test)]
mod tests {
    use super::{PostgresSyncRepository, MAINTENANCE_LOCK_NAME};
    use crate::PostgresDatabase;
    use server_core::MaintenanceCoordinator;
    use sqlx::postgres::PgPoolOptions;
    use std::collections::BTreeSet;
    use std::time::Duration;
    use sync_common::SINGLETON_EVENT_TYPES;

    fn sql_string_literals(expression: &str) -> BTreeSet<String> {
        let mut literals = BTreeSet::new();
        let mut characters = expression.chars().peekable();
        while let Some(character) = characters.next() {
            if character != '\'' {
                continue;
            }
            let mut literal = String::new();
            loop {
                match characters.next() {
                    Some('\'') if characters.peek() == Some(&'\'') => {
                        characters.next();
                        literal.push('\'');
                    }
                    Some('\'') => break,
                    Some(character) => literal.push(character),
                    None => panic!("unterminated SQL string literal in index predicate: {expression}"),
                }
            }
            literals.insert(literal);
        }
        literals
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn singleton_index_predicate_matches_protocol() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();

        let (is_unique, predicate): (bool, Option<String>) = sqlx::query_as(
            "SELECT indisunique, pg_get_expr(indpred, indrelid)
             FROM pg_index
             WHERE indexrelid = to_regclass('idx_library_events_lww')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(is_unique, "singleton index must enforce uniqueness");
        let predicate = predicate.expect("singleton index must be partial");
        let actual = sql_string_literals(&predicate);
        let expected = SINGLETON_EVENT_TYPES.iter().map(|event_type| (*event_type).to_owned()).collect();
        assert_eq!(actual, expected, "PostgreSQL singleton index predicate differs from SyncCommon: {predicate}");
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
}

use crate::{assets, sync_store, PostgresDatabase};
use server_core::{AssetError, BookUploadAdmission, BookUploadReservation, GcReport, LibraryUpsertOutcome, SyncError};
use sqlx::{PgPool, Postgres, Transaction};
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
#[cfg(test)]
use sync_common::PushMutationsRequest;
use sync_common::{ContentHash, PullStateQuery, PullStateResponse, PushMutationsResponse, SyncExchangeRequest, SyncExchangeResponse};

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
        let row = sqlx::query(include_str!("sql/sync/authorize_owned_library.sql")).bind(library_id.as_str()).bind(user_id).fetch_optional(&mut **transaction).await.map_err(internal)?;
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

fn asset_internal(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

impl PostgresSyncRepository {
    pub(crate) async fn owns_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, AssetError> {
        sync_store::owns_library(&self.pool, user_id, library_id).await.map_err(asset_internal)
    }

    pub(crate) async fn negotiate_books(&self, user_id: &str, library_id: &sync_common::LibraryId, blobs: &[BlobManifestEntry]) -> Result<BlobManifestResponse, AssetError> {
        assets::negotiate_books(&self.pool, user_id, library_id, blobs).await
    }

    pub(crate) async fn claim_existing_book(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<(), AssetError> {
        assets::claim_existing_book(&self.pool, user_id, library_id, hash, size_bytes).await
    }

    pub(crate) async fn reserve_book_upload(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError> {
        assets::reserve_book_upload(&self.pool, user_id, library_id, hash, size_bytes).await
    }

    pub(crate) async fn finish_book_upload(&self, reservation: &BookUploadReservation, actual_size_bytes: u64) -> Result<(), AssetError> {
        assets::finish_book_upload(&self.pool, reservation, actual_size_bytes).await
    }

    pub(crate) async fn abort_book_upload(&self, reservation: &BookUploadReservation) -> Result<(), AssetError> {
        assets::abort_book_upload(&self.pool, reservation).await
    }
}

impl PostgresSyncRepository {
    pub async fn list_libraries(&self, user_id: &str) -> Result<Vec<LibrarySummary>, SyncError> {
        sync_store::list_libraries(&self.pool, user_id).await.map_err(internal)
    }

    pub async fn upsert_library(&self, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, SyncError> {
        let name = request.library_name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(SyncError::InvalidLibraryName);
        }
        sync_store::upsert_library(&self.pool, user_id, &UpsertLibraryRequest { library_name: name.to_owned(), ..request.clone() }).await.map_err(internal)
    }

    #[cfg(test)]
    pub(crate) async fn store_changes(&self, user_id: &str, request: &PushMutationsRequest) -> Result<PushMutationsResponse, SyncError> {
        let exchange_request = SyncExchangeRequest { library_id: request.library_id.clone(), replica_id: request.replica_id.clone(), mutations: request.mutations.clone(), cursor: None };
        self.exchange(user_id, &exchange_request).await.map(|response| response.push)
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

pub struct GarbageCollector {
    _repository: std::sync::Arc<PostgresSyncRepository>,
}

impl GarbageCollector {
    pub fn new(repository: std::sync::Arc<PostgresSyncRepository>) -> Self {
        Self { _repository: repository }
    }

    /// Destructive sweeping stays disabled until it has a persisted
    /// pending-deletion/recheck protocol. Sync and quota accounting do not
    /// depend on collection.
    pub async fn collect(&self) -> Result<GcReport, SyncError> {
        Ok(GcReport { blobs_scanned: 0, blobs_deleted: 0, thumbnails_scanned: 0, thumbnails_deleted: 0, bytes_reclaimed: 0 })
    }
}

#[cfg(test)]
mod tests {
    use sync_common::{MutationId, ReplicaSeq, UnixMillis, WireMutation};

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
            changed_at: UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        assert_eq!(mutation.value, [0xde, 0xad, 0xbe, 0xef]);
    }
}

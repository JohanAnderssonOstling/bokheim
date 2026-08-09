use sync_common::api::libraries::LibrarySummary;

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

#[cfg(any())]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sync_common::{BookId, ClientId, LibraryId, MutationBody, MutationId, ReadingPositionState, ReplicaSeq};

    struct RecordingRepository {
        owns_library: bool,
        stores: AtomicUsize,
        loads: AtomicUsize,
    }

    #[async_trait]
    impl SyncRepository for RecordingRepository {
        async fn list_libraries(&self, _user_id: &str) -> Result<Vec<LibrarySummary>, SyncError> {
            Ok(Vec::new())
        }

        async fn upsert_library(&self, _user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, SyncError> {
            Ok(Some(LibraryUpsertOutcome { library: LibrarySummary { library_id: request.library_id.clone(), library_name: request.library_name.clone() }, changed: true }))
        }

        async fn store_changes(&self, _user_id: &str, _request: &PushMutationsRequest) -> Result<PushMutationsResponse, SyncError> {
            self.stores.fetch_add(1, Ordering::SeqCst);
            if self.owns_library {
                Ok(PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() })
            } else {
                Err(SyncError::LibraryOwnershipConflict)
            }
        }

        async fn load_changes(&self, _user_id: &str, _query: &PullStateQuery) -> Result<PullStateResponse, SyncError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            if self.owns_library {
                Ok(PullStateResponse { mutations: Vec::new(), next_cursor: None, has_more: false })
            } else {
                Err(SyncError::LibraryOwnershipConflict)
            }
        }

        async fn exchange(&self, _user_id: &str, _request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
            self.stores.fetch_add(1, Ordering::SeqCst);
            self.loads.fetch_add(1, Ordering::SeqCst);
            if self.owns_library {
                Ok(SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull: PullStateResponse { mutations: Vec::new(), next_cursor: None, has_more: false } })
            } else {
                Err(SyncError::LibraryOwnershipConflict)
            }
        }
    }

    fn repository(owns_library: bool) -> Arc<RecordingRepository> {
        Arc::new(RecordingRepository { owns_library, stores: AtomicUsize::new(0), loads: AtomicUsize::new(0) })
    }

    fn pull_query() -> PullStateQuery {
        PullStateQuery { library_id: LibraryId::parse("library").unwrap(), replica_id: ClientId::parse("client").unwrap(), cursor: None }
    }

    #[tokio::test]
    async fn empty_push_is_handled_without_opening_a_repository_transaction() {
        let repository = repository(true);
        let service = SyncApplication::new(repository.clone());
        let response = service.push("user", PushMutationsRequest { library_id: LibraryId::parse("library").unwrap(), replica_id: ClientId::parse("client").unwrap(), mutations: Vec::new() }).await.unwrap();

        assert!(response.accepted.is_empty());
        assert_eq!(repository.stores.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn oversized_push_and_exchange_stop_before_the_repository() {
        let repository = repository(true);
        let service = SyncApplication::new(repository.clone());
        let mutation = StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "epubcfi(/6/2)".to_owned(), progress: 1.0 } },
            changed_at: sync_common::UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        let mutations = vec![mutation; sync_common::MAX_PUSH_MUTATIONS + 1];
        let library_id = LibraryId::parse("library").unwrap();
        let replica_id = ClientId::parse("client").unwrap();

        let push = service.push("user", PushMutationsRequest { library_id: library_id.clone(), replica_id: replica_id.clone(), mutations: mutations.clone() }).await;
        assert!(matches!(push, Err(SyncError::TooManyMutations)));
        let exchange = service.exchange("user", SyncExchangeRequest { library_id, replica_id, mutations, cursor: None }).await;
        assert!(matches!(exchange, Err(SyncError::TooManyMutations)));
        assert_eq!(repository.stores.load(Ordering::SeqCst), 0);
        assert_eq!(repository.loads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn duplicate_mutation_identity_is_rejected_before_the_repository() {
        let repository = repository(true);
        let service = SyncApplication::new(repository.clone());
        let mutation = StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "epubcfi(/6/2)".to_owned(), progress: 1.0 } },
            changed_at: sync_common::UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        let library_id = LibraryId::parse("library").unwrap();
        let replica_id = ClientId::parse("client").unwrap();
        let duplicate = vec![mutation.clone(), mutation];

        let push = service.push("user", PushMutationsRequest { library_id: library_id.clone(), replica_id: replica_id.clone(), mutations: duplicate.clone() }).await;
        assert!(matches!(push, Err(SyncError::AmbiguousMutationIdentity)));
        let exchange = service.exchange("user", SyncExchangeRequest { library_id, replica_id, mutations: duplicate, cursor: None }).await;
        assert!(matches!(exchange, Err(SyncError::AmbiguousMutationIdentity)));
        assert_eq!(repository.stores.load(Ordering::SeqCst), 0);
        assert_eq!(repository.loads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repository_authorizes_pull_using_the_authenticated_user() {
        let repository = repository(false);
        let service = SyncApplication::new(repository.clone());

        assert!(matches!(service.pull("authenticated", pull_query()).await, Err(SyncError::LibraryOwnershipConflict)));
        assert_eq!(repository.stores.load(Ordering::SeqCst), 0);
        assert_eq!(repository.loads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn repository_authorizes_exchange_using_the_authenticated_user() {
        let repository = repository(false);
        let service = SyncApplication::new(repository.clone());
        let query = pull_query();
        let request = SyncExchangeRequest { library_id: query.library_id, replica_id: query.replica_id, mutations: Vec::new(), cursor: query.cursor };

        assert!(matches!(service.exchange("authenticated", request).await, Err(SyncError::LibraryOwnershipConflict)));
        assert_eq!(repository.stores.load(Ordering::SeqCst), 1);
        assert_eq!(repository.loads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn library_catalogue_validates_and_canonicalizes_names() {
        let service = SyncApplication::new(repository(true));
        let library_id = LibraryId::parse("library").unwrap();
        let saved = service.upsert_library("user", UpsertLibraryRequest { library_id: library_id.clone(), library_name: "  History  ".to_string() }).await.unwrap();
        assert_eq!(saved.library.library_id, library_id);
        assert_eq!(saved.library.library_name, "History");

        let invalid = service.upsert_library("user", UpsertLibraryRequest { library_id: LibraryId::parse("other").unwrap(), library_name: "\n".to_string() }).await;
        assert!(matches!(invalid, Err(SyncError::InvalidLibraryName)));
    }

    #[tokio::test]
    async fn core_rejects_library_owned_by_another_user() {
        let repository = repository(false);
        let service = SyncApplication::new(repository.clone());

        assert!(matches!(service.pull("user", pull_query()).await, Err(SyncError::LibraryOwnershipConflict)));
        assert_eq!(repository.loads.load(Ordering::SeqCst), 1);
    }
}

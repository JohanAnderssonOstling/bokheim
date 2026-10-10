//! Storage-independent state synchronization orchestration.

use std::error::Error;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;
use library_replica::{StatePublication, coalesce_mutations_for_push, prepare_push};
use sync_common::{LibraryId, ReplicaSeq, MutationIssue, PullStateResponse, PushMutationsResponse, ReplicaId, SyncCursor};

pub type StoreError = Box<dyn Error + Send + Sync>;

/// Local: stores borrow the library's single connection on its own thread.
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + 'a>>;

/// Persistence required by state synchronization. Implementations own their
/// transaction semantics; the engine owns protocol and recovery policy.
pub trait SyncStore {
    fn cursor_recovery_pending(&self) -> StoreFuture<'_, bool>;
    fn begin_cursor_recovery(&self) -> StoreFuture<'_, ()>;
    fn complete_cursor_recovery(&self) -> StoreFuture<'_, usize>;
    fn pull_cursor(&self) -> StoreFuture<'_, SyncCursor>;
    fn pending_mutations(&self) -> StoreFuture<'_, Vec<StatePublication>>;
    fn acknowledge_mutations(&self, mutation_ids: &[ReplicaSeq]) -> StoreFuture<'_, ()>;
    /// Atomically applies the mutations and advances the cursor.
    fn apply_pull_page(&self, page: &PullStateResponse) -> StoreFuture<'_, ()>;
    /// Persists validated push acknowledgments and a pull page. Implementations
    /// may share a commit, but must retain acknowledgments if applying pull fails.
    fn apply_exchange_response(&self, ids: &[ReplicaSeq], page: &PullStateResponse) -> StoreFuture<'_, ()>;
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum SyncError {
    AuthenticationRequired,
    CursorRejected,
    Failed(String),
}

impl SyncError {
    fn failed(error: impl Display) -> Self {
        Self::Failed(error.to_string())
    }
}

impl Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::CursorRejected => formatter.write_str("the synchronization cursor was rejected after recovery"),
            Self::Failed(message) => message.fmt(formatter),
        }
    }
}

impl Error for SyncError {}

impl From<sync_transport::SyncRequestError> for SyncError {
    fn from(error: sync_transport::SyncRequestError) -> Self {
        match error {
            sync_transport::SyncRequestError::AuthenticationRequired => Self::AuthenticationRequired,
            sync_transport::SyncRequestError::CursorRejected => Self::CursorRejected,
            error => Self::failed(error),
        }
    }
}

#[derive(Debug, Default)]
pub struct SyncOutcome {
    pub mutation_issues: Vec<MutationIssue>,
}

pub struct SyncEngine<'a, S: SyncStore> {
    store: S,
    http_client: &'a reqwest::Client,
    library_id: LibraryId,
    replica_id: ReplicaId,
    credentials: Option<sync_transport::SyncCredentials>,
}

pub struct SyncEngineConfig<'a, S: SyncStore> {
    pub store: S,
    pub http_client: &'a reqwest::Client,
    pub library_id: LibraryId,
    pub replica_id: ReplicaId,
    pub credentials: Option<sync_transport::SyncCredentials>,
}

impl<'a, S: SyncStore> SyncEngine<'a, S> {
    pub fn new(config: SyncEngineConfig<'a, S>) -> Self {
        Self {
            store: config.store,
            http_client: config.http_client,
            library_id: config.library_id,
            replica_id: config.replica_id,
            credentials: config.credentials,
        }
    }

    /// Publish only committed local mutations. Exchange replies may contain a
    /// pull page, but an inactive library must neither apply nor advance it.
    pub async fn drain_outbox(&self) -> Result<SyncOutcome, SyncError> {
        let changes = coalesce_mutations_for_push(self.store.pending_mutations().await.map_err(SyncError::failed)?);
        let mut remaining = prepare_push(changes).map_err(SyncError::failed)?;
        let mut result = SyncOutcome { mutation_issues: remaining.untransmittable().iter().copied().map(MutationIssue::Untransmittable).collect() };
        while !remaining.is_empty() {
            let requested = remaining.next_batch();
            let creations = requested.iter().filter(|m| m.state.value.address.kind == "book_lifecycle").map(|m| (m.publisher_seq, m.state.value.address.entity_key.clone())).collect::<Vec<_>>();
            let ids = requested.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
            let credentials = self.current_credentials()?;
            let response = sync_transport::exchange_changes(self.http_client, &credentials, self.library_id, self.replica_id, requested, SyncCursor::default()).await?;
            result.mutation_issues.extend(self.apply_push_response_semantics(&ids, &response.push).await?);
            defer_rejected_creation_fields(&mut remaining, &creations, &response.push);
        }
        Ok(result)
    }

    pub async fn synchronize(&self) -> Result<SyncOutcome, SyncError> {
        if self.store.cursor_recovery_pending().await.map_err(SyncError::failed)? {
            self.resume_cursor_recovery().await?;
        }
        for attempt in 0..2 {
            let cursor = self.store.pull_cursor().await.map_err(SyncError::failed)?;
            match self.exchange_events_with_server(cursor).await {
                Ok(result) => return Ok(result),
                Err(SyncError::CursorRejected) if attempt == 0 => {
                    self.begin_cursor_recovery().await?;
                    self.resume_cursor_recovery().await?;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("cursor recovery retry must return")
    }

    pub async fn begin_cursor_recovery(&self) -> Result<(), SyncError> {
        self.store.begin_cursor_recovery().await.map_err(SyncError::failed)?;
        Ok(())
    }

    async fn resume_cursor_recovery(&self) -> Result<(), SyncError> {
        let mut result = SyncOutcome::default();
        let mut remaining = prepare_push(Vec::new()).map_err(SyncError::failed)?;
        let credentials = self.current_credentials()?;
        self.exchange_until_idle(&credentials, SyncCursor::default(), &mut remaining, &mut result).await?;
        let inserted = self.store.complete_cursor_recovery().await.map_err(SyncError::failed)?;
        log::warn!("Recovered a rejected synchronization cursor and queued {inserted} canonical cells for republishing");
        Ok(())
    }

    pub async fn exchange_events_with_server(&self, starting_cursor: SyncCursor) -> Result<SyncOutcome, SyncError> {
        let mut trace = sync_transport::PerformanceTrace::new("sync_exchange", "load_and_batch_outbox");
        log::debug!(target: "sync_performance", "trace_id={} library_id={} initial_cursor={:?} from_zero={}", trace.id(), self.library_id, starting_cursor, starting_cursor == SyncCursor::default());
        let changes = coalesce_mutations_for_push(self.store.pending_mutations().await.map_err(SyncError::failed)?);
        let mut remaining = prepare_push(changes).map_err(SyncError::failed)?;
        let untransmittable = remaining.untransmittable().to_vec();
        let mut result = SyncOutcome { mutation_issues: untransmittable.iter().copied().map(MutationIssue::Untransmittable).collect() };
        let credentials = self.current_credentials()?;
        trace.phase("pages");
        self.exchange_until_idle(&credentials, starting_cursor, &mut remaining, &mut result).await?;
        trace.finish(true);
        Ok(result)
    }

    async fn apply_push_response_semantics(&self, requested_ids: &[ReplicaSeq], response: &PushMutationsResponse) -> Result<Vec<MutationIssue>, SyncError> {
        let validated = sync_transport::validate_push_response(requested_ids, response).map_err(SyncError::failed)?;
        let completed = validated.completed_sequences();
        if !completed.is_empty() {
            self.store.acknowledge_mutations(&completed).await.map_err(SyncError::failed)?;
        }
        Ok(validated.issues)
    }

    async fn exchange_until_idle(&self, credentials: &sync_transport::SyncCredentials, mut cursor: SyncCursor, remaining: &mut sync_common::PushBatcher, result: &mut SyncOutcome) -> Result<(), SyncError> {
        let mut page_number = 0_u64;
        loop {
            page_number += 1;
            let mut trace = sync_transport::PerformanceTrace::new("sync_page", "prepare_request");
            let requested = remaining.next_batch();
            let creations = requested.iter().filter(|m| m.state.value.address.kind == "book_lifecycle").map(|m| (m.publisher_seq, m.state.value.address.entity_key.clone())).collect::<Vec<_>>();
            let requested_ids = requested.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
            log::debug!(target: "sync_performance", "trace_id={} library_id={} page={page_number} push_count={} cursor={cursor:?}", trace.id(), self.library_id, requested.len());
            trace.phase("http_roundtrip");
            let response = sync_transport::exchange_changes(self.http_client, credentials, self.library_id, self.replica_id, requested, cursor).await?;
            trace.phase("validate_response");
            log::debug!(target: "sync_performance", "trace_id={} accepted={} rejected={} pull_count={} has_more={}", trace.id(), requested_ids.len().saturating_sub(response.push.rejected.len()), response.push.rejected.len(), response.pull.mutations.len(), response.pull.has_more);
            let validated = sync_transport::validate_push_response(&requested_ids, &response.push).map_err(SyncError::failed)?;
            let completed = validated.completed_sequences();
            result.mutation_issues.extend(validated.issues);
            let has_more = response.pull.has_more;
            if let Err(error) = sync_transport::validate_pull_batch(&response.pull, cursor) {
                // Preserve the old failure policy: a malformed pull response
                // must not discard independently valid push acknowledgments.
                if !completed.is_empty() {
                    self.store.acknowledge_mutations(&completed).await.map_err(SyncError::failed)?;
                }
                return Err(SyncError::failed(error));
            }
            cursor = response.pull.next_cursor;
            trace.phase("apply_local_transaction");
            self.store.apply_exchange_response(&completed, &response.pull).await.map_err(SyncError::failed)?;
            defer_rejected_creation_fields(remaining, &creations, &response.push);
            trace.finish(true);
            if remaining.is_empty() && !has_more {
                return Ok(());
            }
        }
    }

    fn current_credentials(&self) -> Result<sync_transport::SyncCredentials, SyncError> {
        self.credentials.clone().ok_or(SyncError::AuthenticationRequired)
    }
}

fn defer_rejected_creation_fields(remaining: &mut sync_common::PushBatcher, creations: &[(ReplicaSeq, sync_common::StateKey)], response: &PushMutationsResponse) {
    let books = creations.iter().filter(|(id, _)| response.rejected.iter().any(|rejection| rejection.publisher_seq == *id)).filter_map(|(_, key)| key.book()).collect::<Vec<_>>();
    remaining.defer_book_fields(&books);
}

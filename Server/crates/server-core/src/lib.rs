use async_trait::async_trait;
use bokheim_app_contract::preferences::{AccountPreferencePatch, AccountPreferences};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
use sync_common::{ContentHash, LibraryId, PullStateResponse, PullStateQuery, PushMutationsRequest, PushMutationsResponse, StateMutation, SyncExchangeRequest, SyncExchangeResponse};
use tokio::io::AsyncRead;

pub type AssetReader = Box<dyn AsyncRead + Unpin + Send>;

#[derive(Debug, thiserror::Error)]
pub enum PreferenceError {
    #[error("invalid account preference patch")]
    InvalidPatch,
    #[error("preference persistence failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait PreferenceRepository: Send + Sync {
    async fn initialize(&self, user_id: &str, seed: AccountPreferences) -> Result<AccountPreferences, PreferenceError>;
    async fn apply(&self, user_id: &str, patch: AccountPreferencePatch) -> Result<AccountPreferences, PreferenceError>;
}

#[async_trait]
pub trait PreferenceService: Send + Sync {
    async fn initialize(&self, user_id: &str, seed: AccountPreferences) -> Result<AccountPreferences, PreferenceError>;
    async fn apply(&self, user_id: &str, patch: AccountPreferencePatch) -> Result<AccountPreferences, PreferenceError>;
}

pub struct PreferenceApplication {
    repository: Arc<dyn PreferenceRepository>,
}

impl PreferenceApplication {
    pub fn new(repository: Arc<dyn PreferenceRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl PreferenceService for PreferenceApplication {
    async fn initialize(&self, user_id: &str, seed: AccountPreferences) -> Result<AccountPreferences, PreferenceError> {
        self.repository.initialize(user_id, seed).await
    }

    async fn apply(&self, user_id: &str, patch: AccountPreferencePatch) -> Result<AccountPreferences, PreferenceError> {
        self.repository.apply(user_id, patch).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Book,
    Thumbnail,
}

impl AssetKind {
    pub const fn max_bytes(self) -> u64 {
        match self {
            Self::Book => book_format::MAX_BOOK_BYTES,
            Self::Thumbnail => sync_common::MAX_THUMBNAIL_BYTES,
        }
    }
}

pub struct StoredAsset {
    pub reader: AssetReader,
    pub total_len: u64,
    pub offset: u64,
}

/// A live cover is identified by both its owner and the book content it
/// represents. Book blobs remain globally content-addressed, but covers are
/// deliberately not shared across accounts.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserThumbnail {
    pub user_id: String,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveAssets {
    books: HashSet<ContentHash>,
    thumbnails: HashSet<UserThumbnail>,
}

impl LiveAssets {
    pub fn new(books: HashSet<ContentHash>, thumbnails: HashSet<UserThumbnail>) -> Self {
        Self { books, thumbnails }
    }

    pub fn books(&self) -> &HashSet<ContentHash> {
        &self.books
    }

    pub fn thumbnails(&self) -> &HashSet<UserThumbnail> {
        &self.thumbnails
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub scanned: usize,
    pub deleted: usize,
    pub bytes_reclaimed: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset access is forbidden")]
    Forbidden,
    #[error("invalid asset input: {0}")]
    InvalidInput(String),
    #[error("asset bytes do not match the requested content hash")]
    HashMismatch,
    #[error("book uploads require an exact Content-Length")]
    LengthRequired,
    #[error("asset body length {actual} does not match the declared length {expected}")]
    LengthMismatch { expected: u64, actual: u64 },
    #[error("an upload for this user and book is already in progress")]
    UploadInProgress,
    #[error("book upload staging capacity is temporarily exhausted")]
    Busy,
    #[error("book storage quota exceeded: limit={limit}, used={used}, reserved={reserved}, requested={requested}")]
    QuotaExceeded { limit: u64, used: u64, reserved: u64, requested: u64 },
    #[error("asset exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("asset byte offset {offset} is beyond its {len}-byte length")]
    RangeNotSatisfiable { offset: u64, len: u64 },
    #[error("asset storage failed: {0}")]
    Storage(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageQuotaStatus {
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
}

impl StorageQuotaStatus {
    pub fn over_quota(&self) -> bool {
        self.used_bytes.saturating_add(self.reserved_bytes) > self.quota_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetStorageQuota {
    /// Caller-supplied identifier used to make retries safe.
    pub change_id: String,
    pub user_id: String,
    pub actor: String,
    pub quota_bytes: u64,
    pub reason: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageQuotaError {
    #[error("invalid storage quota change: {0}")]
    InvalidInput(String),
    #[error("user does not exist")]
    UserNotFound,
    #[error("quota change id was already used for a different change")]
    IdempotencyConflict,
    #[error("storage quota service failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait StorageQuotaService: Send + Sync {
    async fn set_quota(&self, change: SetStorageQuota) -> Result<StorageQuotaStatus, StorageQuotaError>;
}

#[async_trait]
pub trait AssetStore: Send + Sync {
    async fn initialize(&self) -> Result<(), AssetError>;
    /// Re-check durable storage without creating a missing storage root.
    /// Production readiness uses this to distinguish a healthy process from
    /// one whose asset mount has disappeared or become unwritable.
    async fn check_ready(&self) -> Result<(), AssetError>;
    /// `user_id` scopes thumbnails to an account. Implementations must not use
    /// it to partition book blobs, which are globally deduplicated by hash.
    async fn exists(&self, user_id: &str, kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError>;
    async fn existing(&self, user_id: &str, kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError>;
    async fn size(&self, user_id: &str, kind: AssetKind, hash: &ContentHash) -> Result<Option<u64>, AssetError>;
    async fn open_from(&self, user_id: &str, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError>;
    async fn put(&self, user_id: &str, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<u64, AssetError>;
    async fn sweep(&self, kind: AssetKind, live: &LiveAssets, cutoff: SystemTime) -> Result<SweepReport, AssetError>;
}

/// Persistence boundary used to authorize content-addressed assets without
/// exposing database technology to the application or HTTP layers.
#[async_trait]
pub trait AssetRepository: Send + Sync {
    async fn owns_library(&self, user_id: &str, library_id: &LibraryId) -> Result<bool, AssetError>;
    async fn negotiate_books(&self, user_id: &str, library_id: &LibraryId, blobs: &[BlobManifestEntry]) -> Result<BlobManifestResponse, AssetError>;
    async fn claim_existing_book(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<(), AssetError>;
    async fn reserve_book_upload(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError>;
    async fn finish_book_upload(&self, reservation: &BookUploadReservation, actual_size_bytes: u64) -> Result<(), AssetError>;
    async fn abort_book_upload(&self, reservation: &BookUploadReservation) -> Result<(), AssetError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookUploadReservation {
    pub id: String,
    pub user_id: String,
    pub library_id: LibraryId,
    pub hash: ContentHash,
    pub expected_size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookUploadAdmission {
    AlreadyOwned,
    Reserved(BookUploadReservation),
}

#[async_trait]
pub trait AssetService: Send + Sync {
    async fn exists(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError>;
    async fn existing(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError>;
    async fn negotiate_books(&self, user_id: &str, library_id: &LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AssetError>;
    async fn open_from(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError>;
    async fn put(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError>;
}

/// Asset use cases and library-ownership authorization. Raw storage remains an
/// adapter detail and is never exposed directly to HTTP.
pub struct AssetApplication {
    repository: Arc<dyn AssetRepository>,
    store: Arc<dyn AssetStore>,
}

impl AssetApplication {
    pub fn new(repository: Arc<dyn AssetRepository>, store: Arc<dyn AssetStore>) -> Self {
        Self { repository, store }
    }

    async fn require_library(&self, user_id: &str, library_id: &LibraryId) -> Result<(), AssetError> {
        if self.repository.owns_library(user_id, library_id).await? {
            Ok(())
        } else {
            Err(AssetError::Forbidden)
        }
    }

    async fn claim_existing_book(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
        let Some(size) = self.store.size(user_id, AssetKind::Book, hash).await? else {
            return Ok(None);
        };
        self.repository.claim_existing_book(user_id, library_id, hash, size).await?;
        Ok(Some(size))
    }
}

#[async_trait]
impl AssetService for AssetApplication {
    async fn exists(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError> {
        self.require_library(user_id, library_id).await?;
        if kind == AssetKind::Book {
            Ok(self.claim_existing_book(user_id, library_id, hash).await?.is_some())
        } else {
            self.store.exists(user_id, kind, hash).await
        }
    }

    async fn existing(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError> {
        self.require_library(user_id, library_id).await?;
        let present = self.store.existing(user_id, kind, hashes).await?;
        if kind == AssetKind::Book {
            let mut accessible = Vec::with_capacity(present.len());
            for hash in present {
                let Some(size) = self.store.size(user_id, kind, &hash).await? else {
                    continue;
                };
                match self.repository.claim_existing_book(user_id, library_id, &hash, size).await {
                    Ok(()) => accessible.push(hash),
                    Err(AssetError::QuotaExceeded { .. }) => {}
                    Err(error) => return Err(error),
                }
            }
            return Ok(accessible);
        }
        Ok(present)
    }

    async fn negotiate_books(&self, user_id: &str, library_id: &LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AssetError> {
        self.require_library(user_id, library_id).await?;
        self.repository.negotiate_books(user_id, library_id, &blobs).await
    }

    async fn open_from(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        self.require_library(user_id, library_id).await?;
        if kind == AssetKind::Book && self.claim_existing_book(user_id, library_id, hash).await?.is_none() {
            return Ok(None);
        }
        self.store.open_from(user_id, kind, hash, offset).await
    }

    async fn put(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        if kind == AssetKind::Thumbnail {
            self.store.put(user_id, kind, hash, expected_len, reader).await?;
            return Ok(());
        }
        let expected_len = expected_len.ok_or(AssetError::LengthRequired)?;
        if expected_len > kind.max_bytes() {
            return Err(AssetError::TooLarge { limit: kind.max_bytes() });
        }
        if self.claim_existing_book(user_id, library_id, hash).await?.is_some() {
            return Ok(());
        }
        let reservation = match self.repository.reserve_book_upload(user_id, library_id, hash, expected_len).await? {
            BookUploadAdmission::AlreadyOwned => return Ok(()),
            BookUploadAdmission::Reserved(reservation) => reservation,
        };
        match self.store.put(user_id, kind, hash, Some(expected_len), reader).await {
            Ok(actual_size) => self.repository.finish_book_upload(&reservation, actual_size).await,
            Err(error) => {
                if let Err(abort_error) = self.repository.abort_book_upload(&reservation).await {
                    return Err(AssetError::Storage(format!("{error}; additionally failed to release quota reservation: {abort_error}")));
                }
                Err(error)
            }
        }
    }
}

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

#[async_trait]
pub trait SyncService: Send + Sync {
    async fn list_libraries(&self, authenticated_user: &str) -> Result<Vec<LibrarySummary>, SyncError>;
    async fn upsert_library(&self, authenticated_user: &str, request: UpsertLibraryRequest) -> Result<LibraryUpsertOutcome, SyncError>;
    async fn push(&self, authenticated_user: &str, request: PushMutationsRequest) -> Result<PushMutationsResponse, SyncError>;
    async fn pull(&self, authenticated_user: &str, query: PullStateQuery) -> Result<PullStateResponse, SyncError>;
    async fn exchange(&self, authenticated_user: &str, request: SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError>;
}

#[async_trait]
pub trait SyncRepository: Send + Sync {
    async fn list_libraries(&self, user_id: &str) -> Result<Vec<LibrarySummary>, SyncError>;
    async fn upsert_library(&self, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibraryUpsertOutcome>, SyncError>;
    async fn store_changes(&self, user_id: &str, request: &PushMutationsRequest) -> Result<PushMutationsResponse, SyncError>;
    async fn load_changes(&self, user_id: &str, query: &PullStateQuery) -> Result<PullStateResponse, SyncError>;
    async fn exchange(&self, user_id: &str, request: &SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError>;
}

/// Synchronization use cases use the authenticated identity supplied by HTTP;
/// user identity is deliberately absent from the untrusted request body.
/// Persistence implementations atomically combine library ownership with each
/// storage operation through [`SyncRepository`], so authorization cannot race
/// the write or require a separate steady-state database round trip.
pub struct SyncApplication {
    repository: Arc<dyn SyncRepository>,
}

impl SyncApplication {
    pub fn new(repository: Arc<dyn SyncRepository>) -> Self {
        Self { repository }
    }
}

fn validate_mutation_batch(mutations: &[StateMutation]) -> Result<(), SyncError> {
    if mutations.len() > sync_common::MAX_PUSH_MUTATIONS {
        return Err(SyncError::TooManyMutations);
    }
    let mut identities = HashSet::with_capacity(mutations.len());
    if mutations.iter().any(|mutation| !identities.insert(mutation.mutation_id)) {
        return Err(SyncError::AmbiguousMutationIdentity);
    }
    Ok(())
}

#[async_trait]
impl SyncService for SyncApplication {
    async fn list_libraries(&self, authenticated_user: &str) -> Result<Vec<LibrarySummary>, SyncError> {
        self.repository.list_libraries(authenticated_user).await
    }

    async fn upsert_library(&self, authenticated_user: &str, request: UpsertLibraryRequest) -> Result<LibraryUpsertOutcome, SyncError> {
        let name = request.library_name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(SyncError::InvalidLibraryName);
        }
        let request = UpsertLibraryRequest { library_name: name.to_owned(), ..request };
        self.repository.upsert_library(authenticated_user, &request).await?.ok_or(SyncError::LibraryOwnershipConflict)
    }

    async fn push(&self, authenticated_user: &str, request: PushMutationsRequest) -> Result<PushMutationsResponse, SyncError> {
        validate_mutation_batch(&request.mutations)?;
        if request.mutations.is_empty() {
            return Ok(PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() });
        }
        self.repository.store_changes(authenticated_user, &request).await
    }

    async fn pull(&self, authenticated_user: &str, query: PullStateQuery) -> Result<PullStateResponse, SyncError> {
        self.repository.load_changes(authenticated_user, &query).await
    }

    async fn exchange(&self, authenticated_user: &str, request: SyncExchangeRequest) -> Result<SyncExchangeResponse, SyncError> {
        validate_mutation_batch(&request.mutations)?;
        self.repository.exchange(authenticated_user, &request).await
    }
}

#[async_trait]
pub trait GcLease: Send {
    fn live_assets(&self) -> &LiveAssets;
    async fn release(self: Box<Self>) -> Result<(), SyncError>;
}

#[async_trait]
pub trait MaintenanceCoordinator: Send + Sync {
    async fn begin_gc(&self) -> Result<Box<dyn GcLease>, SyncError>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub blobs_scanned: usize,
    pub blobs_deleted: usize,
    pub thumbnails_scanned: usize,
    pub thumbnails_deleted: usize,
    pub bytes_reclaimed: u64,
}

pub struct GarbageCollector {
    coordinator: std::sync::Arc<dyn MaintenanceCoordinator>,
    assets: std::sync::Arc<dyn AssetStore>,
}

impl GarbageCollector {
    pub fn new(coordinator: std::sync::Arc<dyn MaintenanceCoordinator>, assets: std::sync::Arc<dyn AssetStore>) -> Self {
        Self { coordinator, assets }
    }

    pub async fn collect(&self, cutoff: SystemTime) -> Result<GcReport, SyncError> {
        let lease = self.coordinator.begin_gc().await?;
        let books = self.assets.sweep(AssetKind::Book, lease.live_assets(), cutoff).await.map_err(|error| SyncError::Internal(error.to_string()))?;
        let thumbnails = self.assets.sweep(AssetKind::Thumbnail, lease.live_assets(), cutoff).await.map_err(|error| SyncError::Internal(error.to_string()))?;
        lease.release().await?;
        Ok(GcReport { blobs_scanned: books.scanned, blobs_deleted: books.deleted, thumbnails_scanned: thumbnails.scanned, thumbnails_deleted: thumbnails.deleted, bytes_reclaimed: books.bytes_reclaimed + thumbnails.bytes_reclaimed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sync_common::{BookId, ClientId, LibraryId, MutationBody, MutationId, ReadingPositionState, ReplicaSeq, StateMutation};

    struct RecordingRepository {
        owns_library: bool,
        stores: AtomicUsize,
        loads: AtomicUsize,
    }

    struct RecordingAssetRepository {
        owns_library: bool,
    }

    #[async_trait]
    impl AssetRepository for RecordingAssetRepository {
        async fn owns_library(&self, _user_id: &str, _library_id: &LibraryId) -> Result<bool, AssetError> {
            Ok(self.owns_library)
        }

        async fn negotiate_books(&self, _user_id: &str, _library_id: &LibraryId, blobs: &[BlobManifestEntry]) -> Result<BlobManifestResponse, AssetError> {
            Ok(BlobManifestResponse { upload: blobs.iter().map(|blob| blob.content_hash.clone()).collect(), ..BlobManifestResponse::default() })
        }

        async fn claim_existing_book(&self, _user_id: &str, _library_id: &LibraryId, _hash: &ContentHash, _size_bytes: u64) -> Result<(), AssetError> {
            Ok(())
        }

        async fn reserve_book_upload(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError> {
            Ok(BookUploadAdmission::Reserved(BookUploadReservation { id: "reservation".to_string(), user_id: user_id.to_string(), library_id: library_id.clone(), hash: hash.clone(), expected_size_bytes: size_bytes }))
        }

        async fn finish_book_upload(&self, _reservation: &BookUploadReservation, _actual_size_bytes: u64) -> Result<(), AssetError> {
            Ok(())
        }

        async fn abort_book_upload(&self, _reservation: &BookUploadReservation) -> Result<(), AssetError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingAssetStore {
        calls: AtomicUsize,
        present: HashSet<ContentHash>,
    }

    #[async_trait]
    impl AssetStore for RecordingAssetStore {
        async fn initialize(&self) -> Result<(), AssetError> {
            Ok(())
        }
        async fn check_ready(&self) -> Result<(), AssetError> {
            Ok(())
        }
        async fn exists(&self, _user_id: &str, _kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.present.contains(hash))
        }
        async fn existing(&self, _user_id: &str, _kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(hashes.into_iter().filter(|hash| self.present.contains(hash)).collect())
        }
        async fn size(&self, _user_id: &str, _kind: AssetKind, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.present.contains(hash).then_some(1))
        }
        async fn open_from(&self, _user_id: &str, _kind: AssetKind, _hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(StoredAsset { reader: Box::new(std::io::Cursor::new(Vec::new())), total_len: offset, offset }))
        }
        async fn put(&self, _user_id: &str, _kind: AssetKind, _hash: &ContentHash, expected_len: Option<u64>, _reader: AssetReader) -> Result<u64, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(expected_len.unwrap_or(0))
        }
        async fn sweep(&self, _kind: AssetKind, _live: &LiveAssets, _cutoff: SystemTime) -> Result<SweepReport, AssetError> {
            Ok(SweepReport::default())
        }
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
                Ok(SyncExchangeResponse {
                    push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                    pull: PullStateResponse { mutations: Vec::new(), next_cursor: None, has_more: false },
                })
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
        let request = SyncExchangeRequest {
            library_id: query.library_id,
            replica_id: query.replica_id,
            mutations: Vec::new(),
            cursor: query.cursor,
        };

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

    #[tokio::test]
    async fn asset_access_requires_library_ownership_but_not_a_live_reference() {
        let allowed = ContentHash::parse(&"a".repeat(64)).unwrap();
        let unreferenced = ContentHash::parse(&"b".repeat(64)).unwrap();
        let library = LibraryId::parse("library").unwrap();

        let store = Arc::new(RecordingAssetStore { calls: AtomicUsize::new(0), present: HashSet::from([allowed.clone(), unreferenced.clone()]) });
        let foreign = AssetApplication::new(Arc::new(RecordingAssetRepository { owns_library: false }), store.clone());
        assert!(matches!(foreign.exists("intruder", &library, AssetKind::Book, &allowed).await, Err(AssetError::Forbidden)));
        assert_eq!(store.calls.load(Ordering::SeqCst), 0, "unauthorized access must stop before storage");

        let owned = AssetApplication::new(Arc::new(RecordingAssetRepository { owns_library: true }), store.clone());
        assert!(owned.open_from("owner", &library, AssetKind::Book, &unreferenced, 0).await.unwrap().is_some());
        assert_eq!(store.calls.load(Ordering::SeqCst), 2, "an owned library may read a globally stored hash without a live metadata reference");

        let present = owned.existing("owner", &library, AssetKind::Book, vec![allowed.clone(), unreferenced.clone()]).await.unwrap();
        assert_eq!(present, vec![allowed, unreferenced]);
        assert_eq!(store.calls.load(Ordering::SeqCst), 5);
    }
}

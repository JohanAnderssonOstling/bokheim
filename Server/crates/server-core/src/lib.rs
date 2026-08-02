use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{ContentHash, LibraryId, PullChangesResponse, PullQuery, PushChangesRequest, PushChangesResponse};
use tokio::io::AsyncRead;

pub type AssetReader = Box<dyn AsyncRead + Unpin + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Book,
    Thumbnail,
}

impl AssetKind {
    pub const fn max_bytes(self) -> u64 {
        match self {
            Self::Book => 1024 * 1024 * 1024,
            Self::Thumbnail => 16 * 1024 * 1024,
        }
    }
}

pub struct StoredAsset {
    pub reader: AssetReader,
    pub total_len: u64,
    pub offset: u64,
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
    #[error("book storage quota exceeded: limit={limit}, used={used}, reserved={reserved}, requested={requested}")]
    QuotaExceeded { limit: u64, used: u64, reserved: u64, requested: u64 },
    #[error("asset exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("asset byte offset {offset} is beyond its {len}-byte length")]
    RangeNotSatisfiable { offset: u64, len: u64 },
    #[error("asset storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait AssetStore: Send + Sync {
    async fn initialize(&self) -> Result<(), AssetError>;
    async fn exists(&self, kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError>;
    async fn existing(&self, kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError>;
    async fn size(&self, kind: AssetKind, hash: &ContentHash) -> Result<Option<u64>, AssetError>;
    async fn open_from(&self, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError>;
    async fn put(&self, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<u64, AssetError>;
    async fn sweep(&self, kind: AssetKind, live: &HashSet<ContentHash>, cutoff: SystemTime) -> Result<SweepReport, AssetError>;
}

/// Persistence boundary used to authorize content-addressed assets without
/// exposing database technology to the application or HTTP layers.
#[async_trait]
pub trait AssetRepository: Send + Sync {
    /// Returns `None` when the user does not own the library. Otherwise returns
    /// the subset of candidates referenced by the library's current projection.
    async fn authorized_content_hashes(&self, user_id: &str, library_id: &LibraryId, candidates: &[ContentHash]) -> Result<Option<HashSet<ContentHash>>, AssetError>;
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
    async fn open_from(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError>;
    async fn put(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError>;
}

/// Asset use cases and library/reference authorization. Raw storage remains an
/// adapter detail and is never exposed directly to HTTP.
pub struct AssetApplication {
    repository: Arc<dyn AssetRepository>,
    store: Arc<dyn AssetStore>,
}

impl AssetApplication {
    pub fn new(repository: Arc<dyn AssetRepository>, store: Arc<dyn AssetStore>) -> Self {
        Self { repository, store }
    }

    async fn require_hash(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash) -> Result<(), AssetError> {
        let authorized = self.repository.authorized_content_hashes(user_id, library_id, std::slice::from_ref(hash)).await?.ok_or(AssetError::Forbidden)?;
        if authorized.contains(hash) {
            Ok(())
        } else {
            Err(AssetError::Forbidden)
        }
    }

    async fn claim_existing_book(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
        let Some(size) = self.store.size(AssetKind::Book, hash).await? else {
            return Ok(None);
        };
        self.repository.claim_existing_book(user_id, library_id, hash, size).await?;
        Ok(Some(size))
    }
}

#[async_trait]
impl AssetService for AssetApplication {
    async fn exists(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError> {
        self.require_hash(user_id, library_id, hash).await?;
        if kind == AssetKind::Book {
            Ok(self.claim_existing_book(user_id, library_id, hash).await?.is_some())
        } else {
            self.store.exists(kind, hash).await
        }
    }

    async fn existing(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError> {
        let authorized = self.repository.authorized_content_hashes(user_id, library_id, &hashes).await?.ok_or(AssetError::Forbidden)?;
        let candidates = hashes.into_iter().filter(|hash| authorized.contains(hash)).collect();
        let present = self.store.existing(kind, candidates).await?;
        if kind == AssetKind::Book {
            let mut accessible = Vec::with_capacity(present.len());
            for hash in present {
                let Some(size) = self.store.size(kind, &hash).await? else {
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

    async fn open_from(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        self.require_hash(user_id, library_id, hash).await?;
        if kind == AssetKind::Book && self.claim_existing_book(user_id, library_id, hash).await?.is_none() {
            return Ok(None);
        }
        self.store.open_from(kind, hash, offset).await
    }

    async fn put(&self, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError> {
        self.require_hash(user_id, library_id, hash).await?;
        if kind == AssetKind::Thumbnail {
            self.store.put(kind, hash, expected_len, reader).await?;
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
        match self.store.put(kind, hash, Some(expected_len), reader).await {
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

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub user_id: String,
    pub username: String,
}

/// A newly issued opaque server-side session. The raw token crosses the
/// boundary exactly once; persistence adapters store only its digest.
#[derive(Clone)]
pub struct IssuedSession {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: SessionRecord,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("username already taken")]
    UsernameTaken,
    #[error("invalid or expired invite")]
    InvalidInvite,
    #[error("forbidden")]
    Forbidden,
    #[error("password must be at least {min_characters} characters and at most {max_bytes} bytes")]
    InvalidPassword { min_characters: usize, max_bytes: usize },
    #[error("username must not be empty")]
    EmptyUsername,
    #[error("username must be 3-64 characters using letters, numbers, '.', '_' or '-'")]
    InvalidUsername,
    #[error("authentication service failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn bootstrap_admin(&self, password: Option<&str>) -> Result<(), AuthError>;
    async fn resolve_token(&self, token: &str) -> Result<SessionRecord, AuthError>;
    async fn login(&self, username: &str, password: &str) -> Result<IssuedSession, AuthError>;
    async fn logout(&self, token: &str) -> Result<(), AuthError>;
    async fn signup(&self, invite: &str, username: &str, password: &str) -> Result<IssuedSession, AuthError>;
    async fn create_invite(&self, note: Option<&str>, expires_in_days: Option<i64>) -> Result<(String, DateTime<Utc>), AuthError>;
    async fn verify_admin(&self, username: &str, password: &str) -> Result<(), AuthError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("authenticated user does not match the request")]
    Forbidden,
    #[error("library belongs to a different user")]
    LibraryOwnershipConflict,
    #[error("invalid library name")]
    InvalidLibraryName,
    #[error("stored synchronization data requires a newer client")]
    UpgradeRequired,
    #[error("synchronization persistence failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait SyncService: Send + Sync {
    async fn list_libraries(&self, authenticated_user: &str) -> Result<Vec<LibrarySummary>, SyncError>;
    async fn upsert_library(&self, authenticated_user: &str, request: UpsertLibraryRequest) -> Result<LibrarySummary, SyncError>;
    async fn push(&self, authenticated_user: &str, request: PushChangesRequest) -> Result<PushChangesResponse, SyncError>;
    async fn pull(&self, authenticated_user: &str, query: PullQuery) -> Result<PullChangesResponse, SyncError>;
}

#[async_trait]
pub trait SyncRepository: Send + Sync {
    async fn list_libraries(&self, user_id: &str) -> Result<Vec<LibrarySummary>, SyncError>;
    async fn upsert_library(&self, user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibrarySummary>, SyncError>;
    async fn ensure_library_owner(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<bool, SyncError>;
    async fn store_changes(&self, user_id: &str, request: &PushChangesRequest) -> Result<PushChangesResponse, SyncError>;
    async fn load_changes(&self, query: &PullQuery) -> Result<PullChangesResponse, SyncError>;
}

/// Synchronization use cases and protocol-level authorization. Persistence
/// implementations only provide atomic storage operations through
/// [`SyncRepository`].
pub struct SyncApplication {
    repository: Arc<dyn SyncRepository>,
}

impl SyncApplication {
    pub fn new(repository: Arc<dyn SyncRepository>) -> Self {
        Self { repository }
    }

    async fn require_library_owner(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<(), SyncError> {
        if self.repository.ensure_library_owner(user_id, library_id).await? {
            Ok(())
        } else {
            Err(SyncError::LibraryOwnershipConflict)
        }
    }
}

#[async_trait]
impl SyncService for SyncApplication {
    async fn list_libraries(&self, authenticated_user: &str) -> Result<Vec<LibrarySummary>, SyncError> {
        self.repository.list_libraries(authenticated_user).await
    }

    async fn upsert_library(&self, authenticated_user: &str, request: UpsertLibraryRequest) -> Result<LibrarySummary, SyncError> {
        let name = request.library_name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(SyncError::InvalidLibraryName);
        }
        let request = UpsertLibraryRequest { library_name: name.to_owned(), ..request };
        self.repository.upsert_library(authenticated_user, &request).await?.ok_or(SyncError::LibraryOwnershipConflict)
    }

    async fn push(&self, authenticated_user: &str, request: PushChangesRequest) -> Result<PushChangesResponse, SyncError> {
        if request.user_id != authenticated_user {
            return Err(SyncError::Forbidden);
        }
        if request.changes.is_empty() {
            return Ok(PushChangesResponse { handled_local_seqs: Vec::new(), rejected: Vec::new() });
        }
        self.require_library_owner(authenticated_user, &request.library_id).await?;
        self.repository.store_changes(authenticated_user, &request).await
    }

    async fn pull(&self, authenticated_user: &str, query: PullQuery) -> Result<PullChangesResponse, SyncError> {
        if query.user_id != authenticated_user {
            return Err(SyncError::Forbidden);
        }
        self.require_library_owner(authenticated_user, &query.library_id).await?;
        self.repository.load_changes(&query).await
    }
}

#[async_trait]
pub trait GcLease: Send {
    fn live_content(&self) -> &HashSet<ContentHash>;
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
        let books = self.assets.sweep(AssetKind::Book, lease.live_content(), cutoff).await.map_err(|error| SyncError::Internal(error.to_string()))?;
        let thumbnails = self.assets.sweep(AssetKind::Thumbnail, lease.live_content(), cutoff).await.map_err(|error| SyncError::Internal(error.to_string()))?;
        lease.release().await?;
        Ok(GcReport { blobs_scanned: books.scanned, blobs_deleted: books.deleted, thumbnails_scanned: thumbnails.scanned, thumbnails_deleted: thumbnails.deleted, bytes_reclaimed: books.bytes_reclaimed + thumbnails.bytes_reclaimed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sync_common::{ClientId, LibraryId};

    struct RecordingRepository {
        owns_library: bool,
        ownership_checks: AtomicUsize,
        loads: AtomicUsize,
    }

    struct RecordingAssetRepository {
        owns_library: bool,
        authorized: HashSet<ContentHash>,
    }

    #[async_trait]
    impl AssetRepository for RecordingAssetRepository {
        async fn authorized_content_hashes(&self, _user_id: &str, _library_id: &LibraryId, candidates: &[ContentHash]) -> Result<Option<HashSet<ContentHash>>, AssetError> {
            if !self.owns_library {
                return Ok(None);
            }
            Ok(Some(candidates.iter().filter(|hash| self.authorized.contains(*hash)).cloned().collect()))
        }

        async fn claim_existing_book(&self, _user_id: &str, _library_id: &LibraryId, _hash: &ContentHash, _size_bytes: u64) -> Result<(), AssetError> {
            Ok(())
        }

        async fn reserve_book_upload(&self, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError> {
            Ok(BookUploadAdmission::Reserved(BookUploadReservation {
                id: "reservation".to_string(),
                user_id: user_id.to_string(),
                library_id: library_id.clone(),
                hash: hash.clone(),
                expected_size_bytes: size_bytes,
            }))
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
        async fn exists(&self, _kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.present.contains(hash))
        }
        async fn existing(&self, _kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(hashes.into_iter().filter(|hash| self.present.contains(hash)).collect())
        }
        async fn size(&self, _kind: AssetKind, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.present.contains(hash).then_some(1))
        }
        async fn open_from(&self, _kind: AssetKind, _hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(StoredAsset { reader: Box::new(std::io::Cursor::new(Vec::new())), total_len: offset, offset }))
        }
        async fn put(&self, _kind: AssetKind, _hash: &ContentHash, expected_len: Option<u64>, _reader: AssetReader) -> Result<u64, AssetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(expected_len.unwrap_or(0))
        }
        async fn sweep(&self, _kind: AssetKind, _live: &HashSet<ContentHash>, _cutoff: SystemTime) -> Result<SweepReport, AssetError> {
            Ok(SweepReport::default())
        }
    }

    #[async_trait]
    impl SyncRepository for RecordingRepository {
        async fn list_libraries(&self, _user_id: &str) -> Result<Vec<LibrarySummary>, SyncError> {
            Ok(Vec::new())
        }

        async fn upsert_library(&self, _user_id: &str, request: &UpsertLibraryRequest) -> Result<Option<LibrarySummary>, SyncError> {
            Ok(Some(LibrarySummary { library_id: request.library_id.clone(), library_name: request.library_name.clone() }))
        }

        async fn ensure_library_owner(&self, _user_id: &str, _library_id: &LibraryId) -> Result<bool, SyncError> {
            self.ownership_checks.fetch_add(1, Ordering::SeqCst);
            Ok(self.owns_library)
        }

        async fn store_changes(&self, _user_id: &str, _request: &PushChangesRequest) -> Result<PushChangesResponse, SyncError> {
            unreachable!("an empty push must not reach persistence")
        }

        async fn load_changes(&self, _query: &PullQuery) -> Result<PullChangesResponse, SyncError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            Ok(PullChangesResponse { changes: Vec::new() })
        }
    }

    fn repository(owns_library: bool) -> Arc<RecordingRepository> {
        Arc::new(RecordingRepository { owns_library, ownership_checks: AtomicUsize::new(0), loads: AtomicUsize::new(0) })
    }

    fn pull_query(user_id: &str) -> PullQuery {
        PullQuery { user_id: user_id.to_string(), library_id: LibraryId::parse("library").unwrap(), client_id: ClientId::parse("client").unwrap(), since_server_seq: None }
    }

    #[tokio::test]
    async fn empty_push_is_handled_without_opening_a_repository_transaction() {
        let repository = repository(true);
        let service = SyncApplication::new(repository.clone());
        let response = service.push("user", PushChangesRequest { user_id: "user".to_string(), library_id: LibraryId::parse("library").unwrap(), client_id: ClientId::parse("client").unwrap(), changes: Vec::new() }).await.unwrap();

        assert!(response.handled_local_seqs.is_empty());
        assert_eq!(repository.ownership_checks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn core_rejects_cross_user_pull_before_persistence() {
        let repository = repository(true);
        let service = SyncApplication::new(repository.clone());

        assert!(matches!(service.pull("authenticated", pull_query("claimed")).await, Err(SyncError::Forbidden)));
        assert_eq!(repository.ownership_checks.load(Ordering::SeqCst), 0);
        assert_eq!(repository.loads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn library_catalogue_validates_and_canonicalizes_names() {
        let service = SyncApplication::new(repository(true));
        let library_id = LibraryId::parse("library").unwrap();
        let saved = service.upsert_library("user", UpsertLibraryRequest { library_id: library_id.clone(), library_name: "  History  ".to_string() }).await.unwrap();
        assert_eq!(saved.library_id, library_id);
        assert_eq!(saved.library_name, "History");

        let invalid = service.upsert_library("user", UpsertLibraryRequest { library_id: LibraryId::parse("other").unwrap(), library_name: "\n".to_string() }).await;
        assert!(matches!(invalid, Err(SyncError::InvalidLibraryName)));
    }

    #[tokio::test]
    async fn core_rejects_library_owned_by_another_user() {
        let repository = repository(false);
        let service = SyncApplication::new(repository.clone());

        assert!(matches!(service.pull("user", pull_query("user")).await, Err(SyncError::LibraryOwnershipConflict)));
        assert_eq!(repository.ownership_checks.load(Ordering::SeqCst), 1);
        assert_eq!(repository.loads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn asset_access_requires_both_library_ownership_and_a_live_reference() {
        let allowed = ContentHash::parse(&"a".repeat(64)).unwrap();
        let unreferenced = ContentHash::parse(&"b".repeat(64)).unwrap();
        let library = LibraryId::parse("library").unwrap();

        let store = Arc::new(RecordingAssetStore { calls: AtomicUsize::new(0), present: HashSet::from([allowed.clone(), unreferenced.clone()]) });
        let foreign = AssetApplication::new(Arc::new(RecordingAssetRepository { owns_library: false, authorized: HashSet::from([allowed.clone()]) }), store.clone());
        assert!(matches!(foreign.exists("intruder", &library, AssetKind::Book, &allowed).await, Err(AssetError::Forbidden)));
        assert_eq!(store.calls.load(Ordering::SeqCst), 0, "unauthorized access must stop before storage");

        let owned = AssetApplication::new(Arc::new(RecordingAssetRepository { owns_library: true, authorized: HashSet::from([allowed.clone()]) }), store.clone());
        assert!(matches!(owned.open_from("owner", &library, AssetKind::Book, &unreferenced, 0).await, Err(AssetError::Forbidden)));
        assert!(matches!(owned.put("owner", &library, AssetKind::Book, &unreferenced, Some(0), Box::new(std::io::Cursor::new(Vec::new()))).await, Err(AssetError::Forbidden)));
        assert_eq!(store.calls.load(Ordering::SeqCst), 0, "unreferenced hashes must stop before storage");

        let present = owned.existing("owner", &library, AssetKind::Book, vec![allowed.clone(), unreferenced]).await.unwrap();
        assert_eq!(present, vec![allowed]);
        assert_eq!(store.calls.load(Ordering::SeqCst), 2);
    }
}

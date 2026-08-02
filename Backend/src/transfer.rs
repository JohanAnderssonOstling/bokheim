use crate::path_types::LibraryRoot;
use crate::scanner::{DownloadState, LibraryEvent};
use crate::sync::DbPool;
use blake3::Hasher;
use futures_util::StreamExt;
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::{error::Error, fmt};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration, Instant};
use tokio_util::io::ReaderStream;
use urlencoding::encode;

use crate::db;
use crate::{BlobKind, BookId, BookPlacement, ContentHash, DownloadProgress, RelativeBookPath, ServerUrl, TransferJobKind};
use sync_common::LibraryId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    Queued,
    Running,
    Retrying,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferOrigin {
    #[default]
    Background,
    UserInitiated,
}

impl TransferOrigin {
    pub(crate) const fn storage(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::UserInitiated => "user_initiated",
        }
    }

    pub(crate) fn parse_storage(value: &str) -> Option<Self> {
        match value {
            "background" => Some(Self::Background),
            "user_initiated" => Some(Self::UserInitiated),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTransport {
    Offline,
    Wifi,
    Cellular,
    Ethernet,
    Vpn,
    Other,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct NetworkStatus {
    pub transport: NetworkTransport,
    pub connected: bool,
    pub metered: Option<bool>,
    pub validated: Option<bool>,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self { transport: NetworkTransport::Unknown, connected: true, metered: None, validated: None }
    }
}

/// Process-local transfer conditions shared by every open library. Connectivity
/// is supplied by the platform shell; the durable user preference is loaded by
/// `AppCore` and never enters a library's synchronized state.
#[derive(Clone, Debug, Default)]
pub(crate) struct TransferRuntime {
    state: Arc<RwLock<TransferRuntimeState>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TransferRuntimeState {
    network: NetworkStatus,
    allow_background_on_metered: bool,
}

impl TransferRuntime {
    pub(crate) fn network_status(&self) -> NetworkStatus {
        self.state.read().map(|state| state.network).unwrap_or_default()
    }

    pub(crate) fn set_network_status(&self, status: NetworkStatus) -> bool {
        let was_allowed = self.allows_background_transfers();
        if let Ok(mut state) = self.state.write() {
            state.network = status;
        }
        !was_allowed && self.allows_background_transfers()
    }

    pub(crate) fn set_allow_background_on_metered(&self, allow: bool) -> bool {
        let was_allowed = self.allows_background_transfers();
        if let Ok(mut state) = self.state.write() {
            state.allow_background_on_metered = allow;
        }
        !was_allowed && self.allows_background_transfers()
    }

    pub(crate) fn allows_background_transfers(&self) -> bool {
        self.state.read().map(|state| state.network.connected && (state.allow_background_on_metered || state.network.metered != Some(true))).unwrap_or(true)
    }
}

impl TransferState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Retrying => "Retrying",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct TransferStatus {
    pub id: String,
    pub kind: TransferJobKind,
    pub content_hash: String,
    pub book_id: Option<String>,
    pub path: Option<String>,
    pub state: TransferState,
    pub origin: TransferOrigin,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct LibraryTransfers {
    pub library_id: String,
    pub library_name: String,
    pub transfers: Vec<TransferStatus>,
}

pub(crate) const MAX_BOOK_BYTES: u64 = book_format::MAX_BOOK_BYTES;
const MAX_THUMBNAIL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_THUMBNAIL_DIMENSION: u32 = 4096;
const MAX_THUMBNAIL_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
struct PermanentTransferError(String);

#[derive(Debug)]
struct AuthenticationRequired;

impl fmt::Display for PermanentTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for PermanentTransferError {}

impl fmt::Display for AuthenticationRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("authentication expired; sign in again")
    }
}

impl Error for AuthenticationRequired {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TransferFailure {
    Retryable(String),
    Permanent(String),
}

fn permanent(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(PermanentTransferError(message.into()))
}

fn classify_failure(error: &(dyn Error + 'static)) -> TransferFailure {
    if error.downcast_ref::<AuthenticationRequired>().is_some() {
        return TransferFailure::Retryable(error.to_string());
    }
    if error.downcast_ref::<PermanentTransferError>().is_some() {
        return TransferFailure::Permanent(error.to_string());
    }
    if let Some(error) = error.downcast_ref::<reqwest::Error>() {
        if let Some(status) = error.status() {
            if status.is_client_error() && status != reqwest::StatusCode::REQUEST_TIMEOUT && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
                return TransferFailure::Permanent(error.to_string());
            }
        }
    }
    TransferFailure::Retryable(error.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransferJob {
    UploadBlob { content_hash: ContentHash, local_path: String, placement: Option<BookPlacement>, origin: TransferOrigin },
    UploadThumbnail { content_hash: ContentHash, thumbnail_path: String, book_id: Option<BookId>, origin: TransferOrigin },
    DownloadBlob { content_hash: ContentHash, placement: BookPlacement, origin: TransferOrigin },
    DownloadThumbnail { content_hash: ContentHash, book_id: BookId, origin: TransferOrigin },
}

impl TransferJob {
    fn blob_key(&self) -> crate::BlobKey {
        match self {
            Self::UploadBlob { content_hash, .. } | Self::DownloadBlob { content_hash, .. } => crate::BlobKey::new(BlobKind::Book, content_hash.clone()),
            Self::UploadThumbnail { content_hash, .. } | Self::DownloadThumbnail { content_hash, .. } => crate::BlobKey::new(BlobKind::Thumbnail, content_hash.clone()),
        }
    }

    pub(crate) const fn origin(&self) -> TransferOrigin {
        match self {
            Self::UploadBlob { origin, .. } | Self::UploadThumbnail { origin, .. } | Self::DownloadBlob { origin, .. } | Self::DownloadThumbnail { origin, .. } => *origin,
        }
    }

    pub(crate) fn same_persisted_key(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::UploadBlob { content_hash: left_hash, local_path: left_path, placement: left_placement, .. }, Self::UploadBlob { content_hash: right_hash, local_path: right_path, placement: right_placement, .. }) => {
                left_hash == right_hash && left_path == right_path && left_placement == right_placement
            }
            (Self::UploadThumbnail { content_hash: left_hash, thumbnail_path: left_path, book_id: left_book, .. }, Self::UploadThumbnail { content_hash: right_hash, thumbnail_path: right_path, book_id: right_book, .. }) => {
                left_hash == right_hash && left_path == right_path && left_book == right_book
            }
            (Self::DownloadBlob { content_hash: left_hash, placement: left_placement, .. }, Self::DownloadBlob { content_hash: right_hash, placement: right_placement, .. }) => left_hash == right_hash && left_placement == right_placement,
            (Self::DownloadThumbnail { content_hash: left_hash, book_id: left_book, .. }, Self::DownloadThumbnail { content_hash: right_hash, book_id: right_book, .. }) => left_hash == right_hash && left_book == right_book,
            _ => false,
        }
    }

    fn persist(&self, conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
        match self {
            Self::UploadBlob { content_hash, local_path, placement, origin } => {
                db::upsert_transfer_job(conn, TransferJobKind::UploadBook, content_hash, placement.as_ref().map(|value| value.rel_path.as_str()).unwrap_or(""), local_path, placement.as_ref().map(|value| value.book_id), *origin)
            }
            Self::UploadThumbnail { content_hash, thumbnail_path, book_id, origin } => db::upsert_transfer_job(conn, TransferJobKind::UploadThumbnail, content_hash, "", thumbnail_path, *book_id, *origin),
            Self::DownloadBlob { content_hash, placement, origin } => db::upsert_transfer_job(conn, TransferJobKind::DownloadBook, content_hash, placement.rel_path.as_str(), placement.rel_path.as_str(), Some(placement.book_id), *origin),
            Self::DownloadThumbnail { content_hash, book_id, origin } => db::upsert_transfer_job(conn, TransferJobKind::DownloadThumbnail, content_hash, "", "", Some(*book_id), *origin),
        }
    }

    pub(crate) fn from_storage(kind: TransferJobKind, content_hash: ContentHash, rel_path: &str, local_path: &str, raw_book_id: &str, origin: TransferOrigin) -> Option<Self> {
        let book_id = if raw_book_id.is_empty() { None } else { Some(BookId::parse(raw_book_id).ok()?) };
        match kind {
            TransferJobKind::UploadBook => {
                if local_path.is_empty() {
                    return None;
                }
                let placement = match (book_id, rel_path.is_empty()) {
                    (None, true) => None,
                    (Some(book_id), false) => Some(BookPlacement::new(book_id, RelativeBookPath::parse(rel_path).ok()?)),
                    _ => return None,
                };
                Some(Self::UploadBlob { content_hash, local_path: local_path.to_owned(), placement, origin })
            }
            TransferJobKind::UploadThumbnail if rel_path.is_empty() && !local_path.is_empty() => Some(Self::UploadThumbnail { content_hash, thumbnail_path: local_path.to_owned(), book_id, origin }),
            TransferJobKind::DownloadBook if !local_path.is_empty() => Some(Self::DownloadBlob { content_hash, placement: BookPlacement::new(book_id?, RelativeBookPath::parse(rel_path).ok()?), origin }),
            TransferJobKind::DownloadThumbnail if rel_path.is_empty() && local_path.is_empty() => Some(Self::DownloadThumbnail { content_hash, book_id: book_id?, origin }),
            _ => None,
        }
    }

    fn priority(&self) -> (u8, u8, u64) {
        let origin = if self.origin() == TransferOrigin::UserInitiated { 0 } else { 1 };
        match self {
            Self::DownloadThumbnail { .. } => (origin, 0, 0),
            Self::UploadThumbnail { thumbnail_path, .. } => (origin, 0, file_size_or(thumbnail_path, 0)),
            Self::UploadBlob { local_path, .. } => (origin, 1, file_size_or(local_path, u64::MAX / 2)),
            Self::DownloadBlob { .. } => (origin, 1, u64::MAX / 2),
        }
    }
}

pub(crate) struct TransferWorker {
    http_client: Client,
    server_url: ServerUrl,
    library_id: LibraryId,
    library_root: LibraryRoot,
    db_pool: DbPool,
    event_tx: Option<crossbeam_channel::Sender<LibraryEvent>>,
    budget: Arc<Semaphore>,
    session: Arc<std::sync::RwLock<Option<crate::auth::Session>>>,
    session_path: PathBuf,
    runtime: TransferRuntime,
}

impl TransferWorker {
    pub(crate) fn new(
        http_client: Client, server_url: ServerUrl, library_id: LibraryId, library_root: LibraryRoot, db_pool: DbPool, event_tx: Option<crossbeam_channel::Sender<LibraryEvent>>,
        session: Arc<std::sync::RwLock<Option<crate::auth::Session>>>, session_path: PathBuf, runtime: TransferRuntime,
    ) -> Self {
        let byte_budget_mb = std::env::var("BOKHEIM_TRANSFER_BUDGET_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(64);
        Self { http_client, server_url, library_id, library_root, db_pool, event_tx, budget: Arc::new(Semaphore::new(byte_budget_mb.max(1))), session, session_path, runtime }
    }

    pub(crate) async fn run(self, jobs: Vec<TransferJob>, worker_count: usize) -> Result<(), Box<dyn std::error::Error>> {
        enqueue_jobs(&self.db_pool, jobs)?;

        self.run_pending(worker_count).await
    }

    pub(crate) async fn run_pending(self, worker_count: usize) -> Result<(), Box<dyn std::error::Error>> {
        {
            let conn = self.db_pool.get()?;
            db::reset_stale_claims(&conn, 300)?;
        }

        let worker = Arc::new(self);
        let mut handles = Vec::new();
        for _ in 0..worker_count.max(1) {
            let worker = Arc::clone(&worker);
            handles.push(tokio::spawn(async move { worker.run_queue().await }));
        }

        for handle in handles {
            handle.await?.map_err(|error| format!("transfer queue failed: {error}"))?;
        }
        // Future retries are durable in SQLite. Keep servicing them in the
        // background without making sync_once wait through long backoffs.
        let has_scheduled_retries = {
            let conn = worker.db_pool.get()?;
            db::get_next_transfer_retry_at(&conn)?.is_some()
        };
        if has_scheduled_retries {
            let scheduler = Arc::clone(&worker);
            tokio::spawn(async move {
                if let Err(error) = scheduler.run_retry_scheduler().await {
                    log::warn!("Transfer retry scheduler stopped: {error}");
                }
            });
        }
        Ok(())
    }

    async fn run_retry_scheduler(&self) -> Result<(), String> {
        loop {
            let next_retry = {
                let conn = self.db_pool.get().map_err(|error| error.to_string())?;
                db::get_next_transfer_retry_at(&conn).map_err(|error| error.to_string())?
            };
            let Some(next_retry) = next_retry else { return Ok(()) };
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error| error.to_string())?.as_secs() as i64;
            if next_retry > now {
                sleep(Duration::from_secs((next_retry - now) as u64)).await;
            }
            self.run_queue().await?;
        }
    }

    async fn run_queue(&self) -> Result<(), String> {
        loop {
            let claimed = {
                let conn = self.db_pool.get().map_err(|error| error.to_string())?;
                db::claim_due_transfer_job(&conn, self.runtime.allows_background_transfers()).map_err(|error| error.to_string())?
            };
            let Some(claimed) = claimed else {
                break;
            };
            let job = claimed.job;
            let blob_key = job.blob_key();
            let result = {
                let future = self.handle_job(job);
                tokio::pin!(future);
                let mut heartbeat = tokio::time::interval(Duration::from_secs(60));
                heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                heartbeat.tick().await;
                loop {
                    tokio::select! {
                        result = &mut future => break result,
                        _ = heartbeat.tick() => {
                            let conn = self.db_pool.get().map_err(|error| error.to_string())?;
                            db::heartbeat_transfer_job(&conn, claimed.id, &claimed.claim_token).map_err(|error| error.to_string())?;
                        }
                    }
                }
            };
            match result {
                Ok(()) => {
                    let conn = self.db_pool.get().map_err(|error| error.to_string())?;
                    db::complete_transfer_job(&conn, claimed.id, &claimed.claim_token).map_err(|error| error.to_string())?;
                }
                Err(err) => {
                    let conn = self.db_pool.get().map_err(|error| error.to_string())?;
                    db::set_asset_remote_error(&conn, blob_key.kind(), blob_key.content_hash(), &err.to_string()).map_err(|error| error.to_string())?;
                    match classify_failure(err.as_ref()) {
                        TransferFailure::Retryable(message) => {
                            db::fail_transfer_job(&conn, claimed.id, &claimed.claim_token, claimed.attempts, &message).map_err(|error| error.to_string())?;
                            log::warn!("Retryable transfer error: {message}");
                        }
                        TransferFailure::Permanent(message) => {
                            db::complete_transfer_job(&conn, claimed.id, &claimed.claim_token).map_err(|error| error.to_string())?;
                            log::warn!("Permanent transfer error: {message}");
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn enqueue_jobs(db_pool: &DbPool, jobs: Vec<TransferJob>) -> Result<(), Box<dyn std::error::Error>> {
    if jobs.is_empty() {
        return Ok(());
    }
    let mut jobs = jobs;
    jobs.sort_by_key(TransferJob::priority);
    let conn = db_pool.get()?;
    db::prune_unclaimed_transfer_jobs(&conn, &jobs)?;

    for job in &jobs {
        job.persist(&conn)?;
    }
    Ok(())
}

fn file_size_or(path: &str, default: u64) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(default)
}

impl TransferWorker {
    fn asset_endpoint(&self, namespace: &str, content_hash: &ContentHash) -> reqwest::Url {
        self.server_url.endpoint(&format!("api/libraries/{}/{namespace}/{}", encode(self.library_id.as_str()), encode(content_hash.as_str())))
    }

    async fn handle_job(&self, job: TransferJob) -> Result<(), Box<dyn std::error::Error>> {
        match job {
            TransferJob::UploadBlob { content_hash, local_path, placement, origin: _ } => {
                let permits = mb_permits_for_path(&local_path, self.budget.available_permits()).await;
                let _permit = self.budget.clone().acquire_many_owned(permits).await?;
                self.upload_blob(&content_hash, &local_path, placement.as_ref()).await?;
            }
            TransferJob::UploadThumbnail { content_hash, thumbnail_path, book_id: _, origin: _ } => {
                let permits = mb_permits_for_path(&thumbnail_path, self.budget.available_permits()).await;
                let _permit = self.budget.clone().acquire_many_owned(permits).await?;
                self.upload_thumbnail(&content_hash, &thumbnail_path).await?;
            }
            TransferJob::DownloadBlob { content_hash, placement, origin: _ } => {
                let _permit = self.budget.clone().acquire_owned().await?;
                self.download_blob(&content_hash, &placement).await?;
            }
            TransferJob::DownloadThumbnail { content_hash, book_id, origin: _ } => {
                let _permit = self.budget.clone().acquire_owned().await?;
                self.download_thumbnail(&content_hash, book_id).await?;
            }
        }
        Ok(())
    }

    fn bearer_token(&self) -> Result<String, Box<dyn std::error::Error>> {
        self.session.read().ok().and_then(|session| session.as_ref().map(|session| session.token().to_owned())).ok_or_else(|| Box::new(AuthenticationRequired) as Box<dyn std::error::Error>)
    }

    fn authenticated_response(&self, response: reqwest::Response) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Ok(mut session) = self.session.write() {
                *session = None;
            }
            if let Err(error) = crate::auth::Session::clear_from(&self.session_path) {
                log::warn!("Failed to remove rejected authentication session: {error}");
            }
            return Err(Box::new(AuthenticationRequired));
        }
        if response.status() == reqwest::StatusCode::INSUFFICIENT_STORAGE {
            return Err(permanent("book storage quota exceeded"));
        }
        Ok(response.error_for_status()?)
    }
}

async fn mb_permits_for_path(path: &str, max_permits: usize) -> u32 {
    let cap = max_permits.max(1);
    let size = tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(1024 * 1024);
    let mb = size.div_ceil(1024 * 1024) as usize;
    mb.clamp(1, cap) as u32
}

impl TransferWorker {
    async fn upload_blob(&self, content_hash: &ContentHash, local_path: &str, placement: Option<&BookPlacement>) -> Result<(), Box<dyn std::error::Error>> {
        let local_path = self.library_root.existing_file(Path::new(local_path))?;
        let validation_path = local_path.clone();
        tokio::task::spawn_blocking(move || book_format::validate_path(validation_path))
            .await
            .map_err(|error| permanent(format!("book validation task failed: {error}")))?
            .map_err(|error| permanent(format!("book upload rejected: {error}")))?;
        let token = self.bearer_token()?;
        let local_path_text = local_path.to_str().ok_or("book path is not valid UTF-8")?;
        let (file, len, actual_hash) = open_hashed_file(&local_path, MAX_BOOK_BYTES).await?;
        if actual_hash.as_str() != content_hash.as_str() {
            let conn = self.db_pool.get()?;
            db::upsert_blob_local_ref(&conn, BlobKind::Book, content_hash, local_path_text, false)?;
            if let Some(placement) = placement {
                db::set_book_downloaded(&conn, &placement.rel_path, placement.book_id, false)?;
            }
            return Err(permanent(format!("upload hash mismatch expected={content_hash} got={actual_hash}")));
        }
        let response = self.http_client.put(self.asset_endpoint("blobs", content_hash)).bearer_auth(&token).header(reqwest::header::CONTENT_LENGTH, len).body(reqwest::Body::wrap_stream(ReaderStream::new(file))).send().await?;
        self.authenticated_response(response)?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Book, content_hash, local_path_text, true)?;
        db::mark_asset_remote_present(&conn, BlobKind::Book, content_hash)?;
        if let Some(placement) = placement {
            db::set_book_downloaded(&conn, &placement.rel_path, placement.book_id, true)?;
        }
        Ok(())
    }

    async fn upload_thumbnail(&self, content_hash: &ContentHash, thumbnail_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        if !Path::new(thumbnail_path).exists() {
            return Ok(());
        }
        let token = self.bearer_token()?;
        let thumbnail_path = self.library_root.existing_file(Path::new(thumbnail_path))?;
        let thumbnail_path_text = thumbnail_path.to_str().ok_or("thumbnail path is not valid UTF-8")?;
        let (mut file, len) = open_limited_file(&thumbnail_path, MAX_THUMBNAIL_BYTES).await?;
        let mut signature = [0_u8; 8];
        file.read_exact(&mut signature).await?;
        if signature != [137, 80, 78, 71, 13, 10, 26, 10] {
            return Err(permanent("thumbnail must be a PNG"));
        }
        file.seek(SeekFrom::Start(0)).await?;
        let response = self.http_client.put(self.asset_endpoint("thumbnails", content_hash)).bearer_auth(&token).header(reqwest::header::CONTENT_LENGTH, len).body(reqwest::Body::wrap_stream(ReaderStream::new(file))).send().await?;
        self.authenticated_response(response)?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, content_hash, thumbnail_path_text, true)?;
        db::mark_asset_remote_present(&conn, BlobKind::Thumbnail, content_hash)?;
        Ok(())
    }

    async fn download_blob(&self, content_hash: &ContentHash, placement: &BookPlacement) -> Result<(), Box<dyn std::error::Error>> {
        let token = self.bearer_token()?;
        let book_id = placement.book_id;
        let rel_path = &placement.rel_path;
        let local_path = self.library_root.resolve_book(rel_path);
        let local_path_text = local_path.to_str().ok_or("book path is not valid UTF-8")?;
        self.send_download_status(book_id, DownloadState::Downloading(DownloadProgress::ZERO));

        let partial_path = partial_book_path(&self.library_root, content_hash);
        if let Some(parent) = partial_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&partial_path).await?;
        let mut downloaded = file.metadata().await?.len();
        if downloaded > MAX_BOOK_BYTES {
            file.set_len(0).await?;
            downloaded = 0;
        }
        let mut hasher = Hasher::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        file.seek(SeekFrom::Start(0)).await?;
        loop {
            let count = file.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let partial_is_complete = downloaded > 0 && hasher.clone().finalize().to_hex().as_str() == content_hash.as_str();

        if !partial_is_complete {
            let total_size;
            let mut request = self.http_client.get(self.asset_endpoint("blobs", content_hash)).bearer_auth(&token);
            if downloaded > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={downloaded}-")).header(reqwest::header::IF_RANGE, format!("\"{}\"", content_hash.as_str()));
            }
            let response = request.send().await?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                let conn = self.db_pool.get()?;
                db::mark_asset_remote_missing(&conn, BlobKind::Book, content_hash)?;
                self.send_download_status(book_id, DownloadState::NotDownloaded);
                return Ok(());
            }
            if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && downloaded > 0 {
                file.set_len(0).await?;
                file.sync_all().await?;
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "server rejected saved download range; retrying from byte zero").into());
            }
            let response = self.authenticated_response(response)?;
            let expected_etag = format!("\"{}\"", content_hash.as_str());
            if response.headers().get(reqwest::header::ETAG).and_then(|value| value.to_str().ok()) != Some(expected_etag.as_str()) {
                return Err(permanent("asset response has a missing or mismatched ETag"));
            }
            if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                if downloaded == 0 {
                    return Err(permanent("server returned a partial response without a range request"));
                }
                total_size = parse_content_range_total(response.headers(), downloaded)?;
            } else {
                if downloaded > 0 {
                    file.set_len(0).await?;
                    downloaded = 0;
                    hasher = Hasher::new();
                }
                total_size = response.content_length().unwrap_or(0);
            }
            if total_size > MAX_BOOK_BYTES {
                return Err(permanent(format!("book download exceeds {MAX_BOOK_BYTES} bytes")));
            }
            file.seek(SeekFrom::Start(downloaded)).await?;
            let mut stream = response.bytes_stream();
            let mut last_progress = DownloadProgress::from_ratio(downloaded, total_size).value();
            let bandwidth_limit = std::env::var("BOKHEIM_DOWNLOAD_BANDWIDTH").ok().and_then(|value| value.parse::<f64>().ok()).filter(|&limit| limit > 0.0);
            let mut next_allowed = Instant::now();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                if downloaded.saturating_add(chunk.len() as u64) > MAX_BOOK_BYTES {
                    return Err(permanent(format!("book download exceeds {MAX_BOOK_BYTES} bytes")));
                }
                if let Some(limit) = bandwidth_limit {
                    let now = Instant::now();
                    if now < next_allowed {
                        sleep(next_allowed - now).await;
                    }
                    next_allowed = Instant::now() + Duration::from_secs_f64(chunk.len() as f64 / limit);
                }
                hasher.update(&chunk);
                file.write_all(&chunk).await?;
                downloaded += chunk.len() as u64;
                let progress = DownloadProgress::from_ratio(downloaded, total_size);
                if (progress.value() - last_progress).abs() >= 0.02 || progress.value() >= 0.99 {
                    self.send_download_status(book_id, DownloadState::Downloading(progress));
                    last_progress = progress.value();
                }
            }
            file.flush().await?;
            file.sync_all().await?;
        }
        drop(file);

        let got_hash = hasher.finalize().to_hex().to_string();
        if got_hash != content_hash.as_str() {
            let _ = tokio::fs::remove_file(&partial_path).await;
            self.send_download_status(book_id, DownloadState::NotDownloaded);
            return Err(permanent(format!("download hash mismatch expected={} got={}", content_hash, got_hash)));
        }

        {
            let conn = self.db_pool.get()?;
            if !db::book_placement_expects_hash(&conn, placement, content_hash)? {
                self.send_download_status(book_id, DownloadState::NotDownloaded);
                return Ok(());
            }
            if let Some((_, dir_id, _)) = db::get_book_dir_entry_by_path(&conn, rel_path)? {
                db::materialize_dir_with_markers(&conn, self.library_root.as_path(), dir_id.as_str())?;
            }
        }
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        match tokio::fs::hard_link(&partial_path, &local_path).await {
            Ok(()) => {
                tokio::fs::remove_file(&partial_path).await?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let (_, _, existing_hash) = open_hashed_file(&local_path, MAX_BOOK_BYTES).await?;
                if existing_hash != content_hash.as_str() {
                    return Err(permanent(format!("refusing to overwrite existing file at {}", local_path.display())));
                }
                tokio::fs::remove_file(&partial_path).await?;
            }
            Err(error) => return Err(error.into()),
        }
        sync_parent_directory(&local_path)?;

        let conn = self.db_pool.get()?;
        if let Err(err) = db::stamp_book_dir_local_hash_by_rel_path(&conn, rel_path, book_id, &local_path) {
            log::warn!("Failed to stamp local fingerprint for downloaded {}: {}", local_path.display(), err);
        }
        db::upsert_blob_local_ref(&conn, BlobKind::Book, content_hash, local_path_text, true)?;
        db::mark_asset_remote_present(&conn, BlobKind::Book, content_hash)?;
        db::complete_asset_request(&conn, BlobKind::Book, content_hash)?;
        db::set_book_downloaded(&conn, rel_path, book_id, true)?;
        self.send_download_status(book_id, DownloadState::Downloaded);
        Ok(())
    }

    fn send_download_status(&self, book_id: BookId, state: DownloadState) {
        if let Some(tx) = &self.event_tx {
            let refresh_folders = matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded);
            let _ = tx.send(LibraryEvent::DownloadStatusChanged { book_id: book_id.clone(), state });
            if refresh_folders {
                let folder_statuses = (|| -> Result<Vec<_>, Box<dyn std::error::Error>> {
                    let conn = self.db_pool.get()?;
                    let dir_ids = db::get_book_ancestor_dir_ids(&conn, book_id)?;
                    dir_ids.into_iter().map(|dir_id| db::get_folder_download_counts(&conn, dir_id.as_str()).map(|(downloaded_books, total_books)| (dir_id, downloaded_books, total_books)).map_err(Into::into)).collect()
                })();
                match folder_statuses {
                    Ok(statuses) => {
                        for (dir_id, downloaded_books, total_books) in statuses {
                            let _ = tx.send(LibraryEvent::FolderDownloadStatusChanged { dir_id, downloaded_books, total_books });
                        }
                    }
                    Err(error) => log::warn!("Failed to publish folder download status: {error}"),
                }
            }
        }
    }

    async fn download_thumbnail(&self, content_hash: &ContentHash, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
        let thumb_path = thumbnail_path_from_content_hash(&self.library_root, content_hash);
        if thumb_path.exists() {
            let valid_existing = read_limited_file(&thumb_path, MAX_THUMBNAIL_BYTES).await.ok().is_some_and(|bytes| validate_thumbnail(&bytes).is_ok());
            if valid_existing {
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(LibraryEvent::ThumbnailGenerated { book_id });
                }
                return Ok(());
            }
            match tokio::fs::remove_file(&thumb_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        let token = self.bearer_token()?;
        let response = self.http_client.get(self.asset_endpoint("thumbnails", content_hash)).bearer_auth(&token).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            let conn = self.db_pool.get()?;
            db::mark_asset_remote_missing(&conn, BlobKind::Thumbnail, content_hash)?;
            return Ok(());
        }
        let response = self.authenticated_response(response)?;
        if response.content_length().is_some_and(|len| len > MAX_THUMBNAIL_BYTES) {
            return Err(permanent(format!("thumbnail download exceeds {MAX_THUMBNAIL_BYTES} bytes")));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > MAX_THUMBNAIL_BYTES {
                return Err(permanent(format!("thumbnail download exceeds {MAX_THUMBNAIL_BYTES} bytes")));
            }
            bytes.extend_from_slice(&chunk);
        }
        validate_thumbnail(&bytes)?;
        if let Some(parent) = thumb_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp_path = temporary_sibling_path(&thumb_path);
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        match tokio::fs::hard_link(&tmp_path, &thumb_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let validation = read_limited_file(&thumb_path, MAX_THUMBNAIL_BYTES).await.and_then(|existing| validate_thumbnail(&existing));
                if let Err(error) = validation {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(error);
                }
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(error.into());
            }
        }
        tokio::fs::remove_file(&tmp_path).await?;
        sync_parent_directory(&thumb_path)?;
        let thumb_path_text = thumb_path.to_str().ok_or("thumbnail path is not valid UTF-8")?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, content_hash, thumb_path_text, true)?;
        db::mark_asset_remote_present(&conn, BlobKind::Thumbnail, content_hash)?;
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(LibraryEvent::ThumbnailGenerated { book_id });
        }
        Ok(())
    }
}

pub(crate) fn thumbnail_path_from_content_hash(library_root: &LibraryRoot, content_hash: &ContentHash) -> PathBuf {
    library_root.as_path().join(crate::APP_HIDDEN_DIR).join("thumbnails").join(format!("{content_hash}.png"))
}

fn partial_book_path(library_root: &LibraryRoot, content_hash: &ContentHash) -> PathBuf {
    library_root.as_path().join(crate::APP_HIDDEN_DIR).join("partials").join(format!("{content_hash}.part"))
}

fn parse_content_range_total(headers: &reqwest::header::HeaderMap, expected_start: u64) -> Result<u64, Box<dyn Error>> {
    let raw = headers.get(reqwest::header::CONTENT_RANGE).and_then(|value| value.to_str().ok()).ok_or_else(|| permanent("partial response is missing Content-Range"))?;
    let raw = raw.strip_prefix("bytes ").ok_or_else(|| permanent("invalid Content-Range unit"))?;
    let (range, total) = raw.split_once('/').ok_or_else(|| permanent("invalid Content-Range syntax"))?;
    let (start, end) = range.split_once('-').ok_or_else(|| permanent("invalid Content-Range bounds"))?;
    let start = start.parse::<u64>().map_err(|_| permanent("invalid Content-Range start"))?;
    let end = end.parse::<u64>().map_err(|_| permanent("invalid Content-Range end"))?;
    let total = total.parse::<u64>().map_err(|_| permanent("invalid Content-Range total"))?;
    let expected_len = end.checked_sub(start).and_then(|value| value.checked_add(1)).ok_or_else(|| permanent("invalid Content-Range interval"))?;
    if start != expected_start || end >= total || headers.get(reqwest::header::CONTENT_LENGTH).and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<u64>().ok()) != Some(expected_len) {
        return Err(permanent("partial response range does not match the requested offset"));
    }
    Ok(total)
}

fn temporary_sibling_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(format!(".tmp-{}", uuid::Uuid::new_v4()));
    PathBuf::from(temporary)
}

async fn open_limited_file(path: &Path, limit: u64) -> Result<(tokio::fs::File, u64), Box<dyn std::error::Error>> {
    let file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    if len > limit {
        return Err(permanent(format!("asset is {len} bytes, exceeding the {limit}-byte limit")));
    }
    Ok((file, len))
}

async fn read_limited_file(path: &Path, limit: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    let (file, _) = open_limited_file(path, limit).await?;
    let mut bytes = Vec::new();
    let count = file.take(limit + 1).read_to_end(&mut bytes).await?;
    if count as u64 > limit {
        return Err(permanent(format!("asset exceeds the {limit}-byte limit while reading")));
    }
    Ok(bytes)
}

async fn open_hashed_file(path: &Path, limit: u64) -> Result<(tokio::fs::File, u64, String), Box<dyn std::error::Error>> {
    let (mut file, len) = open_limited_file(path, limit).await?;
    let mut hasher = Hasher::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > limit {
            return Err(permanent(format!("asset exceeds the {limit}-byte limit while hashing")));
        }
        hasher.update(&buffer[..count]);
    }
    if total != len {
        return Err(permanent("asset changed size while it was being prepared for upload"));
    }
    file.seek(SeekFrom::Start(0)).await?;
    Ok((file, len, hasher.finalize().to_hex().to_string()))
}

fn validate_thumbnail(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || width > MAX_THUMBNAIL_DIMENSION || height > MAX_THUMBNAIL_DIMENSION || u64::from(width) * u64::from(height) > MAX_THUMBNAIL_PIXELS {
        return Err(permanent(format!("thumbnail dimensions {width}x{height} exceed limits")));
    }
    Ok(())
}

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "published asset has no parent"))?;
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderMap, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::atomic::{AtomicBool, Ordering};

    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_session(server_url: &ServerUrl) -> Arc<std::sync::RwLock<Option<crate::auth::Session>>> {
        Arc::new(std::sync::RwLock::new(Some(crate::auth::Session::try_new(server_url.clone(), "test-user".to_string(), "test-reader".to_string(), TEST_TOKEN.to_string()).unwrap())))
    }

    #[test]
    fn thumbnail_paths_are_derived_from_the_validated_library_root() {
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let content_hash = ContentHash::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(
            thumbnail_path_from_content_hash(&library_root, &content_hash),
            root.path().canonicalize().unwrap().join(format!("{}/thumbnails/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png", crate::APP_HIDDEN_DIR))
        );
    }

    #[tokio::test]
    async fn upload_rejects_non_book_bytes_before_network_io() {
        let root = tempfile::tempdir().unwrap();
        let book_path = root.path().join("book.epub");
        std::fs::write(&book_path, b"not an EPUB or PDF").unwrap();
        let manager = SqliteConnectionManager::file(root.path().join("library.db"));
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        db::init_db(&pool.get().unwrap()).unwrap();
        let server_url = ServerUrl::parse("http://127.0.0.1:9").unwrap();
        let worker = TransferWorker::new(
            Client::new(),
            server_url.clone(),
            LibraryId::parse("library").unwrap(),
            LibraryRoot::open(root.path()).unwrap(),
            pool.clone(),
            None,
            test_session(&server_url),
            root.path().join(crate::APP_HIDDEN_SESSION_PATH),
            TransferRuntime::default(),
        );
        let expected = ContentHash::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();

        let error = worker.upload_blob(&expected, book_path.to_str().unwrap(), None).await.unwrap_err();

        assert!(error.to_string().contains("book upload rejected"));
        let local_ref_count: i64 = pool.get().unwrap().query_row("SELECT COUNT(*) FROM blob_local_ref WHERE kind='book' AND hash=?1", [expected.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(local_ref_count, 0);
    }

    fn worker_with_placement(root: &Path, server_url: ServerUrl, content_hash: &ContentHash) -> (TransferWorker, DbPool) {
        let manager = SqliteConnectionManager::file(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH));
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        let conn = pool.get().unwrap();
        db::init_db(&conn).unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Shelf')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at) VALUES ('7', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('d1', '7', 'book.epub', '', ?1, 0)", [content_hash.as_str()]).unwrap();
        drop(conn);
        let worker = TransferWorker::new(
            Client::new(),
            server_url.clone(),
            LibraryId::parse("library").unwrap(),
            LibraryRoot::open(root).unwrap(),
            pool.clone(),
            None,
            test_session(&server_url),
            root.join(crate::APP_HIDDEN_SESSION_PATH),
            TransferRuntime::default(),
        );
        (worker, pool)
    }

    #[tokio::test]
    async fn interrupted_download_resumes_from_saved_prefix_and_publishes_verified_bytes() {
        let bytes = b"a complete epub payload used to prove ranged resume".to_vec();
        let content_hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        let prefix_len = 17_usize;
        let saw_range = Arc::new(AtomicBool::new(false));
        let route_bytes = Arc::new(bytes.clone());
        let route_hash = content_hash.clone();
        let route_saw_range = Arc::clone(&saw_range);
        let app = Router::new().route(
            "/api/libraries/library/blobs/:hash",
            get(move |headers: HeaderMap| {
                let bytes = Arc::clone(&route_bytes);
                let hash = route_hash.clone();
                let saw_range = Arc::clone(&route_saw_range);
                async move {
                    let expected_range = format!("bytes={prefix_len}-");
                    let expected_if_range = format!("\"{}\"", hash.as_str());
                    assert_eq!(headers.get("range").and_then(|value| value.to_str().ok()), Some(expected_range.as_str()));
                    assert_eq!(headers.get("if-range").and_then(|value| value.to_str().ok()), Some(expected_if_range.as_str()));
                    saw_range.store(true, Ordering::SeqCst);
                    let suffix = bytes[prefix_len..].to_vec();
                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("etag", format!("\"{}\"", hash.as_str()))
                        .header("content-range", format!("bytes {prefix_len}-{}/{}", bytes.len() - 1, bytes.len()))
                        .header("content-length", suffix.len())
                        .body(Body::from(suffix))
                        .unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(format!("{}/partials", crate::APP_HIDDEN_DIR))).unwrap();
        std::fs::write(partial_book_path(&LibraryRoot::open(root.path()).unwrap(), &content_hash), &bytes[..prefix_len]).unwrap();
        let (worker, _) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);
        let placement = BookPlacement::new(BookId::new(7), RelativeBookPath::parse("/Shelf/book.epub").unwrap());

        worker.download_blob(&content_hash, &placement).await.unwrap();

        assert!(saw_range.load(Ordering::SeqCst));
        assert_eq!(std::fs::read(root.path().join("Shelf/book.epub")).unwrap(), bytes);
        assert!(!partial_book_path(&LibraryRoot::open(root.path()).unwrap(), &content_hash).exists());
        server.abort();
    }

    #[tokio::test]
    async fn unauthorized_transfer_keeps_the_job_retryable_and_clears_authentication() {
        let app = Router::new().route("/api/libraries/library/thumbnails/:hash", get(|| async { StatusCode::UNAUTHORIZED }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let content_hash = ContentHash::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let (worker, _) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);

        let error = worker.download_thumbnail(&content_hash, BookId::new(7)).await.unwrap_err();

        assert!(matches!(classify_failure(error.as_ref()), TransferFailure::Retryable(_)));
        assert!(worker.session.read().unwrap().is_none());
        server.abort();
    }

    #[tokio::test]
    async fn completed_download_is_not_published_after_placement_becomes_stale() {
        let bytes = b"verified but stale bytes".to_vec();
        let content_hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(format!("{}/partials", crate::APP_HIDDEN_DIR))).unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let partial = partial_book_path(&library_root, &content_hash);
        std::fs::write(&partial, &bytes).unwrap();
        let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse("http://127.0.0.1:9").unwrap(), &content_hash);
        pool.get().unwrap().execute("UPDATE book_dir SET deleted_at=1", []).unwrap();
        let placement = BookPlacement::new(BookId::new(7), RelativeBookPath::parse("/Shelf/book.epub").unwrap());

        worker.download_blob(&content_hash, &placement).await.unwrap();

        assert!(!root.path().join("Shelf/book.epub").exists());
        assert!(partial.exists(), "verified content remains reusable for a later live placement");
    }

    #[tokio::test]
    async fn sparse_oversize_upload_is_rejected_before_reading_or_network_io() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("oversize.epub");
        std::fs::File::create(&path).unwrap().set_len(MAX_BOOK_BYTES + 1).unwrap();
        let error = open_hashed_file(&path, MAX_BOOK_BYTES).await.unwrap_err();
        assert!(matches!(classify_failure(error.as_ref()), TransferFailure::Permanent(_)));
    }

    #[test]
    fn response_range_and_failure_classification_are_closed_and_explicit() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::CONTENT_RANGE, reqwest::header::HeaderValue::from_static("bytes 4-9/10"));
        headers.insert(reqwest::header::CONTENT_LENGTH, reqwest::header::HeaderValue::from_static("6"));
        assert_eq!(parse_content_range_total(&headers, 4).unwrap(), 10);
        assert!(parse_content_range_total(&headers, 3).is_err());
        assert!(matches!(classify_failure(permanent("bad input").as_ref()), TransferFailure::Permanent(_)));
        let io_error = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "offline");
        assert!(matches!(classify_failure(&io_error), TransferFailure::Retryable(_)));
    }

    #[test]
    fn runtime_policy_only_gates_background_transfer_claims() {
        let runtime = TransferRuntime::default();
        assert!(runtime.allows_background_transfers());

        let became_allowed = runtime.set_network_status(NetworkStatus { transport: NetworkTransport::Cellular, connected: true, metered: Some(true), validated: Some(true) });
        assert!(!became_allowed);
        assert!(!runtime.allows_background_transfers());

        assert!(runtime.set_allow_background_on_metered(true));
        assert!(runtime.allows_background_transfers());

        assert!(!runtime.set_network_status(NetworkStatus { transport: NetworkTransport::Offline, connected: false, metered: Some(false), validated: Some(false) }));
        assert!(!runtime.allows_background_transfers());
    }
}

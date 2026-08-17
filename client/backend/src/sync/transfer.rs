use crate::platform::scanner::{DownloadState, LibraryEvent};
use crate::sync::DbPool;
use blake3::Hasher;
use futures_util::StreamExt;
use reqwest::Client;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::{error::Error, fmt};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration, Instant};
use tokio_util::io::ReaderStream;
use urlencoding::encode;

use crate::library::database as db;
use crate::{BlobKind, BookPlacement, ContentHash, DownloadProgress, ServerUrl, TransferJobKind};
use sync_common::api::assets::{ThumbnailBatchDownloadRequest, ThumbnailBatchDownloadResponse, ThumbnailBatchEntry, ThumbnailBatchUploadRequest, MAX_THUMBNAIL_BATCH_ENTRIES};
use sync_common::LibraryId;

pub use app_contract::transfer::{LibraryTransfers, NetworkStatus, NetworkTransport, TransferOrigin, TransferState, TransferStatus};

/// Process-local transfer conditions shared by every open library. Connectivity
/// is supplied by the platform shell; the durable user preference is loaded by
/// `AppBackend` and never enters a library's synchronized state.
#[derive(Clone, Debug, Default)]
pub(crate) struct TransferRuntime {
    state: Arc<RwLock<TransferRuntimeState>>,
}

#[derive(Clone, Debug, Default)]
struct TransferRuntimeState {
    network: NetworkStatus,
    allow_background_on_metered: bool,
    auto_download_limit_mb: Option<u32>,
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

    pub(crate) fn set_auto_download_limit_mb(&self, limit_mb: Option<u32>) -> bool {
        let previous = self.auto_download_limit_mb();
        if let Ok(mut state) = self.state.write() {
            state.auto_download_limit_mb = limit_mb;
        }
        previous != limit_mb
    }

    pub(crate) fn auto_download_limit_bytes(&self) -> Option<u64> {
        self.state.read().ok()?.auto_download_limit_mb.map(|value| u64::from(value) * 1024 * 1024)
    }

    fn auto_download_limit_mb(&self) -> Option<u32> {
        self.state.read().ok().and_then(|state| state.auto_download_limit_mb)
    }

    pub(crate) fn allows_background_transfers(&self) -> bool {
        self.state.read().map(|state| state.network.connected && (state.allow_background_on_metered || state.network.metered != Some(true))).unwrap_or(true)
    }
}

const MAX_THUMBNAIL_BYTES: u64 = sync_common::MAX_THUMBNAIL_BYTES;

#[derive(Debug)]
struct PermanentTransferError(String);

#[derive(Debug)]
pub(crate) struct AuthenticationRequired;

const AUTHENTICATION_REQUIRED_MESSAGE: &str = "authentication access token was rejected";

impl fmt::Display for PermanentTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for PermanentTransferError {}

impl fmt::Display for AuthenticationRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(AUTHENTICATION_REQUIRED_MESSAGE)
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
    UploadThumbnail { thumbnail_hash: ContentHash, thumbnail_path: String, book_content_hash: Option<ContentHash>, origin: TransferOrigin },
    DownloadBlob { content_hash: ContentHash, placement: BookPlacement, origin: TransferOrigin },
    DownloadThumbnail { thumbnail_hash: ContentHash, book_content_hash: ContentHash, origin: TransferOrigin },
}

impl TransferJob {
    fn batch_kind(&self) -> Option<TransferJobKind> {
        match self {
            Self::UploadThumbnail { .. } => Some(TransferJobKind::UploadThumbnail),
            Self::DownloadThumbnail { .. } => Some(TransferJobKind::DownloadThumbnail),
            _ => None,
        }
    }
    pub(crate) const fn origin(&self) -> TransferOrigin {
        match self {
            Self::UploadBlob { origin, .. } | Self::UploadThumbnail { origin, .. } | Self::DownloadBlob { origin, .. } | Self::DownloadThumbnail { origin, .. } => *origin,
        }
    }

    pub(crate) fn same_key(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::UploadBlob { content_hash: left_hash, local_path: left_path, placement: left_placement, .. }, Self::UploadBlob { content_hash: right_hash, local_path: right_path, placement: right_placement, .. }) => {
                left_hash == right_hash && left_path == right_path && left_placement == right_placement
            }
            (
                Self::UploadThumbnail { thumbnail_hash: left_hash, thumbnail_path: left_path, book_content_hash: left_book, .. },
                Self::UploadThumbnail { thumbnail_hash: right_hash, thumbnail_path: right_path, book_content_hash: right_book, .. },
            ) => left_hash == right_hash && left_path == right_path && left_book == right_book,
            (Self::DownloadBlob { content_hash: left_hash, placement: left_placement, .. }, Self::DownloadBlob { content_hash: right_hash, placement: right_placement, .. }) => left_hash == right_hash && left_placement == right_placement,
            (Self::DownloadThumbnail { thumbnail_hash: left_hash, book_content_hash: left_book, .. }, Self::DownloadThumbnail { thumbnail_hash: right_hash, book_content_hash: right_book, .. }) => {
                left_hash == right_hash && left_book == right_book
            }
            _ => false,
        }
    }

    fn with_origin(&self, origin: TransferOrigin) -> Self {
        match self {
            Self::UploadBlob { content_hash, local_path, placement, .. } => Self::UploadBlob { content_hash: content_hash.clone(), local_path: local_path.clone(), placement: placement.clone(), origin },
            Self::UploadThumbnail { thumbnail_hash, thumbnail_path, book_content_hash, .. } => {
                Self::UploadThumbnail { thumbnail_hash: thumbnail_hash.clone(), thumbnail_path: thumbnail_path.clone(), book_content_hash: book_content_hash.clone(), origin }
            }
            Self::DownloadBlob { content_hash, placement, .. } => Self::DownloadBlob { content_hash: content_hash.clone(), placement: placement.clone(), origin },
            Self::DownloadThumbnail { thumbnail_hash, book_content_hash, .. } => Self::DownloadThumbnail { thumbnail_hash: thumbnail_hash.clone(), book_content_hash: book_content_hash.clone(), origin },
        }
    }

    fn status_fields(&self) -> (TransferJobKind, &ContentHash, Option<ContentHash>, Option<String>) {
        match self {
            Self::UploadBlob { content_hash, local_path, placement, .. } => {
                (TransferJobKind::UploadBook, content_hash, placement.as_ref().map(|value| value.content_hash.clone()), Path::new(local_path).file_name().and_then(|name| name.to_str()).map(str::to_owned))
            }
            Self::UploadThumbnail { thumbnail_hash, thumbnail_path, book_content_hash, .. } => {
                (TransferJobKind::UploadThumbnail, thumbnail_hash, book_content_hash.clone(), Path::new(thumbnail_path).file_name().and_then(|name| name.to_str()).map(str::to_owned))
            }
            Self::DownloadBlob { content_hash, placement, .. } => (TransferJobKind::DownloadBook, content_hash, Some(placement.content_hash.clone()), Some(placement.rel_path.as_str().to_owned())),
            Self::DownloadThumbnail { thumbnail_hash, book_content_hash, .. } => (TransferJobKind::DownloadThumbnail, thumbnail_hash, Some(book_content_hash.clone()), None),
        }
    }

    fn priority(&self) -> (u8, u8, u64) {
        let origin = if self.origin() == TransferOrigin::UserInitiated { 0 } else { 1 };
        match self {
            Self::DownloadThumbnail { .. } => (0, origin, 0),
            Self::UploadThumbnail { thumbnail_path, .. } => (0, origin, file_size_or(thumbnail_path, 0)),
            Self::UploadBlob { local_path, .. } => (1, origin, file_size_or(local_path, u64::MAX / 2)),
            Self::DownloadBlob { .. } => (1, origin, u64::MAX / 2),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueJobState {
    Queued,
    Running,
    Retrying,
}

#[derive(Clone, Debug)]
struct QueueJob {
    id: u64,
    job: TransferJob,
    state: QueueJobState,
    attempts: u32,
    next_retry_at: u64,
    last_error: Option<String>,
    created_at: u64,
    updated_at: u64,
}

#[derive(Clone, Debug)]
struct ClaimedTransferJob {
    id: u64,
    attempts: u32,
    job: TransferJob,
}

#[derive(Clone, Debug, Default)]
struct BatchProgress {
    completed: u32,
    total: u32,
    attempts: u32,
    created_at: u64,
    updated_at: u64,
    origin: TransferOrigin,
    permanent_error: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct TransferQueue {
    jobs: Mutex<HashMap<u64, QueueJob>>,
    progress: Mutex<HashMap<TransferJobKind, BatchProgress>>,
    history: Mutex<VecDeque<TransferStatus>>,
    history_db: Option<DbPool>,
    next_id: AtomicU64,
    draining: AtomicBool,
}

impl TransferQueue {
    pub(crate) fn with_history_db(history_db: DbPool) -> Self {
        Self { history_db: Some(history_db), ..Self::default() }
    }

    fn reconcile(&self, mut desired: Vec<TransferJob>) {
        desired.sort_by_key(TransferJob::priority);
        let now = unix_seconds();
        let mut jobs = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        jobs.retain(|_, queued| queued.state == QueueJobState::Running || desired.iter().any(|candidate| candidate.same_key(&queued.job)));
        for candidate in desired {
            if let Some(existing) = jobs.values_mut().find(|queued| queued.job.same_key(&candidate)) {
                if candidate.origin() == TransferOrigin::UserInitiated && existing.job.origin() != TransferOrigin::UserInitiated {
                    existing.job = existing.job.with_origin(TransferOrigin::UserInitiated);
                }
                existing.updated_at = now;
                continue;
            }
            let kind = candidate.status_fields().0;
            let origin = candidate.origin();
            let id = self.next_id.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            jobs.insert(id, QueueJob { id, job: candidate, state: QueueJobState::Queued, attempts: 0, next_retry_at: 0, last_error: None, created_at: now, updated_at: now });
            let mut progress = self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let value = progress.entry(kind).or_default();
            value.total = value.total.saturating_add(1);
            if value.created_at == 0 {
                value.created_at = now;
            }
            value.updated_at = now;
            if origin == TransferOrigin::UserInitiated {
                value.origin = TransferOrigin::UserInitiated;
            }
        }
    }

    fn claim_due(&self, allow_background_transfers: bool) -> Option<ClaimedTransferJob> {
        let now = unix_seconds();
        let mut jobs = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = jobs
            .values()
            .filter(|queued| matches!(queued.state, QueueJobState::Queued | QueueJobState::Retrying) && queued.next_retry_at <= now && (allow_background_transfers || queued.job.origin() == TransferOrigin::UserInitiated))
            .min_by_key(|queued| (queued.job.priority(), queued.created_at, queued.id))?
            .id;
        let queued = jobs.get_mut(&id)?;
        queued.state = QueueJobState::Running;
        queued.updated_at = now;
        Some(ClaimedTransferJob { id, attempts: queued.attempts, job: queued.job.clone() })
    }

    fn claim_due_batch(&self, allow_background_transfers: bool) -> Vec<ClaimedTransferJob> {
        let Some(first) = self.claim_due(allow_background_transfers) else { return Vec::new() };
        let Some(kind) = first.job.batch_kind() else { return vec![first] };
        let now = unix_seconds();
        let mut jobs = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut ids = jobs
            .values()
            .filter(|queued| {
                matches!(queued.state, QueueJobState::Queued | QueueJobState::Retrying)
                    && queued.next_retry_at <= now
                    && queued.job.batch_kind() == Some(kind)
                    && (allow_background_transfers || queued.job.origin() == TransferOrigin::UserInitiated)
            })
            .map(|queued| queued.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.truncate(MAX_THUMBNAIL_BATCH_ENTRIES.saturating_sub(1));
        let mut claimed = vec![first];
        for id in ids {
            if let Some(queued) = jobs.get_mut(&id) {
                queued.state = QueueJobState::Running;
                queued.updated_at = now;
                claimed.push(ClaimedTransferJob { id, attempts: queued.attempts, job: queued.job.clone() });
            }
        }
        claimed
    }

    fn complete(&self, id: u64, permanent_error: Option<String>) {
        let removed = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
        if let Some(removed) = removed {
            let (kind, content_hash, book_content_hash, path) = removed.job.status_fields();
            let content_hash = content_hash.clone();
            let now = unix_seconds();
            let mut progress = self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(value) = progress.get_mut(&kind) {
                value.completed = value.completed.saturating_add(1);
                value.attempts = value.attempts.max(removed.attempts);
                value.updated_at = now;
                if let Some(error) = permanent_error {
                    value.permanent_error = Some(error);
                }
                if value.completed >= value.total {
                    let completed = progress.remove(&kind).unwrap_or_default();
                    drop(progress);
                    let failed = completed.permanent_error.is_some();
                    let mut history = self.history.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let status = TransferStatus {
                        id: format!("history-{}-{}", kind.storage(), uuid::Uuid::new_v4()),
                        kind,
                        content_hash,
                        book_content_hash,
                        path,
                        state: if failed { TransferState::Failed } else { TransferState::Completed },
                        origin: completed.origin,
                        attempts: completed.attempts,
                        completed_items: completed.completed,
                        total_items: completed.total,
                        last_error: completed.permanent_error,
                        next_retry_at: None,
                        created_at: completed.created_at.to_string(),
                        updated_at: completed.updated_at.to_string(),
                    };
                    history.push_front(status.clone());
                    history.truncate(50);
                    drop(history);
                    self.persist_history(&status);
                }
            }
        }
    }

    fn fail(&self, id: u64, attempts: u32, error: String) -> Option<Duration> {
        let now = unix_seconds();
        let backoff = (2_u64.pow(attempts.min(8)) * 2).min(600);
        let delay = backoff + now % 3;
        let mut jobs = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let queued = jobs.get_mut(&id)?;
        queued.attempts = queued.attempts.saturating_add(1);
        queued.state = QueueJobState::Retrying;
        queued.last_error = Some(error);
        queued.next_retry_at = now.saturating_add(delay);
        queued.updated_at = now;
        Some(Duration::from_secs(delay))
    }

    fn release_for_authentication(&self, id: u64) {
        let now = unix_seconds();
        if let Some(queued) = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get_mut(&id) {
            queued.state = QueueJobState::Retrying;
            queued.last_error = Some(AUTHENTICATION_REQUIRED_MESSAGE.to_owned());
            queued.next_retry_at = now;
            queued.updated_at = now;
        }
    }

    fn has_due(&self, allow_background_transfers: bool) -> bool {
        let now = unix_seconds();
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .any(|queued| matches!(queued.state, QueueJobState::Queued | QueueJobState::Retrying) && queued.next_retry_at <= now && (allow_background_transfers || queued.job.origin() == TransferOrigin::UserInitiated))
    }

    pub(crate) fn statuses(&self) -> Vec<TransferStatus> {
        let jobs = self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let progress = self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let kinds = [TransferJobKind::UploadBook, TransferJobKind::UploadThumbnail, TransferJobKind::DownloadBook, TransferJobKind::DownloadThumbnail];
        kinds
            .into_iter()
            .filter_map(|kind| {
                let mut grouped = jobs.values().filter(|queued| queued.job.status_fields().0 == kind).collect::<Vec<_>>();
                grouped.sort_by_key(|queued| (queued.created_at, queued.id));
                let first = *grouped.first()?;
                let state = if grouped.iter().any(|queued| queued.state == QueueJobState::Running) {
                    TransferState::Running
                } else if grouped.iter().any(|queued| queued.state == QueueJobState::Retrying) {
                    TransferState::Retrying
                } else {
                    TransferState::Queued
                };
                let batch = progress.get(&kind).cloned().unwrap_or_else(|| BatchProgress { completed: 0, total: grouped.len() as u32, ..BatchProgress::default() });
                Some(TransferStatus {
                    id: format!("batch-{}", kind.storage()),
                    kind,
                    content_hash: first.job.status_fields().1.clone(),
                    book_content_hash: None,
                    path: None,
                    state,
                    origin: if grouped.iter().any(|queued| queued.job.origin() == TransferOrigin::UserInitiated) { TransferOrigin::UserInitiated } else { TransferOrigin::Background },
                    attempts: grouped.iter().map(|queued| queued.attempts).max().unwrap_or(0),
                    completed_items: batch.completed,
                    total_items: batch.total.max(grouped.len() as u32),
                    last_error: grouped.iter().find_map(|queued| queued.last_error.clone()),
                    next_retry_at: grouped.iter().filter_map(|queued| (queued.next_retry_at > 0).then_some(queued.next_retry_at)).min().map(|value| value.to_string()),
                    created_at: grouped.iter().map(|queued| queued.created_at).min().unwrap_or(first.created_at).to_string(),
                    updated_at: grouped.iter().map(|queued| queued.updated_at).max().unwrap_or(first.updated_at).to_string(),
                })
            })
            .collect()
    }

    pub(crate) fn history(&self) -> Vec<TransferStatus> {
        if let Some(history_db) = &self.history_db {
            let persisted = (|| -> Result<Vec<TransferStatus>, Box<dyn std::error::Error>> {
                let conn = history_db.get()?;
                let mut statement = conn.prepare("SELECT payload FROM transfer_history ORDER BY updated_at DESC, rowid DESC LIMIT 50")?;
                let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
                let mut history = Vec::new();
                for payload in rows {
                    match serde_json::from_str::<TransferStatus>(&payload?) {
                        Ok(status) => history.push(status),
                        Err(error) => log::warn!("Ignoring invalid persisted transfer history: {error}"),
                    }
                }
                Ok(history)
            })();
            match persisted {
                Ok(history) => return history,
                Err(error) => log::warn!("Could not load transfer history: {error}"),
            }
        }
        self.history.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect()
    }

    fn persist_history(&self, status: &TransferStatus) {
        let Some(history_db) = &self.history_db else { return };
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let conn = history_db.get()?;
            let payload = serde_json::to_string(status)?;
            let updated_at = status.updated_at.parse::<i64>()?;
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO transfer_history(id, payload, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET payload=excluded.payload, updated_at=excluded.updated_at",
                rusqlite::params![&status.id, payload, updated_at],
            )?;
            tx.execute(
                "DELETE FROM transfer_history WHERE id NOT IN (
                    SELECT id FROM transfer_history ORDER BY updated_at DESC, rowid DESC LIMIT 50
                 )",
                [],
            )?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            log::warn!("Could not persist transfer history: {error}");
        }
    }
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

#[derive(Clone)]
pub(crate) struct TransferWorker {
    http_client: Client,
    server_url: ServerUrl,
    library_id: LibraryId,
    library_root: PathBuf,
    db_pool: DbPool,
    event_tx: Option<crossbeam_channel::Sender<LibraryEvent>>,
    budget: Arc<Semaphore>,
    session: Arc<std::sync::RwLock<Option<crate::auth::Session>>>,
    cache_root: PathBuf,
    runtime: TransferRuntime,
    queue: Arc<TransferQueue>,
}

impl TransferWorker {
    pub(crate) fn new(
        http_client: Client, server_url: ServerUrl, library_id: LibraryId, library_root: PathBuf, db_pool: DbPool, event_tx: Option<crossbeam_channel::Sender<LibraryEvent>>, session: Arc<std::sync::RwLock<Option<crate::auth::Session>>>,
        cache_root: PathBuf, runtime: TransferRuntime, queue: Arc<TransferQueue>,
    ) -> Self {
        let byte_budget_mb = std::env::var("BOKHEIM_TRANSFER_BUDGET_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(64);
        Self { http_client, server_url, library_id, library_root, db_pool, event_tx, budget: Arc::new(Semaphore::new(byte_budget_mb.max(1))), session, cache_root, runtime, queue }
    }

    pub(crate) async fn run(self, jobs: Vec<TransferJob>, worker_count: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.queue.reconcile(jobs);
        self.send_transfer_status_changed();

        self.run_pending(worker_count).await.map_err(|error| error as Box<dyn std::error::Error>)
    }

    pub(crate) async fn run_pending(self, worker_count: usize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.queue.draining.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Ok(());
        }
        let worker = Arc::new(self);
        let result = loop {
            let mut handles = Vec::new();
            for _ in 0..worker_count.max(1) {
                let worker = Arc::clone(&worker);
                handles.push(tokio::spawn(async move { worker.run_queue().await }));
            }
            let mut batch_result = Ok(());
            for handle in handles {
                match handle.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) if error == AUTHENTICATION_REQUIRED_MESSAGE => batch_result = Err(Box::new(AuthenticationRequired) as Box<dyn std::error::Error + Send + Sync>),
                    Ok(Err(error)) => batch_result = Err(format!("transfer queue failed: {error}").into()),
                    Err(error) => batch_result = Err(format!("transfer worker task failed: {error}").into()),
                }
            }
            if batch_result.is_err() || !worker.queue.has_due(worker.runtime.allows_background_transfers()) {
                break batch_result;
            }
        };
        worker.queue.draining.store(false, Ordering::Release);
        if result.is_ok() && worker.queue.has_due(worker.runtime.allows_background_transfers()) {
            schedule_queue_drain(Arc::new(worker.as_ref().clone()), Duration::ZERO);
        }
        result
    }

    async fn run_queue(self: &Arc<Self>) -> Result<(), String> {
        loop {
            let claimed = self.queue.claim_due_batch(self.runtime.allows_background_transfers());
            if claimed.is_empty() {
                break;
            }
            self.send_transfer_status_changed();
            let result = self.handle_claimed_batch(&claimed).await;
            match result {
                Ok(()) => {
                    for item in &claimed {
                        self.queue.complete(item.id, None);
                    }
                }
                Err(err) => {
                    if err.downcast_ref::<AuthenticationRequired>().is_some() {
                        for item in &claimed {
                            self.queue.release_for_authentication(item.id);
                        }
                        self.send_transfer_status_changed();
                        return Err(AUTHENTICATION_REQUIRED_MESSAGE.to_owned());
                    }
                    match classify_failure(err.as_ref()) {
                        TransferFailure::Retryable(message) => {
                            let mut delay = None;
                            for item in &claimed {
                                delay = delay.or_else(|| self.queue.fail(item.id, item.attempts, message.clone()));
                            }
                            if let Some(delay) = delay {
                                schedule_queue_drain(Arc::clone(self), delay);
                            }
                            log::warn!("Retryable transfer error: {message}");
                        }
                        TransferFailure::Permanent(message) => {
                            for item in &claimed {
                                self.queue.complete(item.id, Some(message.clone()));
                            }
                            log::warn!("Permanent transfer error: {message}");
                        }
                    }
                }
            }
            self.send_transfer_status_changed();
        }
        Ok(())
    }

    fn send_transfer_status_changed(&self) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(LibraryEvent::TransferStatusChanged);
        }
    }
}

fn schedule_queue_drain(worker: Arc<TransferWorker>, delay: Duration) {
    tokio::spawn(async move {
        sleep(delay).await;
        let drain: std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>> = Box::pin(worker.run_queue());
        if let Err(error) = drain.await {
            log::warn!("Transfer retry scheduler stopped: {error}");
        }
    });
}

fn file_size_or(path: &str, default: u64) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(default)
}

impl TransferWorker {
    fn asset_endpoint(&self, namespace: &str, content_hash: &ContentHash) -> reqwest::Url {
        self.server_url.endpoint(&format!("api/libraries/{}/{namespace}/{}", self.library_id, encode(content_hash.as_str())))
    }

    async fn handle_claimed_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), Box<dyn std::error::Error>> {
        match claimed.first().map(|item| item.job.batch_kind()) {
            Some(Some(TransferJobKind::UploadThumbnail)) => self.upload_thumbnail_batch(claimed).await,
            Some(Some(TransferJobKind::DownloadThumbnail)) => self.download_thumbnail_batch(claimed).await,
            Some(_) => self.handle_job(claimed[0].job.clone()).await,
            None => Ok(()),
        }
    }

    async fn handle_job(&self, job: TransferJob) -> Result<(), Box<dyn std::error::Error>> {
        match job {
            TransferJob::UploadBlob { content_hash, local_path, placement, origin: _ } => {
                let permits = mb_permits_for_path(&local_path, self.budget.available_permits()).await;
                let _permit = self.budget.clone().acquire_many_owned(permits).await?;
                self.upload_blob(&content_hash, &local_path, placement.as_ref()).await?;
            }
            TransferJob::UploadThumbnail { thumbnail_hash, thumbnail_path, book_content_hash: _, origin: _ } => {
                let permits = mb_permits_for_path(&thumbnail_path, self.budget.available_permits()).await;
                let _permit = self.budget.clone().acquire_many_owned(permits).await?;
                self.upload_thumbnail(&thumbnail_hash, &thumbnail_path).await?;
            }
            TransferJob::DownloadBlob { content_hash, placement, origin: _ } => {
                let _permit = self.budget.clone().acquire_owned().await?;
                self.download_blob(&content_hash, &placement).await?;
            }
            TransferJob::DownloadThumbnail { thumbnail_hash, book_content_hash, origin: _ } => {
                let _permit = self.budget.clone().acquire_owned().await?;
                self.download_thumbnail(&thumbnail_hash, book_content_hash).await?;
            }
        }
        Ok(())
    }

    fn bearer_token(&self) -> Result<String, Box<dyn std::error::Error>> {
        self.session.read().ok().and_then(|session| session.as_ref().map(|session| session.token().to_owned())).ok_or_else(|| Box::new(AuthenticationRequired) as Box<dyn std::error::Error>)
    }

    fn authenticated_response(&self, response: reqwest::Response) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
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
    fn thumbnail_batch_endpoint(&self) -> reqwest::Url {
        self.server_url.endpoint(&format!("api/libraries/{}/thumbnail-batch", self.library_id))
    }

    async fn upload_thumbnail_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), Box<dyn std::error::Error>> {
        let token = self.bearer_token()?;
        let mut thumbnails = Vec::with_capacity(claimed.len());
        let mut uploaded = Vec::with_capacity(claimed.len());
        for item in claimed {
            let TransferJob::UploadThumbnail { thumbnail_hash, thumbnail_path, .. } = &item.job else { return Err(permanent("mixed thumbnail upload batch")) };
            if !Path::new(thumbnail_path).exists() {
                continue;
            }
            let (mut file, len) = open_limited_file(Path::new(thumbnail_path), MAX_THUMBNAIL_BYTES).await?;
            let mut bytes = Vec::with_capacity(len as usize);
            file.read_to_end(&mut bytes).await?;
            if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                return Err(permanent("thumbnail must be a JPEG"));
            }
            thumbnails.push(ThumbnailBatchEntry { content_hash: thumbnail_hash.clone(), bytes });
            uploaded.push((thumbnail_hash.clone(), thumbnail_path.clone()));
        }
        if thumbnails.is_empty() {
            return Ok(());
        }
        let body = sync_common::transport::encode(&ThumbnailBatchUploadRequest { thumbnails })?;
        let response = self
            .http_client
            .put(self.thumbnail_batch_endpoint())
            .bearer_auth(&token)
            .header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE)
            .header(reqwest::header::CONTENT_TYPE, sync_common::transport::MEDIA_TYPE)
            .body(body)
            .send()
            .await?;
        self.authenticated_response(response)?;
        let conn = self.db_pool.get()?;
        for (hash, path) in uploaded {
            db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, &hash, &path, true)?;
            db::record_remote_asset(&conn, BlobKind::Thumbnail, &hash)?;
        }
        Ok(())
    }

    async fn download_thumbnail_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), Box<dyn std::error::Error>> {
        let token = self.bearer_token()?;
        let mut requested = HashMap::<ContentHash, ContentHash>::new();
        for item in claimed {
            let TransferJob::DownloadThumbnail { thumbnail_hash, book_content_hash, .. } = &item.job else { return Err(permanent("mixed thumbnail download batch")) };
            let thumb_path = thumbnail_path_from_content_hash(&self.library_root, thumbnail_hash);
            if thumb_path.exists() {
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(LibraryEvent::ThumbnailGenerated { content_hash: book_content_hash.clone() });
                }
            } else {
                requested.insert(thumbnail_hash.clone(), book_content_hash.clone());
            }
        }
        if requested.is_empty() {
            return Ok(());
        }
        let body = sync_common::transport::encode(&ThumbnailBatchDownloadRequest { content_hashes: requested.keys().cloned().collect() })?;
        let response = self
            .http_client
            .post(self.thumbnail_batch_endpoint())
            .bearer_auth(&token)
            .header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE)
            .header(reqwest::header::CONTENT_TYPE, sync_common::transport::MEDIA_TYPE)
            .body(body)
            .send()
            .await?;
        let response = self.authenticated_response(response)?;
        let bytes = response.bytes().await?;
        let response: ThumbnailBatchDownloadResponse = sync_common::transport::decode(&bytes, sync_common::api::assets::MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024)?;
        let mut answered = std::collections::HashSet::new();
        for thumbnail in response.thumbnails {
            let Some(book_hash) = requested.get(&thumbnail.content_hash) else { return Err(permanent("thumbnail batch returned an unrequested hash")) };
            if !answered.insert(thumbnail.content_hash.clone()) || thumbnail.bytes.len() > MAX_THUMBNAIL_BYTES as usize {
                return Err(permanent("thumbnail batch returned invalid bytes"));
            }
            self.publish_thumbnail(&thumbnail.content_hash, book_hash, &thumbnail.bytes).await?;
        }
        let conn = self.db_pool.get()?;
        for hash in response.missing {
            if !requested.contains_key(&hash) || !answered.insert(hash.clone()) {
                return Err(permanent("thumbnail batch returned an invalid missing hash"));
            }
            db::forget_remote_asset(&conn, BlobKind::Thumbnail, &hash)?;
        }
        if answered.len() != requested.len() {
            return Err(permanent("thumbnail batch omitted requested hashes"));
        }
        Ok(())
    }

    async fn publish_thumbnail(&self, thumbnail_hash: &ContentHash, book_content_hash: &ContentHash, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let thumb_path = thumbnail_path_from_content_hash(&self.library_root, thumbnail_hash);
        if let Some(parent) = thumb_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp_path = temporary_sibling_path(&thumb_path);
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        if let Err(error) = tokio::fs::rename(&tmp_path, &thumb_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(error.into());
        }
        sync_parent_directory(&thumb_path)?;
        let thumb_path_text = thumb_path.to_str().ok_or("thumbnail path is not valid UTF-8")?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, thumbnail_hash, thumb_path_text, true)?;
        db::record_remote_asset(&conn, BlobKind::Thumbnail, thumbnail_hash)?;
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(LibraryEvent::ThumbnailGenerated { content_hash: book_content_hash.clone() });
        }
        Ok(())
    }

    async fn upload_blob(&self, content_hash: &ContentHash, local_path: &str, placement: Option<&BookPlacement>) -> Result<(), Box<dyn std::error::Error>> {
        let local_path_text = local_path;
        let local_path = Path::new(local_path_text);
        let token = self.bearer_token()?;
        let (file, len, actual_hash) = open_hashed_file(local_path).await?;
        if actual_hash.as_str() != content_hash.as_str() {
            let conn = self.db_pool.get()?;
            db::upsert_blob_local_ref(&conn, BlobKind::Book, content_hash, local_path_text, false)?;
            if let Some(placement) = placement {
                db::set_book_downloaded(&conn, &placement.rel_path, placement.content_hash.clone(), false)?;
            }
            return Err(permanent(format!("upload hash mismatch expected={content_hash} got={actual_hash}")));
        }
        let response = self.http_client.put(self.asset_endpoint("blobs", content_hash)).bearer_auth(&token).header(reqwest::header::CONTENT_LENGTH, len).body(reqwest::Body::wrap_stream(ReaderStream::new(file))).send().await?;
        self.authenticated_response(response)?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Book, content_hash, local_path_text, true)?;
        db::record_remote_asset(&conn, BlobKind::Book, content_hash)?;
        if let Some(placement) = placement {
            db::set_book_downloaded(&conn, &placement.rel_path, placement.content_hash.clone(), true)?;
        }
        Ok(())
    }

    async fn upload_thumbnail(&self, content_hash: &ContentHash, thumbnail_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        if !Path::new(thumbnail_path).exists() {
            return Ok(());
        }
        let token = self.bearer_token()?;
        let thumbnail_path_text = thumbnail_path;
        let thumbnail_path = Path::new(thumbnail_path_text);
        let (mut file, len) = open_limited_file(thumbnail_path, MAX_THUMBNAIL_BYTES).await?;
        let mut signature = [0_u8; 3];
        file.read_exact(&mut signature).await?;
        if signature != [0xff, 0xd8, 0xff] {
            return Err(permanent("thumbnail must be a JPEG"));
        }
        file.seek(SeekFrom::Start(0)).await?;
        let response = self.http_client.put(self.asset_endpoint("thumbnails", content_hash)).bearer_auth(&token).header(reqwest::header::CONTENT_LENGTH, len).body(reqwest::Body::wrap_stream(ReaderStream::new(file))).send().await?;
        self.authenticated_response(response)?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, content_hash, thumbnail_path_text, true)?;
        db::record_remote_asset(&conn, BlobKind::Thumbnail, content_hash)?;
        Ok(())
    }

    async fn download_blob(&self, content_hash: &ContentHash, placement: &BookPlacement) -> Result<(), Box<dyn std::error::Error>> {
        let token = self.bearer_token()?;
        let rel_path = &placement.rel_path;
        let local_path = rel_path.resolve_under(&self.library_root);
        let local_path_text = local_path.to_str().ok_or("book path is not valid UTF-8")?;
        self.send_download_status(content_hash.clone(), DownloadState::Downloading(DownloadProgress::ZERO));

        let partial_path = partial_book_path(&self.library_root, content_hash);
        if let Some(parent) = partial_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(account_id) = self.session.read().ok().and_then(|session| session.as_ref().map(|session| session.user_id().to_owned())) {
            remote_reader::copy_cached_prefix(&self.cache_root, &account_id, &self.library_id, content_hash, &partial_path)?;
        }
        let mut file = tokio::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&partial_path).await?;
        let mut downloaded = file.metadata().await?.len();
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
        let partial_is_complete = if downloaded == 0 { false } else { hasher.clone().finalize().to_hex().as_str() == content_hash.as_str() };

        if !partial_is_complete {
            let total_size;
            let mut request = self.http_client.get(self.asset_endpoint("blobs", content_hash)).bearer_auth(&token);
            if downloaded > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={downloaded}-")).header(reqwest::header::IF_RANGE, format!("\"{}\"", content_hash.as_str()));
            }
            let response = request.send().await?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                let conn = self.db_pool.get()?;
                db::forget_remote_asset(&conn, BlobKind::Book, content_hash)?;
                self.send_download_status(content_hash.clone(), DownloadState::NotDownloaded);
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
            file.seek(SeekFrom::Start(downloaded)).await?;
            let mut stream = response.bytes_stream();
            let mut last_progress = DownloadProgress::from_ratio(downloaded, total_size).value();
            let bandwidth_limit = std::env::var("BOKHEIM_DOWNLOAD_BANDWIDTH").ok().and_then(|value| value.parse::<f64>().ok()).filter(|&limit| limit > 0.0);
            let mut next_allowed = Instant::now();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
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
                    self.send_download_status(content_hash.clone(), DownloadState::Downloading(progress));
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
            self.send_download_status(content_hash.clone(), DownloadState::NotDownloaded);
            return Err(permanent(format!("download hash mismatch expected={} got={}", content_hash, got_hash)));
        }

        {
            let conn = self.db_pool.get()?;
            if !db::book_placement_expects_hash(&conn, placement, content_hash)? {
                self.send_download_status(content_hash.clone(), DownloadState::NotDownloaded);
                return Ok(());
            }
            if let Some((_, dir_id, _)) = db::get_book_dir_entry_by_path(&conn, rel_path)? {
                db::materialize_dir_with_markers(&conn, self.library_root.as_path(), &dir_id.to_string())?;
            }
        }
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        #[cfg(target_os = "android")]
        {
            // Android SELinux denies hard-link creation inside the app
            // sandbox. Downloads for a placement are serialized, so check an
            // existing destination before atomically renaming the fully
            // flushed partial file into place.
            if tokio::fs::try_exists(&local_path).await? {
                let (_, _, existing_hash) = open_hashed_file(&local_path).await?;
                if existing_hash != content_hash.as_str() {
                    return Err(permanent(format!("refusing to overwrite existing file at {}", local_path.display())));
                }
                tokio::fs::remove_file(&partial_path).await?;
            } else if let Err(error) = tokio::fs::rename(&partial_path, &local_path).await {
                let _ = tokio::fs::remove_file(&partial_path).await;
                return Err(error.into());
            }
        }

        #[cfg(not(target_os = "android"))]
        match tokio::fs::hard_link(&partial_path, &local_path).await {
            Ok(()) => {
                tokio::fs::remove_file(&partial_path).await?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let (_, _, existing_hash) = open_hashed_file(&local_path).await?;
                if existing_hash != content_hash.as_str() {
                    return Err(permanent(format!("refusing to overwrite existing file at {}", local_path.display())));
                }
                tokio::fs::remove_file(&partial_path).await?;
            }
            Err(error) => return Err(error.into()),
        }
        sync_parent_directory(&local_path)?;

        let conn = self.db_pool.get()?;
        if let Err(err) = db::stamp_book_dir_local_hash_by_rel_path(&conn, rel_path, content_hash.clone(), &local_path) {
            log::warn!("Failed to stamp local fingerprint for downloaded {}: {}", local_path.display(), err);
        }
        db::upsert_blob_local_ref(&conn, BlobKind::Book, content_hash, local_path_text, true)?;
        db::record_remote_asset(&conn, BlobKind::Book, content_hash)?;
        db::complete_asset_request(&conn, BlobKind::Book, content_hash)?;
        db::set_book_downloaded(&conn, rel_path, content_hash.clone(), true)?;
        self.send_download_status(content_hash.clone(), DownloadState::Downloaded);
        Ok(())
    }

    fn send_download_status(&self, content_hash: ContentHash, state: DownloadState) {
        if let Some(tx) = &self.event_tx {
            let refresh_folders = matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded);
            let _ = tx.send(LibraryEvent::DownloadStatusChanged { content_hash: content_hash.clone(), state });
            if refresh_folders {
                let folder_statuses = (|| -> Result<Vec<_>, Box<dyn std::error::Error>> {
                    let conn = self.db_pool.get()?;
                    let dir_ids = db::get_book_ancestor_dir_ids(&conn, content_hash)?;
                    dir_ids.into_iter().map(|dir_id| db::get_folder_download_counts(&conn, &dir_id.to_string()).map(|(downloaded_books, total_books)| (dir_id, downloaded_books, total_books)).map_err(Into::into)).collect()
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

    async fn download_thumbnail(&self, thumbnail_hash: &ContentHash, book_content_hash: ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let thumb_path = thumbnail_path_from_content_hash(&self.library_root, thumbnail_hash);
        if thumb_path.exists() {
            if let Some(tx) = &self.event_tx {
                let _ = tx.send(LibraryEvent::ThumbnailGenerated { content_hash: book_content_hash });
            }
            return Ok(());
        }

        let token = self.bearer_token()?;
        let response = self.http_client.get(self.asset_endpoint("thumbnails", thumbnail_hash)).bearer_auth(&token).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            let conn = self.db_pool.get()?;
            db::forget_remote_asset(&conn, BlobKind::Thumbnail, thumbnail_hash)?;
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
        if let Some(parent) = thumb_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp_path = temporary_sibling_path(&thumb_path);
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        // The thumbnail is content-addressed, so concurrent writers publish
        // equivalent bytes. Rename keeps the final path invisible until the
        // complete temporary file has been flushed and also works in Android
        // app sandboxes, where SELinux denies hard-link creation.
        if let Err(error) = tokio::fs::rename(&tmp_path, &thumb_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(error.into());
        }
        sync_parent_directory(&thumb_path)?;
        let thumb_path_text = thumb_path.to_str().ok_or("thumbnail path is not valid UTF-8")?;
        let conn = self.db_pool.get()?;
        db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, thumbnail_hash, thumb_path_text, true)?;
        db::record_remote_asset(&conn, BlobKind::Thumbnail, thumbnail_hash)?;
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(LibraryEvent::ThumbnailGenerated { content_hash: book_content_hash });
        }
        Ok(())
    }
}

pub(crate) fn thumbnail_path_from_content_hash(library_root: &Path, content_hash: &ContentHash) -> PathBuf {
    library_root.join(crate::APP_HIDDEN_DIR).join("thumbnails").join(format!("{content_hash}.jpg"))
}

fn partial_book_path(library_root: &Path, content_hash: &ContentHash) -> PathBuf {
    library_root.join(crate::APP_HIDDEN_DIR).join("partials").join(format!("{content_hash}.part"))
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

async fn open_hashed_file(path: &Path) -> Result<(tokio::fs::File, u64, String), Box<dyn std::error::Error>> {
    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let mut hasher = Hasher::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        hasher.update(&buffer[..count]);
    }
    if total != len {
        return Err(permanent("asset changed size while it was being prepared for upload"));
    }
    file.seek(SeekFrom::Start(0)).await?;
    Ok((file, len, hasher.finalize().to_hex().to_string()))
}

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    // Android app sandboxes deny opening directories as files, which makes
    // the Unix directory-fsync durability step fail with EACCES. The file was
    // already synced before publication, so skip this unsupported extra step.
    #[cfg(target_os = "android")]
    {
        let _ = path;
        return Ok(());
    }

    #[cfg(not(target_os = "android"))]
    {
        let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "published asset has no parent"))?;
        std::fs::File::open(parent)?.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RelativeBookPath;
    use axum::body::Body;
    use axum::http::{HeaderMap, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::atomic::{AtomicBool, Ordering};
    use test_support::fixture_content_hash;

    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_session(server_url: &ServerUrl) -> Arc<std::sync::RwLock<Option<crate::auth::Session>>> {
        Arc::new(std::sync::RwLock::new(Some(crate::auth::Session::try_new(server_url.clone(), "test-user".to_string(), "test-reader".to_string(), TEST_TOKEN.to_string()).unwrap())))
    }

    fn queued_upload(hash: &str, origin: TransferOrigin) -> TransferJob {
        TransferJob::UploadBlob { content_hash: ContentHash::new(&hash), local_path: "/tmp/book.epub".to_owned(), placement: None, origin }
    }

    #[test]
    fn ephemeral_queue_deduplicates_and_preserves_user_priority() {
        let queue = TransferQueue::default();
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background), queued_upload(hash, TransferOrigin::UserInitiated)]);
        queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);

        let statuses = queue.statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].origin, TransferOrigin::UserInitiated);
        assert_eq!(statuses[0].state, TransferState::Queued);
    }

    #[test]
    fn transfer_statuses_are_aggregated_with_stable_batch_progress() {
        let queue = TransferQueue::default();
        let first = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let second = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        queue.reconcile(vec![queued_upload(first, TransferOrigin::Background), queued_upload(second, TransferOrigin::Background)]);
        let initial = queue.statuses();
        assert_eq!(initial.len(), 1);
        assert_eq!((initial[0].completed_items, initial[0].total_items), (0, 2));

        let completed = queue.claim_due(true).unwrap();
        queue.complete(completed.id, None);
        let progressed = queue.statuses();
        assert_eq!(progressed.len(), 1);
        assert_eq!((progressed[0].completed_items, progressed[0].total_items), (1, 2));

        let completed = queue.claim_due(true).unwrap();
        queue.complete(completed.id, None);
        assert!(queue.statuses().is_empty());
        let history = queue.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, TransferState::Completed);
        assert_eq!((history[0].completed_items, history[0].total_items), (2, 2));
    }

    #[test]
    fn completed_batch_history_survives_queue_recreation() {
        let root = tempfile::tempdir().unwrap();
        let manager = SqliteConnectionManager::file(root.path().join("library.db")).with_init(|connection| db::configure_connection(connection));
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        db::init_db(&pool.get().unwrap()).unwrap();
        let queue = TransferQueue::with_history_db(pool.clone());
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);
        let completed = queue.claim_due(true).unwrap();
        queue.complete(completed.id, None);
        drop(queue);

        let restored = TransferQueue::with_history_db(pool);
        let history = restored.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, TransferState::Completed);
        assert_eq!((history[0].completed_items, history[0].total_items), (1, 1));
    }

    #[test]
    fn thumbnail_network_work_is_claimed_in_bounded_batches() {
        let queue = TransferQueue::default();
        let jobs = (0..20).map(|index| TransferJob::DownloadThumbnail { thumbnail_hash: fixture_content_hash(index), book_content_hash: fixture_content_hash(index + 100), origin: TransferOrigin::Background }).collect();
        queue.reconcile(jobs);
        assert_eq!(queue.claim_due_batch(true).len(), MAX_THUMBNAIL_BATCH_ENTRIES);
        assert_eq!(queue.claim_due_batch(true).len(), 20 - MAX_THUMBNAIL_BATCH_ENTRIES);
    }

    #[test]
    fn thumbnails_are_claimed_before_user_requested_books() {
        let queue = TransferQueue::default();
        let thumbnail_hash = fixture_content_hash(1);
        let book_hash = fixture_content_hash(2);
        queue.reconcile(vec![
            TransferJob::DownloadBlob { content_hash: book_hash, placement: BookPlacement::new(book_hash, RelativeBookPath::parse("/book.epub").unwrap()), origin: TransferOrigin::UserInitiated },
            TransferJob::DownloadThumbnail { thumbnail_hash, book_content_hash: book_hash, origin: TransferOrigin::Background },
        ]);

        let claimed = queue.claim_due(true).unwrap();
        assert!(matches!(claimed.job, TransferJob::DownloadThumbnail { .. }));
    }

    #[test]
    fn ephemeral_queue_pauses_background_jobs_and_tracks_retries() {
        let queue = TransferQueue::default();
        let background = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let manual = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        queue.reconcile(vec![queued_upload(background, TransferOrigin::Background), queued_upload(manual, TransferOrigin::UserInitiated)]);

        let claimed = queue.claim_due(false).expect("manual transfer remains eligible");
        assert_eq!(claimed.job.origin(), TransferOrigin::UserInitiated);
        assert!(queue.fail(claimed.id, claimed.attempts, "network unavailable".to_owned()).is_some());
        let retrying = queue.statuses().into_iter().find(|status| status.content_hash == ContentHash::new(manual)).unwrap();
        assert_eq!(retrying.state, TransferState::Retrying);
        assert_eq!(retrying.attempts, 1);
        assert_eq!(retrying.last_error.as_deref(), Some("network unavailable"));
        assert!(queue.claim_due(false).is_none());

        let background = queue.claim_due(true).expect("background transfer resumes when allowed");
        assert_eq!(background.job.origin(), TransferOrigin::Background);
    }

    #[test]
    fn authentication_pause_does_not_consume_retry_budget() {
        let queue = TransferQueue::default();
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);
        let rejected = queue.claim_due(true).unwrap();
        queue.release_for_authentication(rejected.id);

        let retried = queue.claim_due(true).unwrap();
        assert_eq!(retried.id, rejected.id);
        assert_eq!(retried.attempts, rejected.attempts);
    }

    #[test]
    fn thumbnail_paths_are_derived_from_the_validated_library_root() {
        let root = tempfile::tempdir().unwrap();
        let library_root = root.path().canonicalize().unwrap();
        let content_hash = ContentHash::new(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(
            thumbnail_path_from_content_hash(&library_root, &content_hash),
            root.path().canonicalize().unwrap().join(format!("{}/thumbnails/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jpg", crate::APP_HIDDEN_DIR))
        );
    }

    /// The upload path deliberately does not inspect book format; the library is
    /// a catalogue of the user's files, not an upload trust boundary. What it
    /// must still refuse is bytes that do not match the content hash they claim
    /// to be, and it must refuse them before any network I/O.
    #[tokio::test]
    async fn upload_rejects_bytes_that_do_not_match_their_content_hash_before_network_io() {
        let root = tempfile::tempdir().unwrap();
        let book_path = root.path().join("book.epub");
        std::fs::write(&book_path, b"bytes that hash to something else").unwrap();
        let manager = SqliteConnectionManager::file(root.path().join("library.db"));
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        db::init_db(&pool.get().unwrap()).unwrap();
        let server_url = ServerUrl::parse("http://127.0.0.1:9").unwrap();
        let worker = TransferWorker::new(
            Client::new(),
            server_url.clone(),
            test_support::fixture_library_id("library"),
            root.path().canonicalize().unwrap(),
            pool.clone(),
            None,
            test_session(&server_url),
            root.path().join("cache"),
            TransferRuntime::default(),
            Arc::new(TransferQueue::default()),
        );
        let expected = ContentHash::new(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        let error = worker.upload_blob(&expected, book_path.to_str().unwrap(), None).await.unwrap_err();

        assert!(error.to_string().contains("upload hash mismatch"), "unexpected error: {error}");
        // The rejection records that these bytes are not the claimed content, so
        // no reference may claim the blob is present on this device.
        let present_refs: i64 = pool.get().unwrap().query_row("SELECT COUNT(*) FROM blob_local_ref WHERE kind='book' AND hash=?1 AND exists_flag=1", [expected.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(present_refs, 0);
    }

    fn worker_with_placement(root: &Path, server_url: ServerUrl, content_hash: &ContentHash) -> (TransferWorker, DbPool) {
        let manager = SqliteConnectionManager::file(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH));
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        let conn = pool.get().unwrap();
        db::init_db(&conn).unwrap();
        let dir_id = test_support::fixture_dir_id("d1").to_string();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, '00000000-0000-0000-0000-000000000000', 'Shelf')", [dir_id.as_str()]).unwrap();
        conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES (?2, (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', '', 0)", rusqlite::params![content_hash.as_str(), dir_id])
            .unwrap();
        drop(conn);
        let worker = TransferWorker::new(
            Client::new(),
            server_url.clone(),
            test_support::fixture_library_id("library"),
            root.canonicalize().unwrap(),
            pool.clone(),
            None,
            test_session(&server_url),
            root.join("cache"),
            TransferRuntime::default(),
            Arc::new(TransferQueue::default()),
        );
        (worker, pool)
    }

    #[tokio::test]
    async fn interrupted_download_resumes_from_saved_prefix_and_publishes_verified_bytes() {
        let bytes = b"a complete epub payload used to prove ranged resume".to_vec();
        let content_hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
        let prefix_len = 17_usize;
        let saw_range = Arc::new(AtomicBool::new(false));
        let route_bytes = Arc::new(bytes.clone());
        let route_hash = content_hash.clone();
        let route_saw_range = Arc::clone(&saw_range);
        let route = format!("/api/libraries/{}/blobs/:hash", test_support::fixture_library_id("library"));
        let app = Router::new().route(
            &route,
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
        std::fs::write(partial_book_path(&root.path().canonicalize().unwrap(), &content_hash), &bytes[..prefix_len]).unwrap();
        let (worker, _) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);
        let placement = BookPlacement::new(content_hash, RelativeBookPath::parse("/Shelf/book.epub").unwrap());

        worker.download_blob(&content_hash, &placement).await.unwrap();

        assert!(saw_range.load(Ordering::SeqCst));
        assert_eq!(std::fs::read(root.path().join("Shelf/book.epub")).unwrap(), bytes);
        assert!(!partial_book_path(&root.path().canonicalize().unwrap(), &content_hash).exists());
        server.abort();
    }

    #[tokio::test]
    async fn unauthorized_transfer_keeps_the_job_retryable_and_preserves_refresh_credentials() {
        let route = format!("/api/libraries/{}/thumbnails/:hash", test_support::fixture_library_id("library"));
        let app = Router::new().route(&route, get(|| async { StatusCode::UNAUTHORIZED }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let content_hash = ContentHash::new(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let (worker, _) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);

        let error = worker.download_thumbnail(&content_hash, fixture_content_hash(7)).await.unwrap_err();

        assert!(matches!(classify_failure(error.as_ref()), TransferFailure::Retryable(_)));
        assert!(worker.session.read().unwrap().is_some(), "the account layer must retain the refresh token so it can renew access before retrying");
        server.abort();
    }

    #[tokio::test]
    async fn completed_download_is_not_published_after_placement_becomes_stale() {
        let bytes = b"verified but stale bytes".to_vec();
        let content_hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(format!("{}/partials", crate::APP_HIDDEN_DIR))).unwrap();
        let library_root = root.path().canonicalize().unwrap();
        let partial = partial_book_path(&library_root, &content_hash);
        std::fs::write(&partial, &bytes).unwrap();
        let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse("http://127.0.0.1:9").unwrap(), &content_hash);
        pool.get().unwrap().execute("UPDATE book_dir SET deleted_at=1", []).unwrap();
        let placement = BookPlacement::new(fixture_content_hash(7), RelativeBookPath::parse("/Shelf/book.epub").unwrap());

        worker.download_blob(&content_hash, &placement).await.unwrap();

        assert!(!root.path().join("Shelf/book.epub").exists());
        assert!(partial.exists(), "verified content remains reusable for a later live placement");
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

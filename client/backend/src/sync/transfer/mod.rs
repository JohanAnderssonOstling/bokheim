use crate::platform::library_events::LibraryEvent;
use crate::sync::DbPool;
use reqwest::Client;
use std::fmt;
use std::sync::{Arc, RwLock};
use tokio::sync::{oneshot, Semaphore};

#[cfg(test)]
use crate::library::database as db;
use crate::{BookPlacement, ContentHash, TransferJobKind};
#[cfg(test)]
use sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES;
use sync_common::LibraryId;

type TransferRunResult = Result<(), TransferError>;

pub use library_replica::{LibraryTransfers, NetworkStatus, NetworkTransport, TransferOrigin, TransferState, TransferStatus};

mod executor;
mod queue;
pub(crate) use queue::TransferQueue;

#[derive(Clone, Debug, Default)]
pub(crate) struct TransferRuntime {
    state: Arc<RwLock<NetworkStatus>>,
}

impl TransferRuntime {
    pub(crate) fn network_status(&self) -> NetworkStatus {
        self.state.read().map(|status| *status).unwrap_or_default()
    }

    pub(crate) fn set_network_status(&self, status: NetworkStatus) -> bool {
        let Ok(mut current) = self.state.write() else { return false };
        let reconnected = !current.connected && status.connected;
        *current = status;
        reconnected
    }
}

const AUTHENTICATION_REQUIRED_MESSAGE: &str = "authentication access token was rejected";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TransferError {
    AuthenticationRequired,
    Retryable(String),
    Rejected(String),
}

impl TransferError {
    fn retryable(error: impl fmt::Display) -> Self {
        Self::Retryable(error.to_string())
    }

    fn rejected(message: impl Into<String>) -> Self {
        Self::Rejected(message.into())
    }
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str(AUTHENTICATION_REQUIRED_MESSAGE),
            Self::Retryable(message) | Self::Rejected(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TransferError {}

impl From<sync_client::AssetResponseError> for TransferError {
    fn from(error: sync_client::AssetResponseError) -> Self {
        match error {
            sync_client::AssetResponseError::AuthenticationRequired => Self::AuthenticationRequired,
            sync_client::AssetResponseError::StorageQuotaExceeded => Self::Retryable("book storage quota exceeded".to_owned()),
            sync_client::AssetResponseError::Retryable(error) => Self::Retryable(error.to_string()),
            sync_client::AssetResponseError::Rejected(error) => Self::Retryable(error.to_string()),
        }
    }
}

impl From<std::io::Error> for TransferError {
    fn from(error: std::io::Error) -> Self {
        Self::retryable(error)
    }
}

impl From<rusqlite::Error> for TransferError {
    fn from(error: rusqlite::Error) -> Self {
        Self::retryable(error)
    }
}

impl From<crate::platform::database::DatabaseError> for TransferError {
    fn from(error: crate::platform::database::DatabaseError) -> Self {
        Self::retryable(error)
    }
}

impl From<sync_common::transport::WireError> for TransferError {
    fn from(error: sync_common::transport::WireError) -> Self {
        Self::retryable(error)
    }
}

impl From<tokio::sync::AcquireError> for TransferError {
    fn from(error: tokio::sync::AcquireError) -> Self {
        Self::retryable(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct TransferJob {
    operation: TransferOperation,
    origin: TransferOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TransferOperation {
    UploadBlob { content_hash: ContentHash, placement: Option<BookPlacement> },
    UploadThumbnail { thumbnail_hash: ContentHash, book_content_hash: Option<ContentHash> },
    DownloadBlob { content_hash: ContentHash, placement: BookPlacement },
    DownloadThumbnail { thumbnail_hash: ContentHash, book_content_hash: ContentHash },
}

impl TransferJob {
    pub(super) fn upload_blob(content_hash: ContentHash, placement: Option<BookPlacement>, origin: TransferOrigin) -> Self {
        Self { operation: TransferOperation::UploadBlob { content_hash, placement }, origin }
    }

    pub(super) fn upload_thumbnail(thumbnail_hash: ContentHash, book_content_hash: Option<ContentHash>, origin: TransferOrigin) -> Self {
        Self { operation: TransferOperation::UploadThumbnail { thumbnail_hash, book_content_hash }, origin }
    }

    pub(super) fn download_blob(content_hash: ContentHash, placement: BookPlacement, origin: TransferOrigin) -> Self {
        Self { operation: TransferOperation::DownloadBlob { content_hash, placement }, origin }
    }

    pub(super) fn download_thumbnail(thumbnail_hash: ContentHash, book_content_hash: ContentHash, origin: TransferOrigin) -> Self {
        Self { operation: TransferOperation::DownloadThumbnail { thumbnail_hash, book_content_hash }, origin }
    }

    fn batch_kind(&self) -> Option<TransferJobKind> {
        match &self.operation {
            TransferOperation::UploadThumbnail { .. } => Some(TransferJobKind::UploadThumbnail),
            TransferOperation::DownloadThumbnail { .. } => Some(TransferJobKind::DownloadThumbnail),
            _ => None,
        }
    }

    const fn origin(&self) -> TransferOrigin {
        self.origin
    }

    fn status_fields(&self) -> (TransferJobKind, &ContentHash, Option<ContentHash>, Option<String>) {
        match &self.operation {
            TransferOperation::UploadBlob { content_hash, placement } => (TransferJobKind::UploadBook, content_hash, placement.as_ref().map(|value| value.content_hash), placement.as_ref().map(|value| value.rel_path.as_str().to_owned())),
            TransferOperation::UploadThumbnail { thumbnail_hash, book_content_hash } => (TransferJobKind::UploadThumbnail, thumbnail_hash, *book_content_hash, None),
            TransferOperation::DownloadBlob { content_hash, placement } => (TransferJobKind::DownloadBook, content_hash, Some(placement.content_hash), Some(placement.rel_path.as_str().to_owned())),
            TransferOperation::DownloadThumbnail { thumbnail_hash, book_content_hash } => (TransferJobKind::DownloadThumbnail, thumbnail_hash, Some(*book_content_hash), None),
        }
    }

    fn priority(&self) -> (u8, u8, u64) {
        let origin = if self.origin() == TransferOrigin::UserInitiated { 0 } else { 1 };
        match &self.operation {
            TransferOperation::DownloadThumbnail { .. } => (0, origin, 0),
            TransferOperation::UploadThumbnail { .. } => (0, origin, 0),
            TransferOperation::UploadBlob { .. } => (1, origin, 0),
            TransferOperation::DownloadBlob { .. } => (1, origin, u64::MAX / 2),
        }
    }
}

#[derive(Clone)]
pub(super) struct TransferWorker {
    http_client: Client,
    library_id: LibraryId,
    db_pool: DbPool,
    event_tx: Option<crate::platform::library_events::LibraryEventSender>,
    budget: Arc<Semaphore>,
    session: Arc<std::sync::RwLock<Option<sync_client::Session>>>,
    runtime: TransferRuntime,
    queue: Arc<TransferQueue>,
}

pub(super) struct TransferWorkerConfig {
    pub(super) http_client: Client,
    pub(super) library_id: LibraryId,
    pub(super) db_pool: DbPool,
    pub(super) event_tx: Option<crate::platform::library_events::LibraryEventSender>,
    pub(super) session: Arc<std::sync::RwLock<Option<sync_client::Session>>>,
    pub(super) runtime: TransferRuntime,
    pub(super) queue: Arc<TransferQueue>,
}

impl TransferWorker {
    pub(super) fn new(config: TransferWorkerConfig) -> Self {
        let byte_budget_mb = std::env::var("BOKHEIM_TRANSFER_BUDGET_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(64);
        Self {
            http_client: config.http_client,
            library_id: config.library_id,
            db_pool: config.db_pool,
            event_tx: config.event_tx,
            budget: Arc::new(Semaphore::new(byte_budget_mb.max(1))),
            session: config.session,
            runtime: config.runtime,
            queue: config.queue,
        }
    }

    pub(super) async fn run(self, jobs: Vec<TransferJob>, worker_count: usize) -> TransferRunResult {
        self.queue.reconcile(jobs);
        self.send_transfer_status_changed();
        self.run_pending(worker_count).await
    }

    pub(super) async fn run_pending(self, worker_count: usize) -> TransferRunResult {
        self.queue.resume_authentication();
        if !self.queue.try_start_scheduler() {
            return Ok(());
        }
        let worker = Arc::new(self);
        let (first_run_tx, first_run_rx) = oneshot::channel();
        crate::app::executor::spawn_detached(scheduler_loop(worker, worker_count.max(1), first_run_tx));
        first_run_rx.await.unwrap_or_else(|_| Err(TransferError::retryable("transfer scheduler stopped before its first run")))
    }

    async fn drain_due(self: &Arc<Self>, worker_count: usize) -> TransferRunResult {
        let runs = futures_util::future::join_all((0..worker_count).map(|_| {
            let worker = Arc::clone(self);
            async move { worker.run_queue().await }
        }))
        .await;
        if runs.into_iter().any(|result| matches!(result, Err(TransferError::AuthenticationRequired))) {
            Err(TransferError::AuthenticationRequired)
        } else {
            Ok(())
        }
    }

    async fn run_queue(self: &Arc<Self>) -> TransferRunResult {
        loop {
            let claimed = self.queue.claim_due_batch(self.background_allowed());
            if claimed.is_empty() {
                break;
            }
            self.send_transfer_status_changed();
            let outcome = self.handle_claimed_batch(&claimed).await;
            match &outcome {
                Ok(()) => {
                    for item in &claimed {
                        self.queue.complete(item.id, None);
                    }
                }
                Err(TransferError::Retryable(message)) => {
                    log::warn!("Retryable transfer error: {message}");
                    for item in &claimed {
                        self.queue.retry(item.id, message.clone());
                    }
                }
                Err(TransferError::Rejected(message)) => {
                    log::warn!("Rejected transfer: {message}");
                    for item in &claimed {
                        self.queue.complete(item.id, Some(message.clone()));
                    }
                }
                Err(TransferError::AuthenticationRequired) => {
                    self.queue.release(&claimed);
                    self.send_transfer_status_changed();
                    return Err(TransferError::AuthenticationRequired);
                }
            }
            self.send_transfer_status_changed();
        }
        Ok(())
    }

    fn send_transfer_status_changed(&self) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(LibraryEvent::TransferStatusChanged);
        }
    }

    fn background_allowed(&self) -> bool {
        self.runtime.network_status().connected
    }
}

async fn scheduler_loop(worker: Arc<TransferWorker>, worker_count: usize, first_run_tx: oneshot::Sender<TransferRunResult>) {
    let first_result = worker.drain_due(worker_count).await;
    if first_result.is_err() {
        worker.queue.pause_authentication();
    }
    let _ = first_run_tx.send(first_result.clone());
    if first_result.is_err() {
        return;
    }

    loop {
        let allow_background = worker.background_allowed();
        let Some(delay) = worker.queue.next_due_in(allow_background) else {
            if worker.queue.keep_scheduler_or_stop(allow_background) {
                continue;
            }
            return;
        };
        worker.queue.wait_for_wake(delay).await;
        if let Err(TransferError::AuthenticationRequired) = worker.drain_due(worker_count).await {
            worker.queue.pause_authentication();
            return;
        }
    }
}

#[cfg(test)]
mod tests;

//! Session-owned transfer execution behind a snapshot-in/outcome-out boundary.
//!
//! The worker carries everything a drain needs except storage: queue,
//! credentials, account, assets, and events. Durable effects never cross as
//! connections or callbacks. Each driver opens its own commit connection to
//! the library file from the stored path, snapshots the due hashes, runs one
//! claimed batch, and commits its outcome; SQLite serializes the writers and
//! every replayed write is guarded and idempotent.
//!
//! The worker is `'static`, so planning pages can detach a driver and keep
//! metadata exchange moving while slow asset HTTP is still in flight.

use std::sync::atomic::Ordering;
use super::TransferRunResult;
use crate::runner::{DroppedUpload, QueueSettlement, TransferDrain, TransferHost, MAX_CONCURRENT_BOOK_UPLOADS};
use crate::{ClaimedTransferJob, TransferJob, TransferQueue};
use crate::{TransferError, TransferOperation};
use futures_util::future::LocalBoxFuture;
use library_database::{Database, TransferOutcome, TransferSnapshot, TransferWrite};
use library_files::asset_store::AssetStore;
use library_model::BlobKind;
use library_model::{BookUploadIntent, TransferJobKind};
use library_runtime::account::{LibraryAccount, SharedSyncCredentials};
use library_runtime::events::{LibraryEvent, LibraryEventSender};
use library_runtime::interest::NotificationInterest;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use sync_common::{ContentHash, LibraryId};

#[derive(Clone)]
pub struct TransferWorker {
    pub asset_storage_enabled: Arc<AtomicBool>,
    pub assets: AssetStore,
    pub http_client: reqwest::Client,
    pub library_id: LibraryId,
    pub event_tx: Option<LibraryEventSender>,
    pub queue: Arc<TransferQueue>,
    pub credentials: SharedSyncCredentials,
    pub account: Option<LibraryAccount>,
    pub notification_interest: NotificationInterest,
    pub availability_changed: Arc<AtomicBool>,
    pub db_path: std::path::PathBuf,
    pub cpu: std::sync::Arc<dyn cpu_host::CpuHost>,
    #[cfg(target_arch = "wasm32")]
    pub progress_key: Option<String>,
}

impl TransferWorker {
    /// One commit connection per driver: sequential use on one task, so the
    /// non-`Sync` handle never crosses threads while held. The library
    /// session initialized the schema before any worker existed.
    pub(super) fn commit_connection(&self) -> Result<Database, TransferError> {
        Ok(Database::open(&self.db_path)?)
    }

    pub(crate) fn asset_storage_enabled(&self) -> bool {
        self.asset_storage_enabled.load(Ordering::Acquire)
    }

    pub(super) fn live_user_id(&self) -> Option<String> {
        self.account.as_ref()?.current().ok()??.user_id().to_owned().into()
    }

    /// Snapshot, drain, and commit one claimed batch.
    /// The outcome commits even when the drain reports an error; records are
    /// idempotent replays of the guarded commits.
    async fn drain_pass(&self) -> TransferRunResult {
        let database = self.commit_connection()?;
        let due_hashes = {
            let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
            let state = self.queue.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut hashes = Vec::new();
            for queued in state.batches.values().flat_map(|batch| batch.jobs.values()).filter(|queued| queued.state == library_runtime::transfer::QueueJobState::Pending && queued.next_attempt_at <= now) {
                let hash = *queued.job.status_fields().1;
                if !hashes.contains(&hash) {
                    hashes.push(hash);
                }
            }
            hashes
        };
        let snapshot = database.transfer_snapshot(&due_hashes)?;
        let mut outcome = TransferOutcome::default();
        let drain = {
            ({
                let snapshot: &TransferSnapshot = &snapshot;
                let out: &mut TransferOutcome = &mut outcome;
                let limit: Option<usize> = Some(1);
                async move {
                    let mut processed = 0;
                    let mut drain = TransferDrain { result: Ok(()), claimed: Vec::new(), settlements: Vec::new() };
                    loop {
                        if limit.is_some_and(|limit| processed >= limit) {
                            break;
                        }
                        let mut claimed = self.queue.claim_due_batch();
                        let claimed_at = web_time::Instant::now();
                        if claimed.is_empty() {
                            break;
                        }
                        drain.claimed.extend(claimed.iter().cloned());
                        processed += 1;
                        self.changed();
                        if claimed[0].job.batch_kind() == Some(TransferJobKind::UploadBook) {
                            let mut intents = Vec::new();
                            for item in &claimed {
                                let hash = *item.job.status_fields().1;
                                if let Some(intent) = snapshot.asset_for(&hash).and_then(|asset| asset.upload.intent.as_ref()) {
                                    if !intents.iter().any(|intent: &BookUploadIntent| intent.content_hash == hash) {
                                        intents.push(intent.clone());
                                    }
                                }
                            }
                            let mut trace = sync_transport::PerformanceTrace::new("prepare_upload_batch", "negotiate_and_plan");
                            log::debug!(target: "sync_performance", "trace_id={} books={}", trace.id(), intents.len());
                            let result = self.prepare_uploads(&intents, out).await;
                            trace.finish(result.is_ok());
                            match result {
                                Ok(prepared) => {
                                    claimed.retain_mut(|item| {
                                        let hash = *item.job.status_fields().1;
                                        if let Some(job) = prepared.jobs.iter().find(|job| *job.status_fields().1 == hash) {
                                            item.job = job.clone();
                                            true
                                        } else {
                                            match prepared.dropped.iter().find(|dropped| {
                                                let dropped_hash = match dropped {
                                                    DroppedUpload::Completed { hash } | DroppedUpload::Rejected { hash, .. } | DroppedUpload::Changed { hash } => hash,
                                                };
                                                dropped_hash == &hash
                                            }) {
                                                Some(DroppedUpload::Completed { .. }) => drain.settlements.push(QueueSettlement::Complete(item.id, None)),
                                                Some(DroppedUpload::Rejected { reason, .. }) => drain.settlements.push(QueueSettlement::Complete(item.id, Some(reason.clone()))),
                                                _ => {
                                                    drain.settlements.push(QueueSettlement::Retry(item.id, "upload intent changed during transfer".to_owned()));
                                                }
                                            }
                                            false
                                        }
                                    });
                                    self.changed();
                                }
                                Err(TransferError::AuthenticationRequired) => {
                                    drain.settlements.extend(claimed.iter().map(|item| QueueSettlement::Release(item.id)));
                                    self.changed();
                                    drain.result = Err(TransferError::AuthenticationRequired);
                                    return drain;
                                }
                                Err(error) => {
                                    for item in &claimed {
                                        drain.settlements.push(QueueSettlement::Retry(item.id, error.to_string()));
                                    }
                                    self.changed();
                                    continue;
                                }
                            }
                        }
                        // Thumbnail requests are bounded by count and payload size. A
                        // rejected upload batch does not identify the offending item;
                        // retry those uploads individually before suppressing any of them.
                        let is_thumbnail_batch = matches!(claimed.first().and_then(|item| item.job.batch_kind()), Some(TransferJobKind::DownloadThumbnail | TransferJobKind::UploadThumbnail));
                        if is_thumbnail_batch {
                            let outcome = { self.execute(snapshot, &claimed, out).await };
                            match outcome {
                                Ok(()) => {
                                    for item in &claimed {
                                        drain.settlements.push(QueueSettlement::Complete(item.id, None));
                                    }
                                }
                                Err(TransferError::Retryable(message)) => {
                                    log::warn!("Retryable thumbnail batch error: {message}");
                                    for item in &claimed {
                                        drain.settlements.push(QueueSettlement::Retry(item.id, message.to_string()));
                                    }
                                }
                                Err(TransferError::Rejected(message)) => {
                                    log::warn!("Rejected thumbnail batch: {message}");
                                    let isolate_uploads = claimed.len() > 1 && claimed[0].job.batch_kind() == Some(TransferJobKind::UploadThumbnail);
                                    for (index, item) in claimed.iter().enumerate() {
                                        let result = if isolate_uploads { self.execute(snapshot, std::slice::from_ref(item), out).await } else { Err(TransferError::Rejected(message.clone())) };
                                        match result {
                                            Ok(()) => drain.settlements.push(QueueSettlement::Complete(item.id, None)),
                                            Err(TransferError::Rejected(reason)) => {
                                                self.settle_rejection(&item.job, &reason.to_string(), out);
                                                drain.settlements.push(QueueSettlement::Complete(item.id, Some(reason.to_string())));
                                            }
                                            Err(TransferError::Retryable(reason)) => drain.settlements.push(QueueSettlement::Retry(item.id, reason.to_string())),
                                            Err(TransferError::AuthenticationRequired) => {
                                                drain.settlements.extend(claimed[index..].iter().map(|item| QueueSettlement::Release(item.id)));
                                                self.changed();
                                                self.flush();
                                                drain.result = Err(TransferError::AuthenticationRequired);
                                                return drain;
                                            }
                                        }
                                    }
                                }
                                Err(TransferError::AuthenticationRequired) => {
                                    drain.settlements.extend(claimed.iter().map(|item| QueueSettlement::Release(item.id)));
                                    self.changed();
                                    self.flush();
                                    drain.result = Err(TransferError::AuthenticationRequired);
                                    return drain;
                                }
                            }
                        } else {
                            let mut batch_results = std::collections::VecDeque::new();
                            for (index, item) in claimed.iter().enumerate() {
                                let mut trace = sync_transport::PerformanceTrace::new("transfer_job", "execute");
                                log::debug!(target: "sync_performance", "trace_id={} job_id={} kind={:?} since_batch_claim_ms={:.3}", trace.id(), item.id, item.job.batch_kind(), claimed_at.elapsed().as_secs_f64() * 1000.0);
                                if batch_results.is_empty() {
                                    // Browser uploads retain their per-file ownership path.
                                    // Native PUTs run concurrently for books of every size.
                                    let count = if cfg!(target_arch = "wasm32") {
                                        1
                                    } else {
                                        claimed[index..].iter().take(MAX_CONCURRENT_BOOK_UPLOADS)
                                            .take_while(|item| matches!(&item.job.operation, TransferOperation::UploadBlob { intent: Some(_), negotiated: true, .. }))
                                            .count().max(1)
                                    };
                                    if count > 1 {
                                        let group = &claimed[index..index + count];
                                        batch_results = match self.execute_uploads(snapshot, group, out).await {
                                            Ok(results) if results.len() == count => results.into(),
                                            Ok(_) => (0..count).map(|_| Err(TransferError::retryable("incomplete individual upload results"))).collect(),
                                            Err(TransferError::AuthenticationRequired) => (0..count).map(|_| Err(TransferError::AuthenticationRequired)).collect(),
                                            Err(error) => (0..count).map(|_| Err(TransferError::retryable(error.clone()))).collect(),
                                        };
                                    } else {
                                        batch_results.push_back(self.execute(snapshot, std::slice::from_ref(item), out).await);
                                    }
                                }
                                let outcome = batch_results.pop_front().expect("one result per claimed job");
                                trace.finish(outcome.is_ok());
                                match outcome {
                                    Ok(()) => drain.settlements.push(QueueSettlement::Complete(item.id, None)),
                                    Err(TransferError::Retryable(message)) => {
                                        log::warn!("Retryable transfer error for job {}: {message}", item.id);
                                        drain.settlements.push(QueueSettlement::Retry(item.id, message.to_string()));
                                    }
                                    Err(TransferError::Rejected(message)) => {
                                        log::warn!("Rejected transfer for job {}: {message}", item.id);
                                        self.settle_rejection(&item.job, &message.to_string(), out);
                                        drain.settlements.push(QueueSettlement::Complete(item.id, Some(message.to_string())));
                                    }
                                    Err(TransferError::AuthenticationRequired) => {
                                        // Release the current and all not-yet-processed
                                        // jobs. Authentication is global; file failures are
                                        // deliberately not.
                                        drain.settlements.extend(claimed[index..].iter().map(|item| QueueSettlement::Release(item.id)));
                                        self.changed();
                                        self.flush();
                                        drain.result = Err(TransferError::AuthenticationRequired);
                                        return drain;
                                    }
                                }
                                self.changed();
                            }
                        }
                        self.changed();
                    }
                    self.flush();
                    drain
                }
            })
            .await
        };
        // A stale member's next planning pass simply re-queues it; this batch
        // path only needs the commit to have happened, not stayed current.
        match database.commit_transfer_outcome(&outcome) {
            Ok(_) => {
                let result = drain.committed(&self.queue);
                self.send_transfer_status_changed();
                let published = self.publish_committed_downloads(&database, &outcome);
                self.flush_availability_changes();
                result.and(published)
            }
            Err(error) => {
                let error = TransferError::from(error);
                ({
                    use crate::runner::*;
                    let drain: TransferDrain = drain;
                    let queue: &TransferQueue = &self.queue;
                    let error: &TransferError = &error;

                    for item in &drain.claimed {
                        queue.retry(item.id, error.to_string());
                    }
                });
                self.send_transfer_status_changed();
                Err(error)
            }
        }
    }

    /// Final availability is announced only after its SQLite outcome commits.
    /// Progress is transient, but completion must agree with a new subscriber's
    /// durable snapshot and with refreshed book rows.
    fn publish_committed_downloads(&self, database: &Database, outcome: &TransferOutcome) -> Result<(), TransferError> {
        let mut hashes = std::collections::HashSet::<ContentHash>::new();
        for write in &outcome.writes {
            match write {
                TransferWrite::CompleteDownload { hash } | TransferWrite::CancelDownload { hash } | TransferWrite::CompleteRetainedDownload { hash, .. } | TransferWrite::CompleteLocalBookRequest { hash } => {
                    hashes.insert(*hash);
                }
                TransferWrite::CompleteDownloadedBook { completion, .. } => {
                    hashes.insert(completion.snapshot.content_hash);
                }
                _ => {}
            }
        }
        for hash in hashes {
            let present = self.assets.book_exists(&hash, &database.book_paths(&hash)?)?;
            let state = database.transfer_download_state(&hash, present)?;
            self.send_download_status(hash, state);
        }
        Ok(())
    }

    /// Drive due batches until the queue idles, holding the scheduler while
    /// work remains. Authentication pauses the queue; anything else is
    /// already settled per job, so the driver keeps going.
    pub async fn drive(&self) {
        self.send_transfer_status_changed();
        if !self.queue.try_start_scheduler() {
            return;
        }
        loop {
            if let Err(TransferError::AuthenticationRequired) = self.drain_pass().await {
                self.queue.pause_authentication();
                return;
            }
            let Some(delay) = self.queue.next_due_in() else {
                if self.queue.keep_scheduler_or_stop() {
                    continue;
                }
                return;
            };
            self.queue.wait_for_wake(delay).await;
        }
    }

    pub fn ensure_driver(worker: Self) {
        if !worker.queue.try_start_scheduler() {
            return;
        }
        // The host works in local futures, so the driver is not `Send` and
        // cannot move onto the shared runtime. It gets its own thread with a
        // current-thread runtime instead; the queue rendezvous across it.
        #[cfg(target_arch = "wasm32")]
        client_platform_runtime::executor::spawn_detached(async move { worker.drive_detached().await });
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("transfer driver runtime");
            runtime.block_on(worker.drive_detached());
        });
    }

    /// Scheduler already acquired by [`Self::ensure_driver`]; same loop as
    /// [`Self::drive`] without re-acquiring it.
    async fn drive_detached(&self) {
        loop {
            if let Err(TransferError::AuthenticationRequired) = self.drain_pass().await {
                self.queue.pause_authentication();
                return;
            }
            let Some(delay) = self.queue.next_due_in() else {
                if self.queue.keep_scheduler_or_stop() {
                    continue;
                }
                return;
            };
            self.queue.wait_for_wake(delay).await;
        }
    }

    async fn handle_batch_with_refresh(&self, snapshot: &TransferSnapshot, claimed: &[ClaimedTransferJob], out: &mut TransferOutcome) -> TransferRunResult {
        let guard = self.refresh_guard(super::RefreshScope::Queued)?;
        let result = self.handle_claimed_batch(snapshot, claimed, out).await;
        if !matches!(result, Err(TransferError::AuthenticationRequired)) {
            return result;
        }
        self.refresh_after_auth(guard).await?;
        self.handle_claimed_batch(snapshot, claimed, out).await
    }

    /// Execute one explicitly requested job outside the queue, committing its
    /// outcome before returning.
    pub(super) async fn execute_owned_job(&self, job: TransferJob) -> Result<(), TransferError> {
        let hash = *job.status_fields().1;
        let download = matches!(&job.operation, TransferOperation::DownloadBlob { .. });
        let database = self.commit_connection()?;
        let snapshot = database.transfer_snapshot(&[hash])?;
        let mut outcome = TransferOutcome::default();
        let result = self.execute_job(&snapshot, job, &mut outcome).await;
        let commit = database.commit_transfer_outcome(&outcome).map_err(TransferError::from).and_then(|current| {
            if current {
                self.publish_committed_downloads(&database, &outcome)
            } else {
                Err(TransferError::retryable("local bytes changed before the job could be confirmed"))
            }
        });
        let result = result.and(commit);
        if download && result.is_err() {
            if let Some(present) = database.book_paths(&hash).ok().and_then(|paths| self.assets.book_exists(&hash, &paths).ok()) {
                if let Ok(state) = database.transfer_download_state(&hash, present) {
                    self.send_download_status(hash, state);
                }
            }
        }
        self.flush_availability_changes();
        result
    }

    /// Announces, once per drained queue, that the set of locally available
    /// books changed. Individual books already reported themselves.
    pub(crate) fn flush_availability_changes(&self) {
        if !self.availability_changed.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(LibraryEvent::ContentsChanged);
        }
    }

    fn send_transfer_status_changed(&self) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(LibraryEvent::TransferStatusChanged);
        }
    }
}

impl TransferHost for TransferWorker {
    fn prepare_uploads<'a>(&'a self, intents: &'a [BookUploadIntent], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, Result<crate::runner::PreparedUploads, TransferError>> {
        Box::pin({
            use crate::transfer::executor::*;

            async move {
                {
                    if !self.asset_storage_enabled() {
                        return Ok(PreparedUploads { jobs: Vec::new(), dropped: Vec::new() });
                    }
                    let guard = self.refresh_guard(crate::transfer::RefreshScope::Planned)?;
                    let result = self.prepare_book_uploads(intents, out).await;
                    if !matches!(result, Err(TransferError::AuthenticationRequired)) {
                        return result;
                    }
                    self.refresh_after_auth(guard).await?;
                    self.prepare_book_uploads(intents, out).await
                }
            }
        })
    }
    fn execute<'a>(&'a self, snapshot: &'a TransferSnapshot, claimed: &'a [ClaimedTransferJob], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, TransferRunResult> {
        Box::pin(async move {
            let _interest = self.notification_interest.work();
            self.handle_batch_with_refresh(snapshot, claimed, out).await
        })
    }
    fn execute_uploads<'a>(&'a self, snapshot: &'a TransferSnapshot, claimed: &'a [ClaimedTransferJob], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, Result<Vec<TransferRunResult>, TransferError>> {
        Box::pin(self.upload_claimed_books_with_refresh(snapshot, claimed, out))
    }
    fn settle_rejection(&self, job: &TransferJob, reason: &str, out: &mut TransferOutcome) {
        ({
            use crate::transfer::executor::*;

            match &job.operation {
                TransferOperation::DownloadBlob { content_hash, .. } => {
                    out.record(TransferWrite::CompleteDownload { hash: *content_hash });
                }
                TransferOperation::UploadBlob { content_hash, intent, .. } => {
                    out.record(TransferWrite::SettleRejectedUpload { hash: *content_hash, intent: intent.clone(), reason: reason.to_owned() });
                }
                TransferOperation::UploadThumbnail { thumbnail_hash } => {
                    out.record(TransferWrite::SettleRejectedThumbnail { hash: *thumbnail_hash, reason: reason.to_owned() });
                }
                TransferOperation::DownloadThumbnail { .. } => {}
            }
        });
    }
    fn changed(&self) {
        self.send_transfer_status_changed();
    }
    fn flush(&self) {
        // Availability is published by the commit owner, after durable state.
    }
}

mod coordination;
use library_files::asset_store::AssetStore;
use library_runtime::database::Database;
use library_runtime::events::{LibraryEvent, LibraryEventSender};
use library_runtime::interest::NotificationInterest;
use reqwest::Client;
use std::sync::Arc;

#[cfg(test)]
use crate::{BookPlacement, ContentHash};
use sync_common::LibraryId;
#[cfg(test)]
use sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES;

type TransferRunResult = Result<(), TransferError>;

pub use asset_transfer::{LibraryTransfers, TransferOrigin, TransferState, TransferStatus};
pub(crate) use asset_transfer::{TransferJob, TransferOperation, TransferQueue};

mod authentication;
mod executor;
use authentication::RefreshScope;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web_upload;

pub(crate) use library_runtime::transfer_history::queue_with_history;

pub(super) use asset_transfer::TransferError;

impl From<library_runtime::database::DatabaseError> for TransferError {
    fn from(error: library_runtime::database::DatabaseError) -> Self {
        Self::retryable(error)
    }
}

#[derive(Clone)]
pub(super) struct TransferWorker {
    pub(super) asset_storage_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub(super) notification_interest: NotificationInterest,
    pub(super) http_client: Client,
    pub(super) library_id: LibraryId,
    pub(super) db_pool: Database,
    pub(super) assets: AssetStore,
    pub(super) event_tx: Option<LibraryEventSender>,
    pub(super) credentials: library_runtime::account::SharedSyncCredentials,
    pub(super) account: Option<library_runtime::account::LibraryAccount>,
    pub(super) queue: Arc<TransferQueue>,
    /// Set when a transfer changed whether some book is available locally.
    /// Shared across clones so the run that observes the change is the one that
    /// announces it, once.
    pub(super) availability_changed: Arc<std::sync::atomic::AtomicBool>,
}

impl TransferWorker {
    pub(in crate::sync) fn asset_storage_enabled(&self) -> bool {
        self.asset_storage_enabled.load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(test)]
    pub(super) async fn run(self, jobs: Vec<TransferJob>) -> TransferRunResult {
        self.queue.reconcile(jobs);
        self.queue.resume_authentication();
        if !self.queue.try_start_scheduler() {
            return Ok(());
        }
        let result = self.runner().drain_ready().await;
        if result.is_err() {
            self.queue.pause_authentication();
        } else {
            library_runtime::executor::spawn_detached(scheduler_loop(self));
        }
        result
    }

    pub(super) fn run_page(self, generation: u64, jobs: Vec<TransferJob>) {
        self.queue.reconcile_page(generation, jobs);
        self.start();
    }

    pub(super) fn finish_plan(self, generation: u64) {
        self.queue.finish_reconcile(generation);
        self.start();
    }

    fn start(self) {
        self.send_transfer_status_changed();
        if self.queue.try_start_scheduler() {
            library_runtime::executor::spawn_detached(scheduler_loop(self));
        }
    }

    async fn handle_batch_with_refresh(&self, claimed: &[asset_transfer::ClaimedTransferJob]) -> TransferRunResult {
        self.with_refresh(RefreshScope::Queued, || self.handle_claimed_batch(claimed)).await
    }

    fn runner(&self) -> asset_transfer::runner::TransferRunner<'_, Self> {
        asset_transfer::runner::TransferRunner { queue: &self.queue, host: self }
    }

    /// Announces, once per drained queue, that the set of locally available
    /// books changed. Individual books already reported themselves.
    pub(super) fn flush_availability_changes(&self) {
        if !self.availability_changed.swap(false, std::sync::atomic::Ordering::AcqRel) {
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

async fn scheduler_loop(worker: TransferWorker) {
    worker.runner().run().await;
}

impl asset_transfer::runner::TransferHost for TransferWorker {
    fn prepare_uploads<'a>(&'a self, hashes: &'a [sync_common::ContentHash]) -> library_runtime::executor::BoxedBackendFuture<'a, Result<Vec<TransferJob>, TransferError>> {
        Box::pin(self.prepare_book_uploads_with_refresh(hashes))
    }
    fn execute<'a>(&'a self, claimed: &'a [asset_transfer::ClaimedTransferJob]) -> library_runtime::executor::BoxedBackendFuture<'a, TransferRunResult> {
        Box::pin(async move {
            let _interest = self.notification_interest.work();
            self.handle_batch_with_refresh(claimed).await
        })
    }
    fn execute_uploads<'a>(&'a self, claimed: &'a [asset_transfer::ClaimedTransferJob]) -> library_runtime::executor::BoxedBackendFuture<'a, Result<Vec<TransferRunResult>, TransferError>> {
        Box::pin(self.upload_claimed_books_with_refresh(claimed))
    }
    fn rejection_reason(&self, hash: &sync_common::ContentHash) -> Result<Option<String>, TransferError> {
        Ok(client_library::assets::book_upload_rejection_reason(&*self.db_pool.get()?, hash)?)
    }
    fn settle_rejection(&self, job: &TransferJob, reason: &str) -> TransferRunResult {
        self.resolve_rejected_job(job, reason)
    }
    fn changed(&self) {
        self.send_transfer_status_changed();
    }
    fn flush(&self) {
        self.flush_availability_changes();
    }
}

#[cfg(test)]
mod tests;

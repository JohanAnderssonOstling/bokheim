//! Transfer queue execution policy, independent of application authentication and events.
pub(crate) use crate::{ClaimedTransferJob, TransferError, TransferJob, TransferQueue};
pub(crate) use futures_util::future::LocalBoxFuture;
pub(crate) use library_database::{TransferOutcome, TransferSnapshot};
pub(crate) use library_model::BookUploadIntent;
pub(crate) use library_model::TransferJobKind;
pub(crate) use sync_common::ContentHash;

pub(crate) type TransferRunResult = Result<(), TransferError>;

pub use crate::concurrent_uploads::MAX_CONCURRENT_BOOK_UPLOADS;

/// Host operations preserve application credential refresh, browser coordination,
/// and notifications without coupling the runner to backend. The host works
/// only on snapshots and records durable effects into the outcome the
/// orchestrator commits; no storage crosses this boundary in either direction.
/// Futures stay local to the driving task, so the host is not `Sync`.
pub trait TransferHost {
    fn prepare_uploads<'a>(&'a self, intents: &'a [BookUploadIntent], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, Result<PreparedUploads, TransferError>>;
    fn execute<'a>(&'a self, snapshot: &'a TransferSnapshot, claimed: &'a [ClaimedTransferJob], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, TransferRunResult>;
    fn execute_uploads<'a>(&'a self, snapshot: &'a TransferSnapshot, claimed: &'a [ClaimedTransferJob], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, Result<Vec<TransferRunResult>, TransferError>> {
        self.execute_uploads_sequentially(snapshot, claimed, out)
    }
    fn execute_uploads_sequentially<'a>(&'a self, snapshot: &'a TransferSnapshot, claimed: &'a [ClaimedTransferJob], out: &'a mut TransferOutcome) -> LocalBoxFuture<'a, Result<Vec<TransferRunResult>, TransferError>> {
        Box::pin(async move {
            let mut results = Vec::new();
            for item in claimed {
                let result = self.execute(snapshot, std::slice::from_ref(item), out).await;
                if matches!(result, Err(TransferError::AuthenticationRequired)) {
                    return Err(TransferError::AuthenticationRequired);
                }
                results.push(result);
            }
            Ok(results)
        })
    }
    fn settle_rejection(&self, job: &TransferJob, reason: &str, out: &mut TransferOutcome);
    fn changed(&self);
    fn flush(&self);
}

/// One negotiated upload batch: jobs to run plus per-intent dispositions for
/// claimed hashes the negotiation dropped. Dropped intents never reach the
/// queue as jobs; the runner completes or retries them from this data.
pub struct PreparedUploads {
    pub jobs: Vec<TransferJob>,
    pub dropped: Vec<DroppedUpload>,
}

/// Why a claimed hash produced no job.
pub enum DroppedUpload {
    /// The server already has the bytes or the rejection was recorded; the
    /// settle record carries the durable effect.
    Completed { hash: ContentHash },
    /// The server rejected the intent with a reason to complete with.
    Rejected { hash: ContentHash, reason: String },
    /// The intent changed or vanished; retry it on a later pass.
    Changed { hash: ContentHash },
}

pub(crate) enum QueueSettlement {
    Complete(u64, Option<String>),
    Retry(u64, String),
    Release(u64),
}

/// Queue results for one database replay. Claimed jobs stay running until the
/// owner confirms that every recorded database write has been replayed.
pub struct TransferDrain {
    pub(crate) result: TransferRunResult,
    pub(crate) claimed: Vec<ClaimedTransferJob>,
    pub(crate) settlements: Vec<QueueSettlement>,
}

impl TransferDrain {
    pub fn committed(self, queue: &TransferQueue) -> TransferRunResult {
        for settlement in self.settlements {
            match settlement {
                QueueSettlement::Complete(id, error) => queue.complete(id, error),
                QueueSettlement::Retry(id, error) => {
                    queue.retry(id, error);
                }
                QueueSettlement::Release(id) => {
                    if let Some(item) = self.claimed.iter().find(|item| item.id == id) {
                        queue.release(std::slice::from_ref(item));
                    }
                }
            }
        }
        self.result
    }
}

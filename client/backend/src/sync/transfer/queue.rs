use super::TransferJob;
use crate::sync::DbPool;
use crate::TransferJobKind;
use library_replica::{TransferOrigin, TransferState, TransferStatus};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES;
use tokio::sync::Notify;
use tokio::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueJobState {
    Pending,
    Running,
}

#[derive(Clone, Debug)]
struct QueueJob {
    id: u64,
    job: TransferJob,
    state: QueueJobState,
    attempts: u32,
    next_attempt_at: u64,
    last_error: Option<String>,
    created_at: u64,
    updated_at: u64,
}

#[derive(Clone, Debug)]
pub(super) struct ClaimedTransferJob {
    pub(super) id: u64,
    pub(super) job: TransferJob,
}

#[derive(Debug)]
struct TransferBatch {
    jobs: HashMap<u64, QueueJob>,
    completed: u32,
    attempts: u32,
    created_at: u64,
    updated_at: u64,
    origin: TransferOrigin,
    permanent_error: Option<String>,
}

impl TransferBatch {
    fn new(now: u64, origin: TransferOrigin) -> Self {
        Self { jobs: HashMap::new(), completed: 0, attempts: 0, created_at: now, updated_at: now, origin, permanent_error: None }
    }

    fn status(&self, kind: TransferJobKind) -> Option<TransferStatus> {
        let mut jobs = self.jobs.values().collect::<Vec<_>>();
        jobs.sort_by_key(|job| (job.created_at, job.id));
        let first = *jobs.first()?;
        let (_, content_hash, _, _) = first.job.status_fields();
        let state = if jobs.iter().any(|job| job.state == QueueJobState::Running) {
            TransferState::Running
        } else if jobs.iter().any(|job| job.last_error.is_some()) {
            TransferState::Retrying
        } else {
            TransferState::Queued
        };
        Some(TransferStatus {
            id: format!("batch-{}", kind.storage()),
            kind,
            content_hash: *content_hash,
            book_content_hash: None,
            path: None,
            state,
            origin: self.origin,
            attempts: self.attempts.max(jobs.iter().map(|job| job.attempts).max().unwrap_or(0)),
            completed_items: self.completed,
            total_items: self.completed.saturating_add(jobs.len() as u32),
            last_error: self.permanent_error.clone().or_else(|| jobs.iter().find_map(|job| job.last_error.clone())),
            next_retry_at: jobs.iter().filter_map(|job| (job.next_attempt_at > 0).then_some(job.next_attempt_at)).min().map(|value| value.to_string()),
            created_at: self.created_at.to_string(),
            updated_at: self.updated_at.max(jobs.iter().map(|job| job.updated_at).max().unwrap_or(self.updated_at)).to_string(),
        })
    }
}

#[derive(Debug, Default)]
struct QueueState {
    batches: HashMap<TransferJobKind, TransferBatch>,
    history: VecDeque<TransferStatus>,
    next_id: u64,
}

#[derive(Debug, Default)]
pub(crate) struct TransferQueue {
    state: Mutex<QueueState>,
    history_db: Option<DbPool>,
    scheduler_running: AtomicBool,
    authentication_paused: AtomicBool,
    wake_scheduler: Notify,
}

impl TransferQueue {
    pub(crate) fn with_history_db(history_db: DbPool) -> Self {
        Self { history_db: Some(history_db), ..Self::default() }
    }

    pub(super) fn reconcile(&self, mut desired: Vec<TransferJob>) {
        desired.sort_by_key(TransferJob::priority);
        let now = unix_seconds();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        for batch in state.batches.values_mut() {
            let previous_len = batch.jobs.len();
            batch.jobs.retain(|_, queued| queued.state == QueueJobState::Running || desired.iter().any(|candidate| candidate.operation == queued.job.operation));
            if batch.jobs.len() != previous_len {
                batch.updated_at = now;
            }
        }
        state.batches.retain(|_, batch| !batch.jobs.is_empty());

        for candidate in desired {
            let kind = candidate.status_fields().0;
            if let Some(batch) = state.batches.get_mut(&kind) {
                if let Some(existing) = batch.jobs.values_mut().find(|queued| queued.job.operation == candidate.operation) {
                    if candidate.origin() == TransferOrigin::UserInitiated {
                        existing.job.origin = TransferOrigin::UserInitiated;
                        batch.origin = TransferOrigin::UserInitiated;
                    }
                    existing.updated_at = now;
                    batch.updated_at = now;
                    continue;
                }
            }

            state.next_id = state.next_id.saturating_add(1);
            let id = state.next_id;
            let origin = candidate.origin();
            let batch = state.batches.entry(kind).or_insert_with(|| TransferBatch::new(now, origin));
            if origin == TransferOrigin::UserInitiated {
                batch.origin = TransferOrigin::UserInitiated;
            }
            batch.updated_at = now;
            batch.jobs.insert(id, QueueJob { id, job: candidate, state: QueueJobState::Pending, attempts: 0, next_attempt_at: 0, last_error: None, created_at: now, updated_at: now });
        }
        drop(state);
        self.wake_scheduler.notify_one();
    }

    #[cfg(test)]
    pub(super) fn claim_due(&self, allow_background: bool) -> Option<ClaimedTransferJob> {
        let now = unix_seconds();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::claim_due_from(&mut state, now, allow_background)
    }

    pub(super) fn claim_due_batch(&self, allow_background: bool) -> Vec<ClaimedTransferJob> {
        let now = unix_seconds();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(first) = Self::claim_due_from(&mut state, now, allow_background) else { return Vec::new() };
        let Some(kind) = first.job.batch_kind() else { return vec![first] };
        let Some(batch) = state.batches.get_mut(&kind) else { return vec![first] };
        let mut ids = batch
            .jobs
            .values()
            .filter(|queued| queued.state == QueueJobState::Pending && queued.next_attempt_at <= now && (allow_background || queued.job.origin() == TransferOrigin::UserInitiated))
            .map(|queued| queued.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.truncate(MAX_THUMBNAIL_BATCH_ENTRIES.saturating_sub(1));
        let mut claimed = vec![first];
        for id in ids {
            if let Some(queued) = batch.jobs.get_mut(&id) {
                queued.state = QueueJobState::Running;
                queued.updated_at = now;
                claimed.push(ClaimedTransferJob { id, job: queued.job.clone() });
            }
        }
        claimed
    }

    fn claim_due_from(state: &mut QueueState, now: u64, allow_background: bool) -> Option<ClaimedTransferJob> {
        let (kind, id) = state
            .batches
            .iter()
            .flat_map(|(kind, batch)| batch.jobs.values().map(move |queued| (*kind, queued)))
            .filter(|(_, queued)| queued.state == QueueJobState::Pending && queued.next_attempt_at <= now && (allow_background || queued.job.origin() == TransferOrigin::UserInitiated))
            .min_by_key(|(_, queued)| (queued.job.priority(), queued.created_at, queued.id))
            .map(|(kind, queued)| (kind, queued.id))?;
        let queued = state.batches.get_mut(&kind)?.jobs.get_mut(&id)?;
        queued.state = QueueJobState::Running;
        queued.updated_at = now;
        Some(ClaimedTransferJob { id, job: queued.job.clone() })
    }

    pub(super) fn release(&self, claimed: &[ClaimedTransferJob]) {
        let now = unix_seconds();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for item in claimed {
            if let Some(queued) = find_job_mut(&mut state, item.id) {
                queued.state = QueueJobState::Pending;
                queued.updated_at = now;
            }
        }
    }

    pub(super) fn retry(&self, id: u64, message: String) -> Option<Duration> {
        let now = unix_seconds();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let queued = find_job_mut(&mut state, id)?;
        let backoff = (2_u64.pow(queued.attempts.min(8)) * 2).min(600);
        let delay = backoff + now % 3;
        queued.attempts = queued.attempts.saturating_add(1);
        queued.state = QueueJobState::Pending;
        queued.last_error = Some(message);
        queued.next_attempt_at = now.saturating_add(delay);
        queued.updated_at = now;
        Some(Duration::from_secs(delay))
    }

    pub(super) fn complete(&self, id: u64, permanent_error: Option<String>) {
        let completed_status = {
            let now = unix_seconds();
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(kind) = state.batches.iter().find_map(|(kind, batch)| batch.jobs.contains_key(&id).then_some(*kind)) else { return };
            let status = {
                let batch = state.batches.get_mut(&kind).expect("batch disappeared while completing a transfer");
                let Some(removed) = batch.jobs.remove(&id) else { return };
                batch.completed = batch.completed.saturating_add(1);
                batch.attempts = batch.attempts.max(removed.attempts);
                batch.updated_at = now;
                if let Some(error) = permanent_error {
                    batch.permanent_error = Some(error);
                }
                batch.jobs.is_empty().then(|| completed_status(kind, batch, &removed.job))
            };
            if let Some(status) = status {
                state.batches.remove(&kind);
                state.history.push_front(status.clone());
                state.history.truncate(50);
                Some(status)
            } else {
                None
            }
        };
        if let Some(status) = completed_status {
            self.persist_history(&status);
        }
    }

    pub(super) fn next_due_in(&self, allow_background: bool) -> Option<Duration> {
        let now = unix_seconds();
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .batches
            .values()
            .flat_map(|batch| batch.jobs.values())
            .filter(|queued| queued.state == QueueJobState::Pending && (allow_background || queued.job.origin() == TransferOrigin::UserInitiated))
            .map(|queued| Duration::from_secs(queued.next_attempt_at.saturating_sub(now)))
            .min()
    }

    pub(super) fn resume_authentication(&self) {
        self.authentication_paused.store(false, Ordering::Release);
        self.wake_scheduler.notify_one();
    }

    pub(super) fn pause_authentication(&self) {
        self.authentication_paused.store(true, Ordering::Release);
        self.scheduler_running.store(false, Ordering::Release);
    }

    pub(super) fn try_start_scheduler(&self) -> bool {
        !self.authentication_paused.load(Ordering::Acquire) && self.scheduler_running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    pub(super) fn keep_scheduler_or_stop(&self, allow_background: bool) -> bool {
        self.scheduler_running.store(false, Ordering::Release);
        if self.authentication_paused.load(Ordering::Acquire) || self.next_due_in(allow_background).is_none() {
            return false;
        }
        self.scheduler_running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    pub(super) async fn wait_for_wake(&self, delay: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = self.wake_scheduler.notified() => {}
        }
    }

    pub(crate) fn statuses(&self) -> Vec<TransferStatus> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut statuses = state.batches.iter().filter_map(|(kind, batch)| batch.status(*kind)).collect::<Vec<_>>();
        statuses.sort_by_key(|status| status.kind.storage());
        statuses
    }

    pub(crate) fn history(&self) -> Vec<TransferStatus> {
        if let Some(history_db) = &self.history_db {
            let persisted = (|| -> Result<Vec<TransferStatus>, Box<dyn std::error::Error>> {
                let conn = history_db.get()?;
                Ok(crate::library::database::load_transfer_history(&conn)?)
            })();
            match persisted {
                Ok(history) => return history,
                Err(error) => log::warn!("Could not load transfer history: {error}"),
            }
        }
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).history.iter().cloned().collect()
    }

    fn persist_history(&self, status: &TransferStatus) {
        let Some(history_db) = &self.history_db else { return };
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let conn = history_db.get()?;
            Ok(crate::library::database::store_transfer_history(&conn, status)?)
        })();
        if let Err(error) = result {
            log::warn!("Could not persist transfer history: {error}");
        }
    }
}

fn find_job_mut(state: &mut QueueState, id: u64) -> Option<&mut QueueJob> {
    state.batches.values_mut().find_map(|batch| batch.jobs.get_mut(&id))
}

fn completed_status(kind: TransferJobKind, batch: &TransferBatch, representative: &TransferJob) -> TransferStatus {
    let (_, content_hash, book_content_hash, path) = representative.status_fields();
    let failed = batch.permanent_error.is_some();
    TransferStatus {
        id: format!("history-{}-{}", kind.storage(), uuid::Uuid::new_v4()),
        kind,
        content_hash: *content_hash,
        book_content_hash,
        path,
        state: if failed { TransferState::Failed } else { TransferState::Completed },
        origin: batch.origin,
        attempts: batch.attempts,
        completed_items: batch.completed,
        total_items: batch.completed,
        last_error: batch.permanent_error.clone(),
        next_retry_at: None,
        created_at: batch.created_at.to_string(),
        updated_at: batch.updated_at.to_string(),
    }
}

fn unix_seconds() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

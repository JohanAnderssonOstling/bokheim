use super::{TransferJob, TransferOutcome, AUTHENTICATION_REQUIRED_MESSAGE};
use crate::sync::DbPool;
use crate::TransferJobKind;
use app_contract::transfer::{TransferOrigin, TransferState, TransferStatus};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES;
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
struct QueueState {
    jobs: HashMap<u64, QueueJob>,
    progress: HashMap<TransferJobKind, BatchProgress>,
    history: VecDeque<TransferStatus>,
    next_id: u64,
}

#[derive(Debug, Default)]
pub(crate) struct TransferQueue {
    state: Mutex<QueueState>,
    history_db: Option<DbPool>,
    draining: AtomicBool,
    drain_requested: AtomicBool,
}

impl TransferQueue {
    pub(crate) fn with_history_db(history_db: DbPool) -> Self {
        Self { history_db: Some(history_db), ..Self::default() }
    }

    pub(super) fn try_start_drain(&self) -> bool {
        if self.draining.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            true
        } else {
            self.drain_requested.store(true, Ordering::Release);
            false
        }
    }

    pub(super) fn finish_drain(&self) -> bool {
        self.draining.store(false, Ordering::Release);
        self.drain_requested.swap(false, Ordering::AcqRel)
    }

    pub(super) fn reconcile(&self, mut desired: Vec<TransferJob>) {
        desired.sort_by_key(TransferJob::priority);
        let now = unix_seconds();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.jobs.retain(|_, queued| queued.state == QueueJobState::Running || desired.iter().any(|candidate| candidate.same_key(&queued.job)));
        for candidate in desired {
            if let Some(existing) = state.jobs.values_mut().find(|queued| queued.job.same_key(&candidate)) {
                if candidate.origin() == TransferOrigin::UserInitiated && existing.job.origin() != TransferOrigin::UserInitiated {
                    existing.job = existing.job.with_origin(TransferOrigin::UserInitiated);
                }
                existing.updated_at = now;
                continue;
            }
            let kind = candidate.status_fields().0;
            let origin = candidate.origin();
            state.next_id = state.next_id.saturating_add(1);
            let id = state.next_id;
            state.jobs.insert(id, QueueJob { id, job: candidate, state: QueueJobState::Pending, attempts: 0, next_attempt_at: 0, last_error: None, created_at: now, updated_at: now });
            let value = state.progress.entry(kind).or_default();
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
        let mut ids = state
            .jobs
            .values()
            .filter(|queued| queued.state == QueueJobState::Pending && queued.next_attempt_at <= now && queued.job.batch_kind() == Some(kind) && (allow_background || queued.job.origin() == TransferOrigin::UserInitiated))
            .map(|queued| queued.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.truncate(MAX_THUMBNAIL_BATCH_ENTRIES.saturating_sub(1));
        let mut claimed = vec![first];
        for id in ids {
            if let Some(queued) = state.jobs.get_mut(&id) {
                queued.state = QueueJobState::Running;
                queued.updated_at = now;
                claimed.push(ClaimedTransferJob { id, job: queued.job.clone() });
            }
        }
        claimed
    }

    fn claim_due_from(state: &mut QueueState, now: u64, allow_background: bool) -> Option<ClaimedTransferJob> {
        let id = state
            .jobs
            .values()
            .filter(|queued| queued.state == QueueJobState::Pending && queued.next_attempt_at <= now && (allow_background || queued.job.origin() == TransferOrigin::UserInitiated))
            .min_by_key(|queued| (queued.job.priority(), queued.created_at, queued.id))?
            .id;
        let queued = state.jobs.get_mut(&id)?;
        queued.state = QueueJobState::Running;
        queued.updated_at = now;
        Some(ClaimedTransferJob { id, job: queued.job.clone() })
    }

    /// Applies the complete transfer state transition in one place. The
    /// executor reports what happened; the queue owns retries, attempt counts,
    /// authentication pauses, progress, and terminal history.
    pub(super) fn finish(&self, id: u64, outcome: TransferOutcome) -> Option<Duration> {
        match outcome {
            TransferOutcome::Completed => {
                self.finish_terminal(id, None);
                None
            }
            TransferOutcome::Rejected(message) => {
                self.finish_terminal(id, Some(message));
                None
            }
            TransferOutcome::AuthenticationRequired => {
                let now = unix_seconds();
                if let Some(queued) = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).jobs.get_mut(&id) {
                    queued.state = QueueJobState::Pending;
                    queued.last_error = Some(AUTHENTICATION_REQUIRED_MESSAGE.to_owned());
                    queued.next_attempt_at = now;
                    queued.updated_at = now;
                }
                None
            }
            TransferOutcome::Retryable(message) => {
                let now = unix_seconds();
                let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let queued = state.jobs.get_mut(&id)?;
                let backoff = (2_u64.pow(queued.attempts.min(8)) * 2).min(600);
                let delay = backoff + now % 3;
                queued.attempts = queued.attempts.saturating_add(1);
                queued.state = QueueJobState::Pending;
                queued.last_error = Some(message);
                queued.next_attempt_at = now.saturating_add(delay);
                queued.updated_at = now;
                Some(Duration::from_secs(delay))
            }
        }
    }

    fn finish_terminal(&self, id: u64, permanent_error: Option<String>) {
        let completed_status = {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(removed) = state.jobs.remove(&id) else { return };
            let (kind, content_hash, book_content_hash, path) = removed.job.status_fields();
            let content_hash = *content_hash;
            let now = unix_seconds();
            let finished = if let Some(value) = state.progress.get_mut(&kind) {
                value.completed = value.completed.saturating_add(1);
                value.attempts = value.attempts.max(removed.attempts);
                value.updated_at = now;
                if let Some(error) = permanent_error {
                    value.permanent_error = Some(error);
                }
                value.completed >= value.total
            } else {
                false
            };
            if finished {
                let completed = state.progress.remove(&kind).unwrap_or_default();
                let failed = completed.permanent_error.is_some();
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

    pub(super) fn has_due(&self, allow_background: bool) -> bool {
        let now = unix_seconds();
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .jobs
            .values()
            .any(|queued| queued.state == QueueJobState::Pending && queued.next_attempt_at <= now && (allow_background || queued.job.origin() == TransferOrigin::UserInitiated))
    }

    pub(crate) fn statuses(&self) -> Vec<TransferStatus> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let kinds = [TransferJobKind::UploadBook, TransferJobKind::UploadThumbnail, TransferJobKind::DownloadBook, TransferJobKind::DownloadThumbnail];
        kinds
            .into_iter()
            .filter_map(|kind| {
                let mut grouped = state.jobs.values().filter(|queued| queued.job.status_fields().0 == kind).collect::<Vec<_>>();
                grouped.sort_by_key(|queued| (queued.created_at, queued.id));
                let first = *grouped.first()?;
                let transfer_state = if grouped.iter().any(|queued| queued.state == QueueJobState::Running) {
                    TransferState::Running
                } else if grouped.iter().any(|queued| queued.last_error.is_some()) {
                    TransferState::Retrying
                } else {
                    TransferState::Queued
                };
                let batch = state.progress.get(&kind).cloned().unwrap_or_else(|| BatchProgress { completed: 0, total: grouped.len() as u32, ..BatchProgress::default() });
                Some(TransferStatus {
                    id: format!("batch-{}", kind.storage()),
                    kind,
                    content_hash: *first.job.status_fields().1,
                    book_content_hash: None,
                    path: None,
                    state: transfer_state,
                    origin: if grouped.iter().any(|queued| queued.job.origin() == TransferOrigin::UserInitiated) { TransferOrigin::UserInitiated } else { TransferOrigin::Background },
                    attempts: grouped.iter().map(|queued| queued.attempts).max().unwrap_or(0),
                    completed_items: batch.completed,
                    total_items: batch.total.max(grouped.len() as u32),
                    last_error: grouped.iter().find_map(|queued| queued.last_error.clone()),
                    next_retry_at: grouped.iter().filter_map(|queued| (queued.next_attempt_at > 0).then_some(queued.next_attempt_at)).min().map(|value| value.to_string()),
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

fn unix_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

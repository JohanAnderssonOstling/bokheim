use super::error::{SyncError, SyncResult};
use super::DbPool;
use crate::library::database as db;
use library_replica::{coalesce_mutations_for_push, mutations_ready_for_push, prepare_push, MutationBody};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::Duration;
#[cfg(test)]
use sync_client::PushResponseError;
use sync_client::{validate_pull_batch, validate_push_response};
use sync_common::{LibraryId, MutationIssue, PullStateResponse, PushMutationsResponse, ReplicaId};

#[cfg(test)]
pub(super) use library_sqlite::CURSOR_RECOVERY_PENDING_KEY;

pub(super) struct StateSyncEngine<'a> {
    db_pool: &'a DbPool,
    http_client: &'a reqwest::Client,
    library_id: LibraryId,
    replica_id: ReplicaId,
    session: &'a RwLock<Option<sync_client::Session>>,
    inventory_checked: &'a AtomicBool,
}

pub(super) struct StateSyncConfig<'a> {
    pub(super) db_pool: &'a DbPool,
    pub(super) http_client: &'a reqwest::Client,
    pub(super) library_id: LibraryId,
    pub(super) replica_id: ReplicaId,
    pub(super) session: &'a RwLock<Option<sync_client::Session>>,
    pub(super) inventory_checked: &'a AtomicBool,
}

#[derive(Debug, Default)]
pub(super) struct StateSyncResult {
    pub(super) next_reading_upload_in: Option<Duration>,
    pub(super) assets_needed: bool,
    pub(super) mutation_issues: Vec<MutationIssue>,
}

struct AppliedPullPage {
    cursor: sync_common::SyncCursor,
    has_more: bool,
    assets_needed: bool,
}

impl<'a> StateSyncEngine<'a> {
    pub(super) fn new(config: StateSyncConfig<'a>) -> Self {
        Self { db_pool: config.db_pool, http_client: config.http_client, library_id: config.library_id, replica_id: config.replica_id, session: config.session, inventory_checked: config.inventory_checked }
    }

    pub(super) async fn synchronize(&self, reading_debounce: Option<Duration>) -> SyncResult<StateSyncResult> {
        let mut recovery_assets_needed = false;
        if self.cursor_recovery_pending()? {
            recovery_assets_needed |= self.resume_cursor_recovery().await?;
        }

        self.reconcile_state_inventory_once().await?;

        for attempt in 0..2 {
            let pull_cursor = {
                let conn = self.db_pool.get()?;
                db::get_pull_cursor(&conn)?
            };
            let recover_cursor = match self.exchange_events_with_server(pull_cursor, reading_debounce).await {
                Ok(mut result) => {
                    result.assets_needed |= recovery_assets_needed;
                    return Ok(result);
                }
                Err(SyncError::CursorRejected) if attempt == 0 => true,
                Err(error) => return Err(error),
            };
            if recover_cursor {
                self.begin_cursor_recovery()?;
                recovery_assets_needed |= self.resume_cursor_recovery().await?;
            }
        }
        unreachable!("cursor recovery retry must return")
    }

    fn current_session(&self) -> Option<sync_client::Session> {
        self.session.read().ok().and_then(|session| session.clone())
    }

    fn cursor_recovery_pending(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(db::cursor_recovery_pending(&conn)?)
    }

    pub(super) fn begin_cursor_recovery(&self) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        db::begin_cursor_recovery(&conn)?;
        self.inventory_checked.store(false, Ordering::Release);
        Ok(())
    }

    async fn resume_cursor_recovery(&self) -> SyncResult<bool> {
        let mut result = StateSyncResult::default();
        let mut remaining = prepare_push(Vec::new())?;
        let session = self.current_session().ok_or(SyncError::AuthenticationRequired)?;
        let asset_bearing = HashSet::new();
        self.exchange_until_idle(&session, sync_common::SyncCursor::default(), &mut remaining, &asset_bearing, &mut result).await?;
        let inserted = {
            let mut conn = self.db_pool.get()?;
            db::complete_cursor_recovery(&mut conn)?
        };
        self.inventory_checked.store(true, Ordering::Release);
        log::warn!("Recovered a rejected synchronization cursor and queued {inserted} canonical cells for republishing");
        Ok(result.assets_needed)
    }

    async fn reconcile_state_inventory_once(&self) -> SyncResult<()> {
        if self.inventory_checked.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = self.reconcile_state_inventory().await;
        if result.is_err() {
            self.inventory_checked.store(false, Ordering::Release);
        }
        result
    }

    async fn reconcile_state_inventory(&self) -> SyncResult<()> {
        let cells = {
            let conn = self.db_pool.get()?;
            db::state_inventory(&conn)?
        };
        if cells.is_empty() {
            return Ok(());
        }
        let session = self.current_session().ok_or(SyncError::AuthenticationRequired)?;
        let mut missing = Vec::new();
        for chunk in cells.chunks(sync_common::MAX_STATE_INVENTORY_CELLS) {
            let requested = chunk.iter().cloned().collect::<HashSet<_>>();
            let Some(response) = sync_client::compare_state_inventory(self.http_client, &session, self.library_id, chunk.to_vec()).await? else {
                log::debug!("Server does not support state inventory reconciliation yet");
                return Ok(());
            };
            if response.missing.len() > chunk.len() || response.missing.iter().any(|cell| !requested.contains(cell)) {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "server returned an invalid state inventory response").into());
            }
            missing.extend(response.missing);
        }
        if missing.is_empty() {
            return Ok(());
        }
        let inserted = {
            let mut conn = self.db_pool.get()?;
            db::enqueue_missing_state_cells(&mut conn, &missing)?
        };
        log::info!("State inventory reconciliation queued {inserted} missing cells for library {}", self.library_id);
        Ok(())
    }

    fn apply_push_response_semantics(&self, requested_ids: &[sync_common::MutationId], push_response: &PushMutationsResponse) -> SyncResult<Vec<MutationIssue>> {
        let validated = validate_push_response(requested_ids, push_response)?;
        for rejection in &push_response.rejected {
            if rejection.reason.is_transient() {
                log::warn!("Server deferred local sync mutation {}: {:?}", rejection.mutation_id, rejection.reason);
            } else {
                log::error!("Server rejected local sync mutation {}: {:?}", rejection.mutation_id, rejection.reason);
            }
        }

        let completed = validated.completed_ids();
        if !completed.is_empty() {
            let conn = self.db_pool.get()?;
            db::acknowledge_mutations(&conn, &completed)?;
        }
        Ok(validated.issues)
    }

    #[cfg(test)]
    pub(super) fn apply_push_response(&self, requested_ids: &[sync_common::MutationId], push_response: &PushMutationsResponse) -> Result<Vec<sync_common::MutationRejection>, Box<dyn std::error::Error>> {
        let issues = self.apply_push_response_semantics(requested_ids, push_response)?;
        if let Some(MutationIssue::Rejected(rejection)) = issues.iter().find(|issue| matches!(issue, MutationIssue::Rejected(_))) {
            return Err(PushResponseError::PermanentRejection { mutation_id: rejection.mutation_id, reason: rejection.reason }.into());
        }
        Ok(issues
            .into_iter()
            .filter_map(|issue| match issue {
                MutationIssue::Retryable(rejection) => Some(rejection),
                MutationIssue::Rejected(_) | MutationIssue::Untransmittable(_) => None,
            })
            .collect())
    }

    pub(super) async fn exchange_events_with_server(&self, starting_cursor: sync_common::SyncCursor, reading_debounce: Option<Duration>) -> SyncResult<StateSyncResult> {
        let (changes, reading_delay) = {
            let conn = self.db_pool.get()?;
            let now_millis = u64::try_from(web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH)?.as_millis())?;
            mutations_ready_for_push(coalesce_mutations_for_push(db::get_pending_mutations(&conn)?), reading_debounce, now_millis)?
        };
        let session = self.current_session().ok_or(SyncError::AuthenticationRequired)?;
        let asset_bearing = changes.iter().filter(|mutation| matches!(mutation.body, MutationBody::BookLifecycle { .. } | MutationBody::Placement { .. })).map(|mutation| mutation.mutation_id).collect::<HashSet<_>>();
        let mut remaining = prepare_push(changes)?;
        let untransmittable_mutations = remaining.untransmittable().to_vec();
        let mut result = StateSyncResult { next_reading_upload_in: reading_delay, assets_needed: false, mutation_issues: untransmittable_mutations.iter().copied().map(MutationIssue::Untransmittable).collect() };
        for mutation_id in &untransmittable_mutations {
            log::error!("Local sync mutation {mutation_id} encodes to more than the {} byte push batch limit and cannot be transmitted; it stays queued while every other change continues to synchronize", sync_common::MAX_PUSH_BATCH_BYTES);
        }
        self.exchange_until_idle(&session, starting_cursor, &mut remaining, &asset_bearing, &mut result).await?;
        Ok(result)
    }

    async fn exchange_until_idle(
        &self, session: &sync_client::Session, starting_cursor: sync_common::SyncCursor, remaining: &mut sync_common::PushBatcher, asset_bearing: &HashSet<sync_common::MutationId>, result: &mut StateSyncResult,
    ) -> SyncResult<()> {
        let mut cursor = starting_cursor;
        loop {
            let requested = remaining.next_batch();
            let requested_ids = requested.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
            let response = sync_client::exchange_changes(self.http_client, session, self.library_id, self.replica_id, requested, cursor).await?;
            result.assets_needed |= response.push.accepted.iter().any(|mutation_id| asset_bearing.contains(mutation_id));
            let issues = self.apply_push_response_semantics(&requested_ids, &response.push)?;

            // An exchange is one server snapshot. Apply its pull half even when
            // the push half contains a permanent rejection, then report that
            // rejection after the cursor and filesystem debt are committed.
            let pull = self.apply_pull_page(cursor, response.pull)?;
            cursor = pull.cursor;
            result.assets_needed |= pull.assets_needed;
            result.mutation_issues.extend(issues);
            if remaining.is_empty() && !pull.has_more {
                return Ok(());
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn pull_events_from_server(&self, cursor: sync_common::SyncCursor) -> SyncResult<()> {
        let mut result = StateSyncResult::default();
        let mut remaining = prepare_push(Vec::new())?;
        let session = self.current_session().ok_or(SyncError::AuthenticationRequired)?;
        let asset_bearing = HashSet::new();
        self.exchange_until_idle(&session, cursor, &mut remaining, &asset_bearing, &mut result).await
    }

    fn apply_pull_page(&self, cursor: sync_common::SyncCursor, pull_response: PullStateResponse) -> SyncResult<AppliedPullPage> {
        let has_more = pull_response.has_more;
        validate_pull_batch(&pull_response, cursor)?;
        let next_cursor = pull_response.next_cursor;
        let changes = pull_response.mutations;
        let assets_needed = changes.iter().any(|change| matches!(change.mutation.kind.as_str(), "book_lifecycle" | "placement"));

        if changes.is_empty() {
            if next_cursor != cursor {
                let conn = self.db_pool.get()?;
                let tx = conn.unchecked_transaction()?;
                db::set_pull_cursor(&tx, next_cursor)?;
                tx.commit()?;
            }
            return Ok(AppliedPullPage { cursor: next_cursor, has_more, assets_needed });
        }

        let mut conn = self.db_pool.get()?;
        let tx = conn.transaction()?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        for change in &changes {
            db::apply_remote_change_in_tx(&tx, change)?;
        }
        db::set_pull_cursor(&tx, next_cursor)?;
        tx.commit()?;
        drop(conn);

        Ok(AppliedPullPage { cursor: next_cursor, has_more, assets_needed })
    }
}

use super::error::{SyncError, SyncResult};
use super::filesystem::FilesystemProjector;
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
    session: &'a RwLock<Option<crate::auth::Session>>,
    inventory_checked: &'a AtomicBool,
    filesystem: FilesystemProjector<'a>,
}

pub(super) struct StateSyncConfig<'a> {
    pub(super) db_pool: &'a DbPool,
    pub(super) http_client: &'a reqwest::Client,
    pub(super) library_id: LibraryId,
    pub(super) replica_id: ReplicaId,
    pub(super) session: &'a RwLock<Option<crate::auth::Session>>,
    pub(super) inventory_checked: &'a AtomicBool,
    pub(super) filesystem: FilesystemProjector<'a>,
}

#[derive(Debug, Default)]
pub(super) struct StateSyncResult {
    pub(super) next_reading_upload_in: Option<Duration>,
    pub(super) assets_needed: bool,
    pub(super) mutation_issues: Vec<MutationIssue>,
}

#[derive(Debug)]
pub(super) struct StateExchangeResult {
    pub(super) next_reading_upload_in: Option<Duration>,
    pub(super) assets_needed: bool,
    pub(super) mutation_issues: Vec<MutationIssue>,
}

struct PullResult {
    cursor: sync_common::SyncCursor,
    assets_needed: bool,
}

struct AppliedPush {
    issues: Vec<MutationIssue>,
}

impl<'a> StateSyncEngine<'a> {
    pub(super) fn new(config: StateSyncConfig<'a>) -> Self {
        Self { db_pool: config.db_pool, http_client: config.http_client, library_id: config.library_id, replica_id: config.replica_id, session: config.session, inventory_checked: config.inventory_checked, filesystem: config.filesystem }
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
                Ok(exchange) => {
                    return Ok(StateSyncResult { next_reading_upload_in: exchange.next_reading_upload_in, assets_needed: recovery_assets_needed | exchange.assets_needed, mutation_issues: exchange.mutation_issues });
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

    fn current_session(&self) -> Option<crate::auth::Session> {
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
        let assets_needed = self.pull_events_from_server_with_first(sync_common::SyncCursor::default(), None).await?.assets_needed;
        let inserted = {
            let mut conn = self.db_pool.get()?;
            db::complete_cursor_recovery(&mut conn)?
        };
        self.inventory_checked.store(true, Ordering::Release);
        log::warn!("Recovered a rejected synchronization cursor and queued {inserted} canonical cells for republishing");
        Ok(assets_needed)
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
            let response = match sync_client::compare_state_inventory(self.http_client, &session, self.library_id, chunk.to_vec()).await {
                Ok(response) => response,
                Err(sync_client::SyncRequestError::EndpointNotFound) => {
                    log::debug!("Server does not support state inventory reconciliation yet");
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
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

    fn apply_push_response_semantics(&self, requested_ids: &[sync_common::MutationId], push_response: &PushMutationsResponse) -> SyncResult<AppliedPush> {
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
        Ok(AppliedPush { issues: validated.issues })
    }

    #[cfg(test)]
    pub(super) fn apply_push_response(&self, requested_ids: &[sync_common::MutationId], push_response: &PushMutationsResponse) -> Result<Vec<sync_common::MutationRejection>, Box<dyn std::error::Error>> {
        let applied = self.apply_push_response_semantics(requested_ids, push_response)?;
        if let Some(MutationIssue::Rejected(rejection)) = applied.issues.iter().find(|issue| matches!(issue, MutationIssue::Rejected(_))) {
            return Err(PushResponseError::PermanentRejection { mutation_id: rejection.mutation_id, reason: rejection.reason }.into());
        }
        Ok(applied
            .issues
            .into_iter()
            .filter_map(|issue| match issue {
                MutationIssue::Retryable(rejection) => Some(rejection),
                MutationIssue::Rejected(_) | MutationIssue::Untransmittable(_) => None,
            })
            .collect())
    }

    pub(super) async fn exchange_events_with_server(&self, starting_cursor: sync_common::SyncCursor, reading_debounce: Option<Duration>) -> SyncResult<StateExchangeResult> {
        let (changes, reading_delay) = {
            let conn = self.db_pool.get()?;
            mutations_ready_for_push(coalesce_mutations_for_push(db::get_pending_mutations(&conn)?), reading_debounce, std::time::SystemTime::now())?
        };
        let session = self.current_session().ok_or(SyncError::AuthenticationRequired)?;
        let asset_bearing = changes.iter().filter(|mutation| matches!(mutation.body, MutationBody::BookLifecycle { .. } | MutationBody::Placement { .. })).map(|mutation| mutation.mutation_id).collect::<HashSet<_>>();
        let mut remaining = prepare_push(changes)?;
        let untransmittable_mutations = remaining.untransmittable().to_vec();
        let mut cursor = starting_cursor;
        let mut sent_page = false;
        let mut mutation_issues = untransmittable_mutations.iter().copied().map(MutationIssue::Untransmittable).collect::<Vec<_>>();
        let mut assets_needed = false;
        for mutation_id in &untransmittable_mutations {
            log::error!("Local sync mutation {mutation_id} encodes to more than the {} byte push batch limit and cannot be transmitted; it stays queued while every other change continues to synchronize", sync_common::MAX_PUSH_BATCH_BYTES);
        }
        while !remaining.is_empty() || !sent_page {
            let requested = remaining.next_batch();
            let requested_ids = requested.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
            let response = sync_client::exchange_changes(self.http_client, &session, self.library_id, self.replica_id, requested, cursor).await?;
            assets_needed |= response.push.accepted.iter().any(|mutation_id| asset_bearing.contains(mutation_id));
            let applied_push = self.apply_push_response_semantics(&requested_ids, &response.push)?;

            // An exchange is one server snapshot. Apply its pull half even when
            // the push half contains a permanent rejection, then report that
            // rejection after the cursor and filesystem debt are committed.
            let pull = self.pull_events_from_server_with_first(cursor, Some(response.pull)).await?;
            cursor = pull.cursor;
            assets_needed |= pull.assets_needed;
            mutation_issues.extend(applied_push.issues);
            sent_page = true;
        }
        Ok(StateExchangeResult { next_reading_upload_in: reading_delay, assets_needed, mutation_issues })
    }

    #[cfg(test)]
    pub(super) async fn pull_events_from_server(&self, cursor: sync_common::SyncCursor) -> SyncResult<()> {
        self.pull_events_from_server_with_first(cursor, None).await.map(|_| ())
    }

    async fn pull_events_from_server_with_first(&self, starting_cursor: sync_common::SyncCursor, mut first_response: Option<PullStateResponse>) -> SyncResult<PullResult> {
        let session = self.current_session().ok_or(SyncError::AuthenticationRequired)?;
        let mut cursor = starting_cursor;
        let mut assets_needed = false;
        loop {
            let pull_response = match first_response.take() {
                Some(response) => response,
                None => sync_client::exchange_changes(self.http_client, &session, self.library_id, self.replica_id, Vec::new(), cursor).await?.pull,
            };
            let has_more = pull_response.has_more;
            validate_pull_batch(&pull_response, cursor)?;
            let next_cursor = pull_response.next_cursor;
            let changes = pull_response.mutations;
            assets_needed |= changes.iter().any(|change| matches!(change.mutation.kind.as_str(), "book_lifecycle" | "placement"));

            if changes.is_empty() {
                if next_cursor != cursor {
                    let conn = self.db_pool.get()?;
                    let tx = conn.unchecked_transaction()?;
                    db::set_pull_cursor(&tx, next_cursor)?;
                    tx.commit()?;
                }
                cursor = next_cursor;
                break;
            }

            let mut conn = self.db_pool.get()?;
            let tx = conn.transaction()?;
            tx.pragma_update(None, "defer_foreign_keys", "ON")?;
            let mut removed_placements = Vec::new();
            let dirs_before = db::get_live_dir_paths_tx(&tx)?;
            for change in &changes {
                if let Some(removed) = db::apply_remote_change_in_tx(&tx, change)? {
                    removed_placements.push(removed);
                }
            }
            let dirs_after = db::get_live_dir_paths_tx(&tx)?;

            let mut owed = Vec::new();
            owed.extend(dirs_after.iter().filter_map(|(id, new_path)| match dirs_before.get(id) {
                None => Some(db::FsAction::MaterializeDir { dir_id: id.to_string() }),
                Some(old_path) if old_path != new_path => Some(db::FsAction::MoveDir { dir_id: id.to_string(), old_path: old_path.as_str().to_owned(), new_path: new_path.as_str().to_owned() }),
                Some(_) => None,
            }));
            owed.extend(removed_placements.iter().map(|placement| db::FsAction::RetirePlacementFile { dir_id: placement.dir_id.to_string(), content_hash: placement.content_hash.to_string() }));
            let mut dir_deletes: Vec<_> = dirs_before.iter().filter(|(id, _)| !dirs_after.contains_key(*id)).map(|(_, old_path)| old_path.clone()).collect();
            dir_deletes.sort_by_key(|path| std::cmp::Reverse(path.as_path().components().count()));
            owed.extend(dir_deletes.iter().map(|path| db::FsAction::RemoveDir { path: path.as_str().to_owned() }));

            db::enqueue_fs_actions(&tx, &owed)?;
            db::set_pull_cursor(&tx, next_cursor)?;
            tx.commit()?;
            drop(conn);

            self.filesystem.run_owed_actions();
            cursor = next_cursor;
            if !has_more {
                break;
            }
        }
        Ok(PullResult { cursor, assets_needed })
    }
}

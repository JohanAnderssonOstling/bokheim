//! Owner-committed pages for the storage-independent sync engine.
use library_database::DatabaseError;
use sync_common::{PullStateResponse, ReplicaSeq, SyncCursor};
use crate::engine::{StoreError, StoreFuture};

/// Storage commands for a complete native exchange. The local
/// library is authoritative, so commands never validate a stored account
/// binding: switching accounts leaves sync state untouched.
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct AccountStore<'a> {
    pub sync: &'a super::LibrarySync,
}

#[cfg(not(target_arch = "wasm32"))]
impl AccountStore<'_> {
    /// One owner transaction: acknowledge the completed outbox mutations and
    /// apply the incoming page atomically. Every command runs synchronously on
    /// the library's connection, so no transaction spans an `.await`.
    ///
    /// A remote-originated directory rename queues the same durable work a
    /// local move does; the sync scheduler replays it once the pass settles.
    fn commit_response(&self, ids: &[ReplicaSeq], page: &PullStateResponse) -> Result<bool, StoreError> {
        self.sync.database.sync_commit_pull_response(ids, page).map_err(store_error)
    }
    fn asset_work_available(&self) {
        if let Some(events) = &self.sync.events {
            let _ = events.emit(library_runtime::events::LibraryEvent::AssetWorkAvailable);
        }
    }
    /// Batching policy belongs to the orchestrator: one owner transaction
    /// per bounded chunk.
    fn acknowledge(&self, ids: &[ReplicaSeq]) -> Result<(), StoreError> {
        let database = &self.sync.database;
        ids.chunks(256).try_for_each(|chunk| database.sync_acknowledge_mutations(chunk)).map_err(store_error)?;
        if !ids.is_empty() {
            self.asset_work_available();
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::engine::SyncStore for AccountStore<'_> {
    fn cursor_recovery_pending(&self) -> StoreFuture<'_, bool> {
        let pending = self.sync.database.sync_cursor_recovery_pending().map_err(store_error);
        Box::pin(async move { pending })
    }
    fn begin_cursor_recovery(&self) -> StoreFuture<'_, ()> {
        let begun = self.sync.database.sync_begin_cursor_recovery().map_err(store_error);
        Box::pin(async move { begun })
    }
    fn complete_cursor_recovery(&self) -> StoreFuture<'_, usize> {
        // Bounded transactions per page, driven synchronously.
        let mut after = None;
        let mut inserted = 0;
        let recovered: Result<usize, DatabaseError> = loop {
            match self.sync.database.sync_recover_state_page(after.as_ref()) {
                Ok((next, count)) => {
                    inserted += count;
                    if next.is_none() {
                        break Ok(inserted);
                    }
                    after = next;
                }
                Err(error) => break Err(error),
            }
        };
        Box::pin(async move { recovered.map_err(store_error) })
    }
    fn pull_cursor(&self) -> StoreFuture<'_, SyncCursor> {
        let cursor = self.sync.database.sync_pull_cursor().map_err(store_error);
        Box::pin(async move { cursor })
    }
    fn pending_mutations(&self) -> StoreFuture<'_, Vec<library_replica::StatePublication>> {
        let mutations = self.sync.database.sync_publishable_mutations().map_err(store_error);
        Box::pin(async move { mutations })
    }
    fn acknowledge_mutations(&self, ids: &[ReplicaSeq]) -> StoreFuture<'_, ()> {
        let committed = self.acknowledge(ids);
        Box::pin(async move { committed })
    }
    fn apply_pull_page(&self, page: &PullStateResponse) -> StoreFuture<'_, ()> {
        let result = self.commit_response(&[], page).map(|changed| {
            if changed {
                self.sync.notify_remote_page_applied();
            }
        });
        Box::pin(async move { result })
    }
    fn apply_exchange_response(&self, ids: &[ReplicaSeq], page: &PullStateResponse) -> StoreFuture<'_, ()> {
        if ids.is_empty() {
            return self.apply_pull_page(page);
        }
        // Keep the same bound as ordinary acknowledgment commands.
        if ids.len() > 256 {
            let result = self.acknowledge(ids).and_then(|()| {
                self.commit_response(&[], page).map(|changed| {
                    if changed {
                        self.sync.notify_remote_page_applied();
                    }
                })
            });
            return Box::pin(async move { result });
        }
        let result = match self.commit_response(ids, page) {
            Ok(changed) => {
                self.asset_work_available();
                if changed {
                    self.sync.notify_remote_page_applied();
                }
                Ok(())
            }
            Err(error) => {
                // Failed application/commit rolled back the whole command.
                // Re-acknowledge under a fresh transaction, retaining the
                // prior semantics without partially applying pull.
                self.acknowledge(ids).and(Err(error))
            }
        };
        Box::pin(async move { result })
    }
}

fn store_error(error: DatabaseError) -> StoreError {
    Box::new(error)
}

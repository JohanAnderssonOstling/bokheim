//! Queued SQLite commands for the storage-independent sync engine.
use library_runtime::database::{Database, DatabaseError};
use library_runtime::storage_queue::Priority;
use sync_common::{MutationId, PullStateResponse, StateCell, SyncCursor};
use sync_engine::{StoreError, StoreFuture};

/// Captures the account for a complete native exchange or repair pass.
/// Every write checks that binding under the same SQLite write transaction.
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct AccountStore {
    pub database: Database,
    pub user: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AccountStore {
    async fn commit_response(&self, ids: Vec<MutationId>, page: PullStateResponse) -> Result<bool, StoreError> {
        let user = self.user.clone();
        self.database
            .command(Priority::Background, move |conn| {
                // CPU-only wire decoding runs on the storage worker before reserving
                // SQLite's writer. No transaction spans preparation or network I/O.
                let prepared = client_library::remote_reducer::prepare_remote_changes(&page.mutations).map_err(DatabaseError::operation)?;
                let tx = client_library::schema::library_write_transaction(conn)?;
                ensure_sync_account(&tx, &user)?;
                client_library::sync_state::acknowledge_mutations(&tx, &ids)?;
                let changed = apply_prepared_pull_page_in_tx(&tx, &page, &prepared)?;
                tx.commit()?;
                Ok(changed)
            })
            .await
            .map_err(store_error)
    }
    async fn read<T: library_runtime::executor::BackendOutput>(&self, operation: impl FnOnce(&rusqlite::Connection) -> Result<T, DatabaseError> + Send + 'static) -> Result<T, StoreError> {
        let user = self.user.clone();
        self.database
            .command(Priority::Background, move |conn| {
                let tx = conn.transaction()?;
                ensure_sync_account(&tx, &user)?;
                operation(&tx)
            })
            .await
            .map_err(store_error)
    }
    async fn write<T: library_runtime::executor::BackendOutput>(&self, operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, DatabaseError> + Send + 'static) -> Result<T, StoreError> {
        let user = self.user.clone();
        self.database
            .command(Priority::Background, move |conn| {
                let tx = client_library::schema::library_write_transaction(conn)?;
                ensure_sync_account(&tx, &user)?;
                let result = operation(&tx)?;
                tx.commit()?;
                Ok(result)
            })
            .await
            .map_err(store_error)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl sync_engine::SyncStore for AccountStore {
    fn cursor_recovery_pending(&self) -> StoreFuture<'_, bool> {
        Box::pin(self.read(|conn| Ok(client_library::sync_state::cursor_recovery_pending(conn)?)))
    }
    fn begin_cursor_recovery(&self) -> StoreFuture<'_, ()> {
        Box::pin(self.write(|tx| Ok(client_library::sync_state::begin_cursor_recovery_in_tx(tx)?)))
    }
    fn complete_cursor_recovery(&self) -> StoreFuture<'_, usize> {
        Box::pin(super::recovery::complete(|after| self.write(move |tx| Ok(client_library::sync_state::recover_state_page_in_tx(tx, after.as_ref())?))))
    }
    fn pull_cursor(&self) -> StoreFuture<'_, SyncCursor> {
        Box::pin(self.read(|conn| Ok(client_library::sync_state::get_pull_cursor(conn)?)))
    }
    fn inventory_page(&self, after: Option<StateCell>) -> StoreFuture<'_, Vec<StateCell>> {
        Box::pin(self.read(move |conn| Ok(client_library::sync_state::state_inventory_page(conn, after.as_ref())?)))
    }
    fn inventory_checkpoint(&self) -> StoreFuture<'_, Option<StateCell>> {
        Box::pin(self.read(inventory_checkpoint))
    }
    fn save_inventory_checkpoint(&self, after: Option<StateCell>) -> StoreFuture<'_, ()> {
        Box::pin(self.write(move |tx| save_inventory_checkpoint(tx, after)))
    }
    fn enqueue_missing_state_cells(&self, cells: &[StateCell]) -> StoreFuture<'_, usize> {
        let cells = cells.to_vec();
        Box::pin(self.write(move |tx| Ok(client_library::sync_state::enqueue_missing_state_cells_in_tx(tx, &cells)?)))
    }
    fn pending_mutations(&self) -> StoreFuture<'_, Vec<library_replica::StateMutation>> {
        Box::pin(self.read(|conn| client_library::sync_state::get_publishable_mutations(conn).map_err(DatabaseError::operation)))
    }
    fn acknowledge_mutations(&self, ids: &[MutationId]) -> StoreFuture<'_, ()> {
        let ids = ids.to_vec();
        Box::pin(async move {
            for ids in ids.chunks(256) {
                let ids = ids.to_vec();
                self.write(move |tx| {
                    client_library::sync_state::acknowledge_mutations(tx, &ids)?;
                    Ok(())
                })
                .await?;
            }
            Ok(())
        })
    }
    fn apply_pull_page(&self, page: &PullStateResponse) -> StoreFuture<'_, ()> {
        let page = page.clone();
        Box::pin(async move {
            let changed = self.commit_response(Vec::new(), page).await?;
            if changed {
                self.database.remote_page_committed();
            }
            Ok(())
        })
    }
    fn apply_exchange_response(&self, ids: &[MutationId], page: &PullStateResponse) -> StoreFuture<'_, ()> {
        let ids = ids.to_vec();
        let page = page.clone();
        Box::pin(async move {
            if ids.is_empty() {
                return self.apply_pull_page(&page).await;
            }
            // Keep the same bound as ordinary acknowledgment commands.
            if ids.len() > 256 {
                self.acknowledge_mutations(&ids).await?;
                return self.apply_pull_page(&page).await;
            }
            let completed = ids.clone();
            match self.commit_response(completed, page).await {
                Ok(changed) => {
                    if changed {
                        self.database.remote_page_committed();
                    }
                    Ok(())
                }
                Err(error) => {
                    // Failed application/commit rolled back the whole command.
                    // Re-acknowledge under a fresh account-checked transaction,
                    // retaining the prior semantics without partially applying pull.
                    self.acknowledge_mutations(&ids).await?;
                    Err(error)
                }
            }
        })
    }
}

fn store_error(error: DatabaseError) -> StoreError {
    Box::new(error)
}

// Adapt feature-owned persistence errors to the host storage queue.
pub(crate) fn apply_pull_page(conn: &mut rusqlite::Connection, page: &PullStateResponse) -> Result<bool, DatabaseError> {
    client_library::sync_state::apply_pull_page(conn, page).map_err(DatabaseError::operation)
}
fn apply_prepared_pull_page_in_tx(tx: &rusqlite::Transaction<'_>, page: &PullStateResponse, prepared: &client_library::remote_reducer::PreparedRemoteChanges<'_>) -> Result<bool, DatabaseError> {
    client_library::sync_state::apply_prepared_pull_page_in_tx(tx, page, prepared).map_err(DatabaseError::operation)
}

/// Must be called with the same connection that applies the response, after
/// any queued wait. Rebinding a library invalidates the old account's work.
pub(crate) fn ensure_sync_account(conn: &rusqlite::Connection, user: &Option<String>) -> Result<(), DatabaseError> {
    if client_library::sync_state::get_sync_metadata(conn, "bound_user_id")? != *user {
        return Err(DatabaseError::operation("account changed during synchronization"));
    }
    Ok(())
}

pub(crate) fn inventory_checkpoint(conn: &rusqlite::Connection) -> Result<Option<StateCell>, DatabaseError> {
    client_library::sync_state::inventory_checkpoint(conn).map_err(DatabaseError::operation)
}
pub(crate) fn save_inventory_checkpoint(conn: &rusqlite::Connection, after: Option<StateCell>) -> Result<(), DatabaseError> {
    client_library::sync_state::save_inventory_checkpoint(conn, after).map_err(DatabaseError::operation)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod account_tests;

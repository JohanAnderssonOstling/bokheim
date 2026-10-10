//! Direct sync-engine execution and native serialization of sync passes.

use super::*;
use crate::state_sync::AccountStore;

pub(super) async fn lock(sync: &LibrarySync) -> tokio::sync::MutexGuard<'_, ()> {
    sync.sync_gate.lock().await
}

impl LibrarySync {
}

pub(super) async fn exchange(sync: &LibrarySync, _user: &Option<String>, credentials: Option<sync_transport::SyncCredentials>, mode: SyncMode) -> SyncResult<Option<SyncOutcome>> {
    let engine = crate::engine::SyncEngine::new(crate::engine::SyncEngineConfig { store: AccountStore { sync }, http_client: &sync.http_client, library_id: sync.library_id, replica_id: sync.replica_id, credentials });
    let outcome = match mode {
        SyncMode::Full => engine.synchronize().await?,
        SyncMode::Outbox => engine.drain_outbox().await?,
    };
    Ok(Some(outcome))
}

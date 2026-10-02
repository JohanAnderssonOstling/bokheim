//! Direct sync-engine execution and native serialization of sync passes.

use super::*;
use crate::sync::state_sync::AccountStore;

pub(super) async fn lock(sync: &LibrarySync) -> tokio::sync::MutexGuard<'_, ()> {
    sync.sync_gate.lock().await
}

impl LibrarySync {
    #[cfg(test)]
    pub(super) fn state_engine(&self) -> SyncResult<sync_engine::SyncEngine<'_, AccountStore<'_>>> {
        let (_user, credentials) = self.sync_identity()?;
        Ok(sync_engine::SyncEngine::new(sync_engine::SyncEngineConfig { store: AccountStore { sync: self }, http_client: &self.http_client, library_id: self.library_id, replica_id: self.replica_id, credentials }))
    }
}

pub(super) async fn exchange(sync: &LibrarySync, _user: &Option<String>, credentials: Option<sync_transport::SyncCredentials>, mode: SyncMode) -> SyncResult<Option<SyncOutcome>> {
    let engine = sync_engine::SyncEngine::new(sync_engine::SyncEngineConfig { store: AccountStore { sync }, http_client: &sync.http_client, library_id: sync.library_id, replica_id: sync.replica_id, credentials });
    let outcome = match mode {
        SyncMode::Full => engine.synchronize().await?,
        SyncMode::Outbox => engine.drain_outbox().await?,
    };
    Ok(Some(outcome))
}

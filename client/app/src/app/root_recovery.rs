//! Device-owned recovery starts when filesystem access detects a lost root.

use super::core::{AppBackend, AppState};
use crate::executor::{self, BackendExecutor, BackendTask};
use crate::BackendError;
use library_backend::library::handle::{LibraryActorCommand, LibraryControl};
use std::collections::HashSet;
use std::sync::Weak;
use std::time::Duration;
use sync_common::LibraryId;

pub(super) fn start(executor: &BackendExecutor, state: Weak<AppState>, requests: async_channel::Receiver<LibraryId>) -> BackendTask<()> {
    executor.spawn(async move {
        executor::library_maintenance_ready().await;
        let mut pending = HashSet::new();
        loop {
            if pending.is_empty() {
                let Ok(id) = requests.recv().await else { return };
                pending.insert(id);
            }
            while let Ok(id) = requests.try_recv() {
                pending.insert(id);
            }
            let Some(state) = state.upgrade() else { return };
            let backend = AppBackend { state };
            let mut retry = HashSet::new();
            for id in pending.drain() {
                if let Err(error) = backend.recover_library_root(id).await {
                    log::warn!("library folder recovery for {id} will retry: {error}");
                    retry.insert(id);
                }
            }
            drop(backend);
            pending = retry;
            if !pending.is_empty() {
                tokio::select! {
                    result = executor::sleep(Duration::from_secs(30)) => if result.is_err() { return; },
                    request = requests.recv() => match request {
                        Ok(id) => { pending.insert(id); },
                        Err(_) => return,
                    },
                }
            }
        }
    })
}

impl AppBackend {
    pub(super) async fn recover_library_root(&self, id: LibraryId) -> Result<(), BackendError> {
        let _location = self.state.library_location_lock.lock().await;
        let result = async {
            let prepared = self.with_storage(move |this| {
                // A queued failure may outlive removal of its library.
                if !this.state.registry.accessible_to_account(&id, this.state.library_service.account_identity()?.as_deref()).map_err(BackendError::operation)?
                    || this.state.registry.entry(&id).map_err(BackendError::operation)?.is_none()
                    || this.state.registry.is_detached(&id).map_err(BackendError::operation)?
                {
                    return Ok(None);
                }
                this.state.registry.recover_library_root(&id).map_err(BackendError::operation)?;
                let handle = this.state.library_directory.try_get(&id)?;
                // Access already reported a failed storage identity. Reopen
                // even if the original mount returned before discovery ran.
                let (spec, _) = this.library_runtime_spec(id)?;
                Ok(Some((handle, spec)))
            })?;
            let Some((handle, spec)) = prepared else {
                return Ok(());
            };
            match handle {
                Some(handle) => {
                    let (reply, result) = async_channel::bounded(1);
                    handle.commands.send(LibraryActorCommand::RecoverRoot { config: Box::new(spec.config), reply }).await.map_err(|_| BackendError::message("library stopped before folder recovery"))?;
                    tokio::select! {
                        result = result.recv() => result.map_err(BackendError::operation)??,
                        () = handle.commands.closed() => return Err(BackendError::message("library stopped during folder recovery")),
                    }
                }
                None => {
                    self.with_storage(move |this| {
                        this.library_entry(&id)?;
                        this.state.library_directory.open_recovered(spec, &this.state.executor)?;
                        Ok(())
                    })?;
                }
            }
            self.state.registry.remember_library_root(&id).map_err(BackendError::operation)?;
            self.send_library_list_changed();
            Ok(())
        }
        .await;
        if result.is_err() {
            if let Some(handle) = self.state.library_directory.try_get(&id)? {
                let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::RootUnavailable));
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "../../tests/app/root_recovery/tests.rs"]
mod tests;

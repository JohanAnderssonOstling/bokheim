//! Device-owned recovery starts when filesystem access detects a lost root.

use super::LibraryService;
use crate::executor::{self, BackendTask};
use crate::BackendError;
use library_registry::LibraryRootRecovery;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use sync_common::LibraryId;

impl LibraryService {
    pub fn start_root_recovery(self: &Arc<Self>, requests: async_channel::Receiver<LibraryId>) -> BackendTask<()> {
        let state = Arc::downgrade(self);
        self.config.executor.spawn(async move {
            let mut pending = HashSet::new();
            loop {
                if pending.is_empty() {
                    let Ok(id) = requests.recv().await else { return };
                    pending.insert(id);
                }
                while let Ok(id) = requests.try_recv() {
                    pending.insert(id);
                }
                let Some(backend) = state.upgrade() else { return };
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
}

impl LibraryService {
    pub async fn recover_library_root(self: &Arc<Self>, id: LibraryId) -> Result<(), BackendError> {
        // Pause before taking the registry location lock: admitted file work
        // may need management services while it finishes.
        if let Some(handle) = self.try_handle(&id) {
            handle.root_unavailable().await;
        }
        let _location = self.config.library_location_lock.lock().await;
        let result = async {
            let prepared = self
                .run(move |this| {
                    // A queued failure may outlive removal of its library.
                    if !this.config.registry.accessible_to_account(&id, this.account_identity_on_storage()?.as_deref()).map_err(BackendError::operation)?
                        || this.config.registry.entry(&id).map_err(BackendError::operation)?.is_none()
                        || this.config.registry.is_detached(&id).map_err(BackendError::operation)?
                    {
                        return Ok(None);
                    }
                    let entry = match this.config.registry.recover_library_root(&id, &this.effective_library_root()?).map_err(BackendError::operation)? {
                        LibraryRootRecovery::Unchanged(entry) | LibraryRootRecovery::Recovered(entry) => entry,
                    };
                    Ok(Some((this.try_handle(&id), entry.storage_locator().to_owned())))
                })
                .await?;
            let Some((handle, root)) = prepared else {
                return Ok(());
            };
            match handle {
                Some(handle) => {
                    // Access reported a failed identity, so rebind even if the
                    // original mount returned before discovery ran.
                    handle.recover_root(root).await?;
                }
                None => {
                    let (mut options, reset_token) = self.open_options(id).await?;
                    options.reset_local_presence = true;
                    self.open_handle(options, reset_token).await?;
                }
            }
            self.config.registry.access(move |registry| registry.remember_library_root(&id)).await.map_err(BackendError::operation)?;
            self.send_library_list_changed();
            Ok(())
        }
        .await;
        // A failed attempt leaves file access and the UI paused for the retry.
        drop(_location);
        result
    }
}

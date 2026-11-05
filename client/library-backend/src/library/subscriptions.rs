//! Subscription requests and live resources owned by one library.

use super::LibraryHandle;
use crate::BackendError;
#[cfg(target_arch = "wasm32")]
use crate::BookId;
use async_channel::{Receiver, RecvError, TryRecvError};
use library_model::LibraryUpdate;
#[cfg(target_arch = "wasm32")]
use uuid::Uuid;

/// Receives updates across folder changes and stops when the library closes.
#[derive(Clone)]
pub struct LibraryUpdates {
    events: Receiver<LibraryUpdate>,
    owner: Option<LibraryHandle>,
}

impl LibraryUpdates {
    pub(crate) fn new(events: Receiver<LibraryUpdate>, owner: LibraryHandle) -> Self {
        Self { events, owner: Some(owner) }
    }

    pub async fn recv(&self) -> Result<LibraryUpdate, RecvError> {
        match &self.owner {
            Some(owner) => tokio::select! {
                biased;
                _ = owner.closed() => Err(RecvError),
                update = self.events.recv() => update,
            },
            None => self.events.recv().await,
        }
    }

    pub fn try_recv(&self) -> Result<LibraryUpdate, TryRecvError> {
        if self.owner.as_ref().is_some_and(|owner| owner.is_closed()) {
            return Err(TryRecvError::Closed);
        }
        self.events.try_recv()
    }
}

impl From<Receiver<LibraryUpdate>> for LibraryUpdates {
    fn from(events: Receiver<LibraryUpdate>) -> Self {
        Self { events, owner: None }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct BookSubscription {
    handle: LibraryHandle,
    id: Uuid,
}
#[cfg(target_arch = "wasm32")]
impl Drop for BookSubscription {
    fn drop(&mut self) {
        // Resource cleanup is synchronous and must also work after shutdown.
        let resources = &self.handle.owner.resources;
        drop(resources.books.lock().expect("library book leases").remove(&self.id));
    }
}

impl LibraryHandle {
    pub async fn updates(&self) -> Result<(LibraryUpdates, bool), BackendError> {
        let resources = &self.owner.resources;
        let activity = resources.activity.snapshot(false);
        if activity.reason().is_some() {
            resources.events.updates.publish(LibraryUpdate::BookFileActivity(activity));
        }
        let updates = resources.events.updates.subscribe();
        let scanning = resources.scanner_running();
        if self.is_closed() {
            return Err(BackendError::message("library is no longer running"));
        }
        Ok((LibraryUpdates::new(updates, self.clone()), scanning))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn subscribe_book(&self, book_id: BookId) -> Result<(crate::ResolvedBookData, BookSubscription), BackendError> {
        let handle = self.clone();
        self.run_io(async {
            let resources = &self.owner.resources;
            let storage = self.storage().await?;
            let (book, lease) = resources.prepare_book_for_reader(&storage, book_id).await?;
            let id = Uuid::new_v4();
            resources.books.lock().expect("library book leases").insert(id, lease);
            Ok((book, BookSubscription { handle, id }))
        })
        .await
    }
}

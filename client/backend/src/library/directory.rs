//! Library-owned lifecycle directory for already-open library actors.
//!
//! This is deliberately separate from the durable application registry: the
//! registry decides which libraries exist, while this directory owns the
//! running sessions for the current process.

use crate::{BackendError, LibraryId};

use super::{LibraryHandle, LibraryRuntimeSpec};

#[derive(Default)]
pub(crate) struct LibraryDirectory {
    sessions: library_runtime::directory::SessionDirectory<LibraryHandle>,
}

impl LibraryDirectory {
    /// Serializes open/retire transitions with their registry-policy update.
    pub(crate) fn lock_lifecycle(&self) -> Result<std::sync::MutexGuard<'_, ()>, BackendError> {
        self.sessions.lock_lifecycle().map_err(BackendError::operation)
    }

    pub(crate) fn get(&self, id: &LibraryId) -> Result<LibraryHandle, BackendError> {
        self.sessions.get(id).map_err(BackendError::operation)?.ok_or_else(|| BackendError::message(format!("library {id} is not open")))
    }

    /// Opens exactly one session for this process. Holding the directory lock
    /// through construction is intentional: a duplicate open would start a
    /// second scanner/sync actor before either handle could be discarded.
    /// Ensures a session exists, returning whether this call created it.
    ///
    /// The directory retains the handle: application lifecycle only needs to
    /// establish the session, while users of a running library obtain a handle
    /// explicitly through `get`.
    pub(crate) fn ensure_open(&self, spec: LibraryRuntimeSpec, executor: &crate::executor::BackendExecutor) -> Result<bool, BackendError> {
        let id = spec.config.id;
        self.sessions.insert_if_absent(id, || LibraryHandle::open(spec, executor)).map_err(|error| match error {
            library_runtime::directory::InsertError::Directory(error) => BackendError::operation(error),
            library_runtime::directory::InsertError::Open(error) => error,
        })
    }

    /// Removes a session from the process directory and tells its actor to
    /// stop. Registry changes remain app policy; retirement is session
    /// lifecycle and belongs here.
    pub(crate) fn retire(&self, id: &LibraryId) -> Result<(), BackendError> {
        let handle = self.sessions.remove(id).map_err(BackendError::operation)?;
        if let Some(handle) = handle {
            handle.shutdown();
        }
        Ok(())
    }

    pub(crate) fn all(&self) -> Result<Vec<LibraryHandle>, BackendError> {
        self.sessions.all().map_err(BackendError::operation)
    }

    /// Collects presentation snapshots from the open library actors. The
    /// application may display this aggregate, but does not inspect sessions.
    pub(crate) async fn transfer_snapshots(&self) -> Result<Vec<crate::LibraryTransfers>, BackendError> {
        let handles = self.all()?;
        let mut snapshots = Vec::with_capacity(handles.len());
        for handle in handles {
            snapshots.push(handle.transfer_snapshot().await?);
        }
        snapshots.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        Ok(snapshots)
    }

    pub(crate) fn shutdown_all(&mut self) {
        for handle in self.sessions.drain() {
            handle.shutdown();
        }
    }
}

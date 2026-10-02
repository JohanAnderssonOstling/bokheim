//! Library-owned lifecycle directory for already-open library actors.
//!
//! This is deliberately separate from the durable application registry: the
//! registry decides which libraries exist, while this directory owns the
//! running sessions for the current process.

use crate::{BackendError, LibraryId};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use super::{LibraryHandle, LibraryRuntimeSpec};

#[derive(Default)]
pub struct LibraryDirectory {
    pub handles: Mutex<HashMap<LibraryId, LibraryHandle>>,
    lifecycle: Mutex<()>,
}

impl LibraryDirectory {
    /// Serializes open/retire transitions with their registry-policy update.
    pub fn lock_lifecycle(&self) -> Result<std::sync::MutexGuard<'_, ()>, BackendError> {
        self.lifecycle.lock().map_err(|_| BackendError::message("library lifecycle lock is poisoned"))
    }

    pub fn get(&self, id: &LibraryId) -> Result<LibraryHandle, BackendError> {
        self.try_get(id)?.ok_or_else(|| BackendError::message(format!("library {id} is not open")))
    }

    /// Resolves an optional runtime without treating a registered but
    /// unavailable library as a directory failure.
    pub fn try_get(&self, id: &LibraryId) -> Result<Option<LibraryHandle>, BackendError> {
        Ok(self.lock_handles()?.get(id).cloned())
    }

    /// Removes a session from the process directory and tells its actor to
    /// stop. Registry changes remain app policy; retirement is session
    /// lifecycle and belongs here.
    pub fn retire(&self, id: &LibraryId) -> Result<(), BackendError> {
        let handle = self.lock_handles()?.remove(id);
        if let Some(handle) = handle {
            handle.shutdown();
        }
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<LibraryHandle>, BackendError> {
        Ok(self.lock_handles()?.values().cloned().collect())
    }

    fn lock_handles(&self) -> Result<MutexGuard<'_, HashMap<LibraryId, LibraryHandle>>, BackendError> {
        self.handles.lock().map_err(|_| BackendError::message("library directory lock is poisoned"))
    }
}

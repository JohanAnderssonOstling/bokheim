//! Process-local ownership of open library session handles.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use sync_common::LibraryId;

#[derive(Debug)]
pub struct DirectoryError;

impl std::fmt::Display for DirectoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("library directory lock is poisoned")
    }
}

impl std::error::Error for DirectoryError {}

#[derive(Debug)]
pub enum InsertError<E> {
    Directory(DirectoryError),
    Open(E),
}

/// The runtime owns session handles and serializes their lifecycle. The host
/// supplies the concrete handle type and its construction/shutdown behavior.
pub struct SessionDirectory<H> {
    handles: Mutex<HashMap<LibraryId, H>>,
    lifecycle: Mutex<()>,
}

impl<H> Default for SessionDirectory<H> {
    fn default() -> Self {
        Self { handles: Mutex::new(HashMap::new()), lifecycle: Mutex::new(()) }
    }
}

impl<H: Clone> SessionDirectory<H> {
    pub fn lock_lifecycle(&self) -> Result<MutexGuard<'_, ()>, DirectoryError> {
        self.lifecycle.lock().map_err(|_| DirectoryError)
    }

    pub fn get(&self, id: &LibraryId) -> Result<Option<H>, DirectoryError> {
        Ok(self.lock_handles()?.get(id).cloned())
    }

    pub fn all(&self) -> Result<Vec<H>, DirectoryError> {
        Ok(self.lock_handles()?.values().cloned().collect())
    }

    pub fn insert_if_absent<E>(&self, id: LibraryId, open: impl FnOnce() -> Result<H, E>) -> Result<bool, InsertError<E>> {
        let mut handles = self.lock_handles().map_err(InsertError::Directory)?;
        if handles.contains_key(&id) {
            return Ok(false);
        }
        handles.insert(id, open().map_err(InsertError::Open)?);
        Ok(true)
    }

    pub fn remove(&self, id: &LibraryId) -> Result<Option<H>, DirectoryError> {
        Ok(self.lock_handles()?.remove(id))
    }

    pub fn drain(&mut self) -> Vec<H> {
        self.handles.get_mut().unwrap_or_else(|error| error.into_inner()).drain().map(|(_, handle)| handle).collect()
    }

    fn lock_handles(&self) -> Result<MutexGuard<'_, HashMap<LibraryId, H>>, DirectoryError> {
        self.handles.lock().map_err(|_| DirectoryError)
    }
}

//! Library connection access supplied by the application.
use client_platform_runtime::executor::{BackendSend, BoxedBackendFuture};
pub use client_platform_runtime::storage_queue::Priority;
use std::{
    fmt,
    ops::{Deref, DerefMut},
};
#[derive(Debug)]
pub struct FileDatabaseError(String);
impl FileDatabaseError {
    pub fn operation(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}
impl fmt::Display for FileDatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for FileDatabaseError {}
impl From<rusqlite::Error> for FileDatabaseError {
    fn from(error: rusqlite::Error) -> Self {
        Self::operation(error)
    }
}
/// Connections use the library schema configuration. Each checkout owns its
/// connection until dropped; commands run through the host's prioritized storage
/// queue. Filesystem and network work must stay outside command closures.
pub trait FileDatabase: Clone + BackendSend + Sync + 'static {
    type Connection: Deref<Target = rusqlite::Connection> + DerefMut;
    fn get(&self) -> Result<Self::Connection, FileDatabaseError>;
    fn command<T: BackendSend + 'static>(&self, priority: Priority, operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, FileDatabaseError> + BackendSend + 'static) -> BoxedBackendFuture<'static, Result<T, FileDatabaseError>>;
}

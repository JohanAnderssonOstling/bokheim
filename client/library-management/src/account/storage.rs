//! Session persistence uses the existing registry key and storage owner.
use account_client::{Session, SessionFuture, SessionPersistence};
use client_platform_runtime::storage_queue;
use library_registry::LibraryRegistry;
use std::io::{self, Error};

#[derive(Clone, Debug)]
pub struct SessionStorage(LibraryRegistry);
impl SessionStorage {
    pub fn new(registry: LibraryRegistry) -> Self {
        Self(registry)
    }
}
impl SessionPersistence for SessionStorage {
    fn run<T: Send + 'static>(&self, operation: impl FnOnce() -> T + Send + 'static) -> io::Result<T> {
        storage_queue::run_sync(operation).map_err(Error::other)
    }
    fn run_async<T: Send + 'static>(&self, operation: impl FnOnce() -> T + Send + 'static) -> SessionFuture<'_, io::Result<T>> {
        Box::pin(async move { storage_queue::run(operation).await.map_err(Error::other) })
    }
    fn load_account_session(&self) -> io::Result<Option<Session>> {
        self.0.read_value("account_session").map_err(|error| Error::other(error))?.map(|bytes| serde_json::from_slice(&bytes).map_err(|error| Error::new(io::ErrorKind::InvalidData, error))).transpose()
    }
    fn save_account_session(&self, session: &Session) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(session).map_err(|error| Error::new(io::ErrorKind::InvalidData, error))?;
        self.0.write_value("account_session", &bytes).map_err(|error| Error::other(error))
    }
    fn clear_account_session(&self) -> io::Result<()> {
        self.0.remove_value("account_session").map_err(|error| Error::other(error))
    }
}

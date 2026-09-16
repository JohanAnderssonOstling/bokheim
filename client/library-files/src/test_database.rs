//! File-backed connections for exercising scanner transactions without an app.
use crate::database::{FileDatabase, FileDatabaseError, Priority};
use crate::executor::{BackendSend, BoxedBackendFuture};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct TestDatabase {
    path: PathBuf,
    configure: fn(&rusqlite::Connection) -> rusqlite::Result<()>,
}
impl TestDatabase {
    pub(crate) fn open(path: PathBuf, configure: fn(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<Self, FileDatabaseError> {
        let database = Self { path, configure };
        database.get()?;
        Ok(database)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}
impl FileDatabase for TestDatabase {
    type Connection = Box<rusqlite::Connection>;
    fn get(&self) -> Result<Self::Connection, FileDatabaseError> {
        let connection = rusqlite::Connection::open(&self.path)?;
        (self.configure)(&connection)?;
        Ok(Box::new(connection))
    }
    fn command<T: BackendSend + 'static>(&self, _priority: Priority, operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, FileDatabaseError> + BackendSend + 'static) -> BoxedBackendFuture<'static, Result<T, FileDatabaseError>> {
        let database = self.clone();
        Box::pin(async move { crate::executor::run_blocking(move || operation(&mut *database.get()?)).await.map_err(FileDatabaseError::operation)? })
    }
}

//! Generic storage adapters for library file services.

use crate::database::{Database, DatabaseConnection, DatabaseError};
use crate::executor::{BackendSend, BoxedBackendFuture};
use library_files::database::{FileDatabase, FileDatabaseError, Priority};

impl FileDatabase for Database {
    type Connection = DatabaseConnection;

    fn get(&self) -> Result<Self::Connection, FileDatabaseError> {
        Database::get(self).map_err(FileDatabaseError::operation)
    }

    fn command<T: BackendSend + 'static>(&self, priority: Priority, operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, FileDatabaseError> + BackendSend + 'static) -> BoxedBackendFuture<'static, Result<T, FileDatabaseError>> {
        let database = self.clone();
        Box::pin(async move { database.command(priority, move |conn| operation(conn).map_err(DatabaseError::operation)).await.map_err(FileDatabaseError::operation) })
    }
}

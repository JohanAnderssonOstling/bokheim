//! Read snapshots shared by a browse page's component queries.
use crate::{Database, DatabaseError};

impl Database {
    /// Hold one WAL read snapshot across a view's component queries.
    pub(super) fn with_browse_snapshot<T>(&self, read: impl FnOnce() -> Result<T, DatabaseError>) -> Result<T, DatabaseError> {
        let snapshot = self.connection.unchecked_transaction()?;
        let result = read()?;
        snapshot.commit()?;
        Ok(result)
    }
}

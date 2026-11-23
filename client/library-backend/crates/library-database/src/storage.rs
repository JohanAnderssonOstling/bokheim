use crate::{Database, DatabaseError, OwnerSql};
use rusqlite::OptionalExtension;
use sync_common::storage::LibraryStorage;

impl Database {
    pub fn storage_policy(&self) -> Result<LibraryStorage, DatabaseError> {
        let json: Option<String> = self.connection.query_row("SELECT configuration FROM library_storage WHERE singleton=1", [], |row| row.get(0)).optional()?;
        if let Some(json) = json {
            return serde_json::from_str(&json).map_err(DatabaseError::operation);
        }
        Ok(if self.connection.owner_cloud_storage_enabled(|row| row.get::<_, Option<bool>>(0))? == Some(false) { LibraryStorage::Local } else { LibraryStorage::Bokheim })
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn storage_policy() -> LibraryStorage;
    arguments () = ();
    execute database, arguments => database.storage_policy();
}

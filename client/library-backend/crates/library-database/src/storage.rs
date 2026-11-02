use crate::{Database, DatabaseError};
use rusqlite::OptionalExtension;
use sync_common::storage::LibraryStorage;

impl Database {
    pub fn storage_policy(&self) -> Result<LibraryStorage, DatabaseError> {
        let json: Option<String> = self.connection.query_row("SELECT configuration FROM library_storage WHERE singleton=1", [], |row| row.get(0)).optional()?;
        if let Some(json) = json {
            return serde_json::from_str(&json).map_err(DatabaseError::operation);
        }
        Ok(if self.sync_metadata("cloud_storage_enabled")?.as_deref() == Some("0") { LibraryStorage::Local } else { LibraryStorage::Bokheim })
    }
}

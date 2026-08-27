use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::CatalogProviderError;

const CLIENT_SCHEMA_VERSION: i64 = 1;

/// Owns every client-specific catalogue table and serializes access through a
/// single SQLite connection. The neutral base schema is initialized first by
/// `CatalogCache` and remains shared with the server store.
pub(super) struct ClientCatalogStore {
    pub(super) connection: Mutex<Connection>,
}

impl ClientCatalogStore {
    pub(super) fn open(path: &Path) -> Result<Self, CatalogProviderError> {
        let connection = Connection::open(path).map_err(error)?;
        connection.busy_timeout(std::time::Duration::from_secs(10)).map_err(error)?;
        connection.execute_batch("PRAGMA foreign_keys=ON;").map_err(error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS catalog_client_schema_metadata (
                    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                    schema_version INTEGER NOT NULL
                ) STRICT;
                INSERT OR IGNORE INTO catalog_client_schema_metadata(singleton,schema_version) VALUES(1,1);

                CREATE TABLE IF NOT EXISTS catalog_client_registry (
                    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                    payload BLOB NOT NULL
                ) STRICT;

                CREATE TABLE IF NOT EXISTS catalog_subject_publication_cache (
                    epoch TEXT NOT NULL,
                    entry_id INTEGER NOT NULL,
                    publication_payload BLOB NOT NULL,
                    PRIMARY KEY(epoch,entry_id)
                ) STRICT;",
            )
            .map_err(error)?;
        let version = connection.query_row("SELECT schema_version FROM catalog_client_schema_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).map_err(error)?;
        if version != CLIENT_SCHEMA_VERSION {
            return Err(CatalogProviderError::new(format!("unsupported catalogue client schema version {version}")));
        }
        Ok(Self { connection: Mutex::new(connection) })
    }
}

fn error(error: impl std::fmt::Display) -> CatalogProviderError {
    CatalogProviderError::new(error.to_string())
}

use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::CatalogClientError;

const CLIENT_SCHEMA_VERSION: i64 = 6;
const CLIENT_SCHEMA: &str = include_str!("schema.sql");

/// Cache payloads and layouts are tied to a schema/protocol version. Keep each
/// version in its own file so an application upgrade can rebuild its disposable
/// cache without modifying an older application's database.
pub(super) fn cache_path(data_directory: &Path) -> PathBuf {
    data_directory.join(format!("catalog-cache-v{CLIENT_SCHEMA_VERSION}.db"))
}


/// Owns the complete client catalogue cache and serializes access through one
/// SQLite connection. No server crawl or indexing tables are installed.
pub(super) struct ClientCatalogStore {
    pub(super) connection: Mutex<Connection>,
}

impl ClientCatalogStore {
    pub(super) fn open(path: &Path) -> Result<Self, CatalogClientError> {
        let connection = device_sqlite::open(path, device_sqlite::OpenOptions::new().busy_timeout(std::time::Duration::from_secs(10)).journal_mode(device_sqlite::JournalMode::Wal)).map_err(CatalogClientError::new)?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        match existing_client_schema_version(&connection)? {
            Some(CLIENT_SCHEMA_VERSION) => {}
            Some(version) => {
                return Err(CatalogClientError::new(format!("unsupported catalogue client schema version {version}")));
            }
            None => {
                let populated = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%')", [], |row| row.get::<_, bool>(0))?;
                if populated {
                    return Err(CatalogClientError::new("unsupported unversioned catalogue client schema"));
                }
                connection.execute_batch(CLIENT_SCHEMA)?;
                connection.execute(
                    "INSERT INTO catalog_client_schema_metadata(singleton,schema_version) VALUES(1,?1)
                         ON CONFLICT(singleton) DO UPDATE SET schema_version=excluded.schema_version",
                    [CLIENT_SCHEMA_VERSION],
                )?;
            }
        }
        Ok(Self { connection: Mutex::new(connection) })
    }
}

fn existing_client_schema_version(connection: &Connection) -> Result<Option<i64>, CatalogClientError> {
    let exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_client_schema_metadata')", [], |row| row.get::<_, bool>(0))?;
    if !exists {
        return Ok(None);
    }
    connection.query_row("SELECT schema_version FROM catalog_client_schema_metadata WHERE singleton=1", [], |row| row.get(0)).optional().map_err(CatalogClientError::new)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn rejects_old_schema_without_changing_cached_data() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache.db");
        {
            let store = ClientCatalogStore::open(&path).unwrap();
            store.connection.lock().unwrap().execute_batch("UPDATE catalog_client_schema_metadata SET schema_version=3; INSERT INTO catalog_subject_publication_cache VALUES('epoch',7,x'0304');").unwrap();
        }
        assert!(ClientCatalogStore::open(&path).is_err());
        let connection = Connection::open(&path).unwrap();
        assert_eq!(existing_client_schema_version(&connection).unwrap(), Some(3));
        let payload: Vec<u8> = connection.query_row("SELECT publication_payload FROM catalog_subject_publication_cache WHERE epoch='epoch' AND entry_id=7", [], |row| row.get(0)).unwrap();
        assert_eq!(payload, [3,4]);
    }
}

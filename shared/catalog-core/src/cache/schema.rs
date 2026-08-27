#[cfg(not(target_arch = "wasm32"))]
use super::CATALOG_CACHE_SCHEMA_VERSION;
use super::{storage_i64, CatalogCache, CatalogCacheError, CatalogPool, Connection, Duration, PathBuf, Uuid, CATALOG_CACHE_FILE_VERSION, CATALOG_SUBJECT_BROWSE_INDEX_VERSION, CREATE_SCHEMA};
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use web_time::{SystemTime, UNIX_EPOCH};

impl CatalogCache {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, CatalogCacheError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        }
        prepare_database_file(&path)?;
        {
            let connection = Connection::open(&path)?;
            connection.busy_timeout(Duration::from_secs(10))?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(&path).with_init(configure_connection);
        let pool = CatalogPool(r2d2::Pool::builder().max_size(8).build(manager)?);
        initialize_pool(&pool)?;
        Ok(Self { pool })
    }

    /// Opens a cache through SQLite's currently registered VFS without using
    /// host filesystem probes. This is the same cache implementation used by
    /// native clients, with a single connection suitable for browser WASM.
    #[cfg(target_arch = "wasm32")]
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, CatalogCacheError> {
        let mut connection = Connection::open(path.into())?;
        configure_connection(&mut connection)?;
        let pool = CatalogPool(std::rc::Rc::new(std::cell::RefCell::new(connection)));
        initialize_pool(&pool)?;
        Ok(Self { pool })
    }
}

fn initialize_pool(pool: &CatalogPool) -> Result<(), CatalogCacheError> {
    let connection = pool.get()?;
    connection.execute_batch(CREATE_SCHEMA)?;
    connection.execute("INSERT OR IGNORE INTO catalog_transport_state(singleton,epoch) VALUES(1,?1)", [Uuid::new_v4().to_string()])?;
    // A new or still-empty cache can maintain the browse index incrementally
    // from its first assignment. Existing populated caches deliberately remain
    // unmarked until the maintenance pass has rebuilt every ancestor.
    connection.execute(
        "INSERT OR IGNORE INTO catalog_subject_browse_index_metadata(singleton,index_version)
         SELECT 1,?1 WHERE NOT EXISTS(SELECT 1 FROM catalog_publication_subject_assignment)",
        [CATALOG_SUBJECT_BROWSE_INDEX_VERSION],
    )?;
    connection.pragma_update(None, "user_version", CATALOG_CACHE_FILE_VERSION)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn prepare_database_file(path: &Path) -> Result<(), CatalogCacheError> {
    if !path.exists() {
        return Ok(());
    }
    let current = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()
        .and_then(|connection| {
            let file_version = connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).ok()?;
            let schema_version = connection.query_row("SELECT schema_version FROM catalog_cache_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).ok()?;
            Some(file_version == CATALOG_CACHE_FILE_VERSION && schema_version == CATALOG_CACHE_SCHEMA_VERSION)
        })
        .unwrap_or(false);
    if current {
        return Ok(());
    }
    remove_database_file(path)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn remove_database_file(path: &Path) -> Result<(), CatalogCacheError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut target = path.as_os_str().to_os_string();
        target.push(suffix);
        match fs::remove_file(PathBuf::from(target)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CatalogCacheError::new(error.to_string())),
        }
    }
    Ok(())
}

pub(crate) fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    // This database is a rebuildable cache. WAL + NORMAL remains consistent
    // after a crash while avoiding a full filesystem sync for every small
    // queue transaction.
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.busy_timeout(Duration::from_secs(10))
}

pub fn unix_timestamp() -> Result<i64, CatalogCacheError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| CatalogCacheError::new(error.to_string()))?.as_secs();
    storage_i64(timestamp, "Unix timestamp")
}

pub fn advance_catalog_transport_revision(connection: &Connection) -> Result<(), CatalogCacheError> {
    connection.execute("UPDATE catalog_transport_state SET revision=revision+1 WHERE singleton=1", [])?;
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn shared_cache_does_not_install_server_schema() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let connection = cache.pool.get().unwrap();

        let shared_table_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_publication')", [], |row| row.get::<_, bool>(0)).unwrap();
        assert!(shared_table_exists);

        for server_table in ["catalog_server_schema_metadata", "catalog_process_control", "catalog_cover_download", "catalog_epub_extraction", "catalog_metadata_backfill", "catalog_description_backfill", "catalog_browse_serving_metadata"] {
            let exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [server_table], |row| row.get::<_, bool>(0)).unwrap();
            assert!(!exists, "shared cache unexpectedly installed server table {server_table}");
        }
        for client_table in ["catalog_client_value", "catalog_client_schema_metadata", "catalog_client_registry", "catalog_subject_publication_cache"] {
            let exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [client_table], |row| row.get::<_, bool>(0)).unwrap();
            assert!(!exists, "shared cache unexpectedly installed client table {client_table}");
        }
    }
}

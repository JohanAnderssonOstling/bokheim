#[cfg(not(target_arch = "wasm32"))]
use super::CATALOG_CACHE_SCHEMA_VERSION;
use super::{
    params, storage_i64, CatalogCache, CatalogCacheError, CatalogPool, Connection, Duration, OptionalExtension, PathBuf, Uuid, CATALOG_CACHE_FILE_VERSION, CATALOG_SUBJECT_BROWSE_INDEX_VERSION, CREATE_SCHEMA, MAX_CRAWL_ERROR_BYTES,
};
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub fn open_vfs(path: impl Into<PathBuf>) -> Result<Self, CatalogCacheError> {
        let mut connection = Connection::open(path.into())?;
        configure_connection(&mut connection)?;
        let pool = CatalogPool(std::rc::Rc::new(std::cell::RefCell::new(connection)));
        initialize_pool(&pool)?;
        Ok(Self { pool })
    }
}

fn initialize_pool(pool: &CatalogPool) -> Result<(), CatalogCacheError> {
    let connection = pool.get()?;
    ensure_cover_download_process(&connection)?;
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
    ensure_description_backfill_outcome_column(&connection)?;
    connection.pragma_update(None, "user_version", CATALOG_CACHE_FILE_VERSION)?;
    let now = unix_timestamp()?;
    connection.execute(
        "INSERT OR IGNORE INTO catalog_epub_failure(source_id,acquisition_url,category,retryable,first_failed_at,last_failed_at,next_retry_at)
         SELECT source_id,acquisition_url,'legacy_unclassified',1,?1,?1,?2
         FROM catalog_epub_extraction WHERE status='failed'",
        params![now, now.saturating_add(15 * 60)],
    )?;
    connection.execute("UPDATE catalog_source SET crawl_status='failed', last_crawl_error='crawl interrupted by service restart' WHERE crawl_status='running'", [])?;
    connection.execute("UPDATE catalog_epub_extraction SET status='pending', phase=NULL, last_error='extraction interrupted by service restart' WHERE status='running'", [])?;
    connection.execute("UPDATE catalog_cover_download SET status='pending', last_error='cover download interrupted by service restart' WHERE status='running'", [])?;
    connection.execute("UPDATE catalog_metadata_backfill SET status='pending', last_error='metadata backfill interrupted by service restart' WHERE status='running'", [])?;
    connection.execute("UPDATE catalog_description_backfill SET status='pending', last_error='description backfill interrupted by service restart' WHERE status='running'", [])?;
    Ok(())
}

fn ensure_cover_download_process(connection: &Connection) -> Result<(), CatalogCacheError> {
    let schema = connection.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='catalog_process_control'", [], |row| row.get::<_, String>(0)).optional()?;
    if schema.as_deref().is_none_or(|schema| schema.contains("cover_downloads")) {
        return Ok(());
    }
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         ALTER TABLE catalog_process_control RENAME TO catalog_process_control_legacy;
         CREATE TABLE catalog_process_control (
             process TEXT PRIMARY KEY CHECK(process IN ('catalogue_crawling','cover_downloads','epub_enrichment','catalogue_enrichment')),
             enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
         );
         INSERT INTO catalog_process_control(process,enabled)
             SELECT process,enabled FROM catalog_process_control_legacy;
         INSERT INTO catalog_process_control(process,enabled) VALUES('cover_downloads',0);
         DROP TABLE catalog_process_control_legacy;
         COMMIT;",
    )?;
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

pub(crate) fn ensure_description_backfill_outcome_column(connection: &Connection) -> Result<(), CatalogCacheError> {
    let exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('catalog_description_backfill') WHERE name='outcome')", [], |row| row.get::<_, bool>(0))?;
    if !exists {
        connection.execute("ALTER TABLE catalog_description_backfill ADD COLUMN outcome TEXT CHECK(outcome IS NULL OR outcome = 'no_description')", [])?;
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

pub(crate) fn unix_timestamp() -> Result<i64, CatalogCacheError> {
    #[cfg(target_arch = "wasm32")]
    let timestamp = (js_sys::Date::now() / 1_000.0) as u64;
    #[cfg(not(target_arch = "wasm32"))]
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| CatalogCacheError::new(error.to_string()))?.as_secs();
    storage_i64(timestamp, "Unix timestamp")
}

pub(crate) fn advance_catalog_transport_revision(connection: &Connection) -> Result<(), CatalogCacheError> {
    connection.execute("UPDATE catalog_transport_state SET revision=revision+1 WHERE singleton=1", [])?;
    Ok(())
}

pub(crate) fn bounded_crawl_error(error: &str) -> String {
    let mut boundary = error.len().min(MAX_CRAWL_ERROR_BYTES);
    while !error.is_char_boundary(boundary) {
        boundary -= 1;
    }
    error[..boundary].to_owned()
}

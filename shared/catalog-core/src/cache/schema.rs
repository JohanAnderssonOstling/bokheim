use super::{
    fs, params, storage_i64, CatalogCache, CatalogCacheError, Connection, Duration, Path, PathBuf, Pool, SqliteConnectionManager, SystemTime, CATALOG_CACHE_FILE_VERSION, CATALOG_CACHE_SCHEMA_VERSION, CREATE_SCHEMA, MAX_CRAWL_ERROR_BYTES,
    UNIX_EPOCH,
};

impl CatalogCache {
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
        let manager = SqliteConnectionManager::file(&path).with_init(configure_connection);
        let pool = Pool::builder().max_size(8).build(manager)?;
        {
            let connection = pool.get()?;
            connection.execute_batch(CREATE_SCHEMA)?;
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
            connection.execute("UPDATE catalog_metadata_backfill SET status='pending', last_error='metadata backfill interrupted by service restart' WHERE status='running'", [])?;
            connection.execute("UPDATE catalog_description_backfill SET status='pending', last_error='description backfill interrupted by service restart' WHERE status='running'", [])?;
        }
        Ok(Self { pool })
    }
}

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
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| CatalogCacheError::new(error.to_string()))?.as_secs();
    storage_i64(timestamp, "Unix timestamp")
}

pub(crate) fn bounded_crawl_error(error: &str) -> String {
    let mut boundary = error.len().min(MAX_CRAWL_ERROR_BYTES);
    while !error.is_char_boundary(boundary) {
        boundary -= 1;
    }
    error[..boundary].to_owned()
}

use super::*;

const SERVER_SCHEMA: &str = include_str!("schema.sql");
const SERVER_SCHEMA_VERSION: i64 = 2;

pub(super) fn initialize_server_schema(store: &CatalogStore) -> Result<(), CatalogCacheError> {
    let mut connection = store.connection()?;
    if let Some(schema_version) = existing_server_schema_version(&connection)? {
        validate_server_schema_version(schema_version)?;
    }
    ensure_cover_download_process(&connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SERVER_SCHEMA)?;
    let schema_version = existing_server_schema_version(&transaction)?.ok_or_else(|| CatalogCacheError::new("catalogue server schema has no version"))?;
    validate_server_schema_version(schema_version)?;
    ensure_description_backfill_outcome_column(&transaction)?;

    let now = unix_timestamp()?;
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_epub_failure(source_id,acquisition_url,category,retryable,first_failed_at,last_failed_at,next_retry_at)
         SELECT source_id,acquisition_url,'legacy_unclassified',1,?1,?1,?2
         FROM catalog_epub_extraction WHERE status='failed'",
        params![now, now.saturating_add(15 * 60)],
    )?;
    transaction.execute("UPDATE catalog_source SET crawl_status='failed', last_crawl_error='crawl interrupted by service restart' WHERE crawl_status='running'", [])?;
    transaction.execute("UPDATE catalog_epub_extraction SET status='pending', phase=NULL, last_error='extraction interrupted by service restart' WHERE status='running'", [])?;
    transaction.execute("UPDATE catalog_cover_download SET status='pending', last_error='cover download interrupted by service restart' WHERE status='running'", [])?;
    transaction.execute("UPDATE catalog_metadata_backfill SET status='pending', last_error='metadata backfill interrupted by service restart' WHERE status='running'", [])?;
    transaction.execute("UPDATE catalog_description_backfill SET status='pending', last_error='description backfill interrupted by service restart' WHERE status='running'", [])?;
    transaction.commit()?;
    Ok(())
}

fn existing_server_schema_version(connection: &Connection) -> Result<Option<i64>, CatalogCacheError> {
    let metadata_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_server_schema_metadata')", [], |row| row.get::<_, bool>(0))?;
    if !metadata_exists {
        return Ok(None);
    }
    connection.query_row("SELECT schema_version FROM catalog_server_schema_metadata WHERE singleton=1", [], |row| row.get(0)).optional().map_err(Into::into)
}

fn validate_server_schema_version(schema_version: i64) -> Result<(), CatalogCacheError> {
    if schema_version != SERVER_SCHEMA_VERSION {
        return Err(CatalogCacheError::new(format!("unsupported catalogue server schema version {schema_version}")));
    }
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

fn ensure_description_backfill_outcome_column(connection: &Connection) -> Result<(), CatalogCacheError> {
    let exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('catalog_description_backfill') WHERE name='outcome')", [], |row| row.get::<_, bool>(0))?;
    if !exists {
        connection.execute("ALTER TABLE catalog_description_backfill ADD COLUMN outcome TEXT CHECK(outcome IS NULL OR outcome = 'no_description')", [])?;
    }
    Ok(())
}

pub(crate) fn bounded_crawl_error(error: &str) -> String {
    let mut boundary = error.len().min(MAX_CRAWL_ERROR_BYTES);
    while !error.is_char_boundary(boundary) {
        boundary -= 1;
    }
    error[..boundary].to_owned()
}

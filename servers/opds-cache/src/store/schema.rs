use super::*;
use std::path::Path;

const SERVER_SCHEMA: &str = include_str!("schema.sql");
const SERVER_SCHEMA_VERSION: i64 = 6;

pub(super) fn initialize_server_schema(store: &CatalogStore) -> Result<(), CatalogStoreError> {
    let mut connection = store.connection()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    migrate_process_control(&transaction)?;
    transaction.execute_batch(SERVER_SCHEMA)?;
    super::serving_changes::install(&transaction)?;
    let has_staged_path = transaction.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('catalog_epub_extraction') WHERE name='staged_path')", [], |row| row.get::<_, bool>(0))?;
    if !has_staged_path {
        transaction.execute("ALTER TABLE catalog_epub_extraction ADD COLUMN staged_path TEXT", [])?;
    }
    let has_staged_metadata_only = transaction.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('catalog_epub_extraction') WHERE name='staged_metadata_only')", [], |row| row.get::<_, bool>(0))?;
    if !has_staged_metadata_only {
        transaction.execute("ALTER TABLE catalog_epub_extraction ADD COLUMN staged_metadata_only INTEGER NOT NULL DEFAULT 0", [])?;
    }
    transaction.execute("INSERT OR IGNORE INTO catalog_server_schema_metadata(singleton,schema_version) VALUES(1,?1)", [SERVER_SCHEMA_VERSION])?;
    let schema_version = existing_server_schema_version(&transaction)?.ok_or_else(|| CatalogStoreError::new("catalogue server schema has no version"))?;
    validate_server_schema_version(schema_version)?;
    transaction.execute("INSERT OR IGNORE INTO catalog_transport_state(singleton,epoch) VALUES(1,lower(hex(randomblob(16))))", [])?;
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_subject_browse_index_metadata(singleton,index_version)
         SELECT 1,?1 WHERE NOT EXISTS(SELECT 1 FROM catalog_book_subject_assignment)",
        [CATALOG_SUBJECT_BROWSE_INDEX_VERSION],
    )?;

    let now = unix_timestamp()?;
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_epub_failure(source_id,acquisition_url,category,retryable,first_failed_at,last_failed_at,next_retry_at)
         SELECT source_id,acquisition_url,'legacy_unclassified',1,?1,?1,?2
         FROM catalog_epub_extraction WHERE status='failed'",
        params![now, now.saturating_add(15 * 60)],
    )?;
    transaction.execute("UPDATE catalog_source SET crawl_status='failed', last_crawl_error='crawl interrupted by service restart' WHERE crawl_status='running'", [])?;
    transaction.execute(
        "UPDATE catalog_epub_extraction
            SET status='pending', phase=NULL, staged_path=NULL, last_error='extraction interrupted by service restart'
          WHERE status='running' AND phase!='staged'",
        [],
    )?;
    transaction.execute("UPDATE catalog_cover_download SET status='pending', last_error='cover download interrupted by service restart' WHERE status='running'", [])?;
    transaction.execute("UPDATE catalog_metadata_backfill SET status='pending', last_error='metadata backfill interrupted by service restart' WHERE status='running'", [])?;
    transaction.execute("UPDATE catalog_metadata_backfill SET ocr_status='pending', ocr_last_error='OCR backfill interrupted by service restart' WHERE ocr_status='running'", [])?;
    transaction.execute("UPDATE catalog_description_backfill SET status='pending', last_error='description backfill interrupted by service restart' WHERE status='running'", [])?;
    transaction.commit()?;
    Ok(())
}

fn migrate_process_control(transaction: &rusqlite::Transaction<'_>) -> Result<(), CatalogStoreError> {
    let exists = transaction.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_process_control')", [], |row| row.get::<_, bool>(0))?;
    if !exists {
        return Ok(());
    }
    let has_enrichment = transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_process_control WHERE process='enrichment')", [], |row| row.get::<_, bool>(0))?;
    if has_enrichment {
        return Ok(());
    }
    let enrichment_enabled = transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_process_control WHERE process IN ('epub_enrichment','catalogue_enrichment') AND enabled=1)", [], |row| row.get::<_, bool>(0))?;
    transaction.execute_batch(
        "ALTER TABLE catalog_process_control RENAME TO catalog_process_control_legacy;
         CREATE TABLE catalog_process_control (
             process TEXT PRIMARY KEY CHECK(process IN ('catalogue_crawling','cover_downloads','enrichment')),
             enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
         );",
    )?;
    transaction.execute(
        "INSERT INTO catalog_process_control(process,enabled)
         SELECT process,enabled FROM catalog_process_control_legacy
         WHERE process IN ('catalogue_crawling','cover_downloads')",
        [],
    )?;
    transaction.execute("INSERT INTO catalog_process_control(process,enabled) VALUES('enrichment',?1)", [enrichment_enabled])?;
    transaction.execute("DROP TABLE catalog_process_control_legacy", [])?;
    Ok(())
}

pub(super) fn prepare_database_file(path: &Path) -> Result<(), CatalogStoreError> {
    // Never delete a catalogue cache during startup. A binary redeploy must
    // preserve crawled pages and enrichment state; incompatible schemas must
    // fail closed and be handled by an explicit migration/backup procedure.
    let _ = path;
    Ok(())
}

pub(super) fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.busy_timeout(std::time::Duration::from_secs(10))
}

fn existing_server_schema_version(connection: &Connection) -> Result<Option<i64>, CatalogStoreError> {
    let metadata_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_server_schema_metadata')", [], |row| row.get::<_, bool>(0))?;
    if !metadata_exists {
        return Ok(None);
    }
    connection.query_row("SELECT schema_version FROM catalog_server_schema_metadata WHERE singleton=1", [], |row| row.get(0)).optional().map_err(Into::into)
}

fn validate_server_schema_version(schema_version: i64) -> Result<(), CatalogStoreError> {
    if schema_version != SERVER_SCHEMA_VERSION {
        return Err(CatalogStoreError::new(format!("unsupported catalogue server schema version {schema_version}")));
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

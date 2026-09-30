//! The sole owner of a concrete library's SQLite connection.
//!
//! Other crates use typed database operations; they never own a SQLite
//! connection or select a connection policy.

use crate::annotations::{IndexSql as AnnotationsIndexSql, TablesSql as AnnotationsTablesSql};
use crate::assets::TablesSql as AssetsTablesSql;
use crate::enrichment::{IndexSql as EnrichmentIndexSql, TablesSql as EnrichmentTablesSql, TriggersSql as EnrichmentTriggersSql};
use crate::native::{IndexSql as NativeIndexSql, TablesSql as NativeTablesSql, TriggersSql as NativeTriggersSql, ViewsSql as NativeViewsSql};
use crate::placements::{IndexSql as PlacementsIndexSql, TablesSql as PlacementsTablesSql};
use crate::shared_sql::SharedSql;
use crate::sync::{IndexSql as SyncIndexSql, TablesSql as SyncTablesSql, TriggersSql as SyncTriggersSql};
#[macro_use]
mod query_methods;
mod contributors;
mod purge;
mod shared_sql;
mod taxonomy_update;
#[cfg(test)]
mod test_support;
mod timing;
mod transactions;

pub const LIBRARY_SCHEMA_VERSION: i64 = 7;

pub use client_platform_runtime::executor;
pub use client_platform_runtime::storage_queue;
pub use rusqlite;

#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_native::sqlite;
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::sqlite;

pub use library_replica::{BookPlacement, BookUploadIntent, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
use sync_common::ContentHash;

// Connection ownership for one concrete library session.
use std::fmt;
use std::path::{Path, PathBuf};

use include_sqlite_sql::include_sql;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibrarySyncDetails {
    pub has_unsynced_changes: bool,
    pub last_synced_at: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum DatabaseError {
    Message(String),
    Operation(std::sync::Arc<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => formatter.write_str(message),
            Self::Operation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Operation(error) => Some(error.as_ref()),
            Self::Message(_) => None,
        }
    }
}

impl DatabaseError {
    pub fn operation(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Operation(std::sync::Arc::from(error.into()))
    }
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl From<rusqlite::Error> for DatabaseError {
    fn from(error: rusqlite::Error) -> Self {
        Self::operation(error)
    }
}

/// Connection policy owned by the database crate: busy handler, pragmas,
/// sync functions, and the read-only taxonomy attach. Callers open a path
/// and get a configured connection; no external setup function is needed.
fn configure_library_connection(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    // A scan/import touches more statements than rusqlite's default cache can
    // hold. Keep its write and reconciliation statements hot between books.
    connection.set_prepared_statement_cache_capacity(128);
    connection.busy_timeout(std::time::Duration::from_secs(30))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let journal_mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    crate::sync::functions::register_sync_functions(connection)?;
    attach_curated_taxonomy_to(connection).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

/// Scalar flag coalescing metadata payloads without a persistent batch flag.
fn configure_metadata_batch_flag(connection: &rusqlite::Connection) -> rusqlite::Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let flag = std::sync::Arc::new(AtomicBool::new(false));
    let active = flag.clone();
    connection.create_scalar_function("metadata_batch_flag", 1, rusqlite::functions::FunctionFlags::SQLITE_UTF8, move |ctx| {
        Ok(match ctx.get::<Option<bool>>(0)? {
            Some(value) => active.swap(value, Ordering::Relaxed),
            None => active.load(Ordering::Relaxed),
        })
    })?;
    Ok(flag)
}

fn attach_curated_taxonomy_to(connection: &rusqlite::Connection) -> Result<(), DatabaseError> {
    let mut attached = false;
    connection.taxonomy_attached(|row| {
        attached = row.get(0)?;
        Ok(())
    })?;
    if attached {
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = subject_projection::installed::active_path() {
        let uri = format!("{}?mode=ro&immutable=1", url_for_taxonomy(path)?);
        connection.taxonomy_attach(&uri)?;
        return Ok(());
    }
    connection.taxonomy_attach(":memory:")?;
    // SAFETY: this non-owning wrapper is confined to connection
    // configuration; no statement is active and it cannot outlive or
    // close the original handle. SQLite reads the static bundled bytes
    // directly without copying the tree.
    let mut borrowed = unsafe { rusqlite::Connection::from_handle(connection.handle())? };
    borrowed.deserialize_bytes("curated", subject_projection::DEFAULT_UNIFIED_TAXONOMY_SQLITE)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn url_for_taxonomy(path: &Path) -> Result<String, DatabaseError> {
    let path = path.to_str().ok_or_else(|| DatabaseError::message("taxonomy path is not UTF-8"))?;
    Ok(format!("file:{}", path.bytes().map(|b| if b.is_ascii_alphanumeric() || b"/-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>()))
}

pub struct Database {
    pub(crate) connection: rusqlite::Connection,
    path: PathBuf,
    metadata_batch_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

/// Queues every book's newest local version in a live folder for upload.
fn requeue_cloud_storage(transaction: &rusqlite::Transaction<'_>) -> Result<(), DatabaseError> {
    let mut candidates = Vec::new();
    transaction.owner_cloud_storage_candidates(|row| {
        candidates.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?));
        Ok(())
    })?;
    let mut folders = placements::FolderPaths::new(transaction);
    let mut queued = std::collections::HashSet::new();
    for (content_hash, dir_id, checksum, size_bytes) in candidates {
        if !queued.contains(&content_hash) && folders.folder(&dir_id)?.is_some() {
            transaction.owner_queue_book_upload(&content_hash, &checksum, size_bytes)?;
            queued.insert(content_hash);
        }
    }
    Ok(())
}

impl Database {
    pub fn initialize_library(&self) -> Result<sync_common::ReplicaId, DatabaseError> {
        configure_library_connection(&self.connection)?;
        // Connection pragmas and the taxonomy attachment must be configured
        // before the transaction. Each committed schema and replica identity
        // becomes visible with its matching version; failed transactions roll back.
        let transaction = self.connection.unchecked_transaction()?;
        const VERSION: i64 = LIBRARY_SCHEMA_VERSION;
        let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == VERSION {
            let mut raw: Option<String> = None;
            transaction.shared_owner_replica_id(|row| {
                raw = row.get::<_, Option<String>>(0)?;
                Ok(())
            })?;
            let replica_id = raw.ok_or_else(|| DatabaseError::message("library has no replica id"))?.parse().map_err(DatabaseError::operation)?;
            transaction.commit()?;
            self.refresh_taxonomy_assignments()?;
            return Ok(replica_id);
        }
        if version != 0 {
            return Err(DatabaseError::message(format!("unsupported library schema version {version}; expected a new library or version {VERSION}")));
        }
        transaction.schema_sync_tables()?;
        transaction.schema_native_tables()?;
        transaction.schema_placements_tables()?;
        transaction.schema_assets_tables()?;
        transaction.schema_enrichment_tables()?;
        transaction.schema_annotations_tables()?;

        // Initial singleton rows and the root directory belong to a new library.
        transaction.execute_batch(
            "INSERT OR IGNORE INTO sync_clock VALUES(1,0);
            INSERT OR IGNORE INTO sync_metadata(singleton) VALUES(1);
            INSERT INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle,deleted_at)
            VALUES('00000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000','','00000000-0000-0000-0000-000000000000','',0,NULL)
            ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id,name=excluded.name,deleted_at=NULL;",
        )?;

        transaction.schema_sync_index()?;
        transaction.schema_native_index()?;
        transaction.schema_placements_index()?;
        transaction.schema_enrichment_index()?;
        transaction.schema_annotations_index()?;
        transaction.schema_native_views()?;
        transaction.schema_sync_triggers()?;
        transaction.schema_native_triggers()?;
        transaction.schema_enrichment_triggers()?;
        let replica_id = sync_common::ReplicaId::new_v4();
        transaction.owner_set_change_origin("local")?;
        transaction.owner_set_replica_id(&replica_id.to_string())?;
        transaction.pragma_update(None, "user_version", VERSION)?;
        transaction.commit()?;
        self.refresh_taxonomy_assignments()?;
        Ok(replica_id)
    }

    pub fn set_remote_library_name(&self, name: &str) -> Result<(), DatabaseError> {
        self.connection.owner_set_remote_library_name(name)?;
        Ok(())
    }

    pub fn sync_remote_library_name(&self) -> Result<Option<String>, DatabaseError> {
        let mut name = None;
        self.connection.shared_owner_remote_library_name(|row| {
            name = row.get(0)?;
            Ok(())
        })?;
        Ok(name)
    }

    pub fn load_transfer_history(&self) -> Result<Option<String>, DatabaseError> {
        let mut history = None;
        self.connection.owner_transfer_history(|row| {
            history = Some(row.get(0)?);
            Ok(())
        })?;
        Ok(history)
    }

    pub fn save_transfer_history(&self, entries_json: &str) -> Result<(), DatabaseError> {
        self.connection.owner_save_transfer_history(entries_json)?;
        Ok(())
    }

    /// Returns the locally stored name when this account/server still needs a
    /// remote library created, or `None` when creation was already recorded.
    pub fn remote_library_name_to_create(&self, identity: &str) -> Result<Option<String>, DatabaseError> {
        let mut created: Option<String> = None;
        self.connection.shared_owner_server_library_created(|row| {
            created = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        if created.as_deref() == Some(identity) {
            return Ok(None);
        }
        let mut name: Option<String> = None;
        self.connection.shared_owner_remote_library_name(|row| {
            name = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let name = name.ok_or_else(|| DatabaseError::message("library has no remote name"))?;
        Ok(Some(name))
    }

    /// Records only a successful remote-library creation for the supplied
    /// account/server identity.
    pub fn record_remote_library_created(&self, identity: &str) -> Result<(), DatabaseError> {
        self.connection.owner_set_server_library_created(identity)?;
        Ok(())
    }

    pub fn stored_sync_status(&self) -> Result<LibrarySyncDetails, DatabaseError> {
        if !self.connection.owner_sync_initialized(|row| row.get(0))? {
            return Ok(LibrarySyncDetails::default());
        }
        let mut last_synced_at: Option<String> = None;
        self.connection.shared_owner_last_successful_sync_at(|row| {
            last_synced_at = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let last_synced_at = last_synced_at.map(|value| value.parse().map_err(DatabaseError::operation)).transpose()?;
        Ok(LibrarySyncDetails { has_unsynced_changes: self.connection.owner_has_unsynced_changes(|row| row.get(0))?, last_synced_at })
    }

    pub fn set_asset_storage_policy(&self, enabled: bool, reset_cloud_presence: bool) -> Result<(), DatabaseError> {
        let enabled = if enabled { "1" } else { "0" };
        if reset_cloud_presence {
            let transaction = self.connection.unchecked_transaction()?;
            transaction.owner_reset_remote_books()?;
            transaction.owner_reset_rejected_book_uploads()?;
            requeue_cloud_storage(&transaction)?;
            transaction.owner_refresh_book_uploads()?;
            transaction.owner_cloud_storage_policy(enabled)?;
            transaction.commit()?;
            return Ok(());
        }
        self.connection.owner_cloud_storage_policy(enabled)?;
        Ok(())
    }

    pub fn needs_scan(&self) -> Result<bool, DatabaseError> {
        let mut record: Option<(String, bool, bool)> = None;
        self.connection.owner_operation_record(|row| {
            let kind: Option<String> = row.get(0)?;
            let Some(kind) = kind else { return Ok(()) };
            let scan_complete: Option<i64> = row.get(1)?;
            let completed: Option<i64> = row.get(2)?;
            record = Some((kind, scan_complete.unwrap_or(0) != 0, completed.unwrap_or(0) != 0));
            Ok(())
        })?;
        Ok(record.is_some_and(|(kind, scan_complete, completed)| !completed && kind == "Import" && !scan_complete))
    }

    pub fn book_format(&self, hash: &sync_common::ContentHash) -> Result<Option<book_model::BookFormat>, DatabaseError> {
        let mut format = None;
        self.connection.owner_book_format(hash.as_str(), |row| {
            format = Some(row.get::<_, Option<i64>>(0)?);
            Ok(())
        })?;
        format.map(|format| format.and_then(book_model::BookFormat::from_code).ok_or_else(|| DatabaseError::message(format!("book {hash} has no known format")))).transpose()
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_path_buf();
        let connection = device_sqlite::open(&path, device_sqlite::OpenOptions::new()).map_err(DatabaseError::operation)?;
        let metadata_batch_flag = configure_metadata_batch_flag(&connection)?;
        sqlite::initialize(&connection, configure_library_connection).map_err(DatabaseError::operation)?;
        Ok(Self { connection, path, metadata_batch_flag })
    }

    /// Filesystem location of this library, so detached transfer drivers can
    /// open their own commit connection to the same file. SQLite serializes
    /// the writers; every commit replays guarded, idempotent writes.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Opens a configured, independent connection for external test fixtures.
/// This feature is enabled only by test dependencies; production callers use
/// typed database operations through `Database`.
#[cfg(feature = "fixture-sql")]
pub fn open_fixture_connection(path: impl AsRef<Path>) -> Result<rusqlite::Connection, DatabaseError> {
    let database = Database::open(path)?;
    Ok(database.connection)
}

/// Connection-local fixture setup for tests that exercise an attached schema.
#[cfg(feature = "fixture-sql")]
pub fn execute_owner_fixture_sql(database: &Database, sql: &str) -> Result<(), DatabaseError> {
    database.connection.execute_batch(sql)?;
    Ok(())
}

/// Install connection-local functions and hooks for interruption fixtures.
#[cfg(feature = "fixture-sql")]
pub fn configure_fixture_connection(database: &Database, configure: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<(), DatabaseError> {
    configure(&database.connection)?;
    Ok(())
}

/// Bounds a fixture query so a broken recursive projection fails promptly.
#[cfg(feature = "fixture-sql")]
pub fn set_fixture_progress_limit(database: &Database, limit: usize) -> Result<(), DatabaseError> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let steps = AtomicUsize::new(0);
    database.connection.progress_handler(1000, Some(move || steps.fetch_add(1, Ordering::Relaxed) > limit))?;
    Ok(())
}

include_sql!("src/sql/owner.sql");

mod annotations;
mod assets;
mod books;
mod browse;
mod enrichment;
mod import;
mod native;
mod placements;
mod sync;
pub mod sync_status;

pub(crate) use annotations::*;
pub use assets::*;
pub use browse::*;
pub use enrichment::*;
pub use import::*;
pub use native::*;
pub use placements::*;
pub use sync::*;

impl From<DatabaseError> for client_runtime::BackendError {
    fn from(error: DatabaseError) -> Self {
        Self::operation(error)
    }
}

#[cfg(test)]
mod runtime_error_tests {
    #[test]
    fn worker_roundtrip_preserves_sqlite_codes_through_database_wrapper() {
        use client_runtime::{
            BackendError, ErrorCause,
            wire::{decode_worker_message, encode_worker_message},
        };
        let source = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(2067), None);
        let error = BackendError::from(super::DatabaseError::from(source));
        let remote: BackendError = decode_worker_message(&encode_worker_message(&error).unwrap()).unwrap();
        assert!(remote.report().causes.contains(&ErrorCause::Sqlite { extended_code: 2067 }));
    }
}

#[cfg(test)]
extern crate self as library_database;

// These files need the owner's configured connection for in-memory fixtures.
#[cfg(test)]
#[path = "../tests/folder_facets_validation.rs"]
mod folder_facets_validation_tests;
#[cfg(test)]
#[path = "../tests/local_evict_guard.rs"]
mod local_evict_guard_tests;
#[cfg(test)]
#[path = "../tests/regressions.rs"]
mod regression_tests;

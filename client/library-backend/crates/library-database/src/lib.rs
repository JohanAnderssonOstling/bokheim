//! The sole owner of a concrete library's SQLite connection.
//!
//! Callers may borrow the connection for inlined library operations.
//! This crate retains connection ownership and chooses the connection policy.

use std::error::Error;
use std::sync::Arc;
use crate::annotations::{IndexSql as AnnotationsIndexSql, TablesSql as AnnotationsTablesSql};
use crate::assets::TablesSql as AssetsTablesSql;
use crate::enrichment::{IndexSql as EnrichmentIndexSql, TablesSql as EnrichmentTablesSql};
use crate::native::{IndexSql as NativeIndexSql, TablesSql as NativeTablesSql};
use crate::placements::{IndexSql as PlacementsIndexSql, TablesSql as PlacementsTablesSql};
use crate::shared_sql::SharedSql;
use crate::sync::{IndexSql as SyncIndexSql, TablesSql as SyncTablesSql};
#[cfg(test)]
use sync_common::ContentHash;
#[macro_use]
mod query_methods;
pub(crate) mod contributors;
pub(crate) mod metadata;
pub(crate) mod purge;
pub mod shared_sql;
mod taxonomy_update;
#[cfg(test)]
mod test_support;
mod timing;
mod transactions;

pub const LIBRARY_SCHEMA_VERSION: i64 = 23;

pub use client_platform_runtime::executor;
pub use client_platform_runtime::storage_queue;
pub use rusqlite;
pub use transactions::commit;

#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_native::sqlite;
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::sqlite;

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
    Operation(Arc<dyn Error + Send + Sync>),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => formatter.write_str(message),
            Self::Operation(error) => error.fmt(formatter),
        }
    }
}

impl Error for DatabaseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Operation(error) => Some(error.as_ref()),
            Self::Message(_) => None,
        }
    }
}

impl DatabaseError {
    pub fn operation(error: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Operation(Arc::from(error.into()))
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
    use rusqlite::functions::FunctionFlags;
    connection.create_scalar_function("normalize_search_text", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(book_model::normalize_search_text(&context.get::<String>(0)?)))?;
    connection.create_scalar_function("portable_name_key", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(library_replica::portable_name_key(&context.get::<String>(0)?)))?;
    {
        'configure_library_connection: {
            let mut attached = false;
            (match connection.taxonomy_attached(|row| {
                attached = row.get(0)?;
                Ok(())
            }) {
                Ok(value) => value,
                Err(error) => break 'configure_library_connection Err(error.into()),
            });
            if attached {
                break 'configure_library_connection Ok(());
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = subject_projection::installed::active_path() {
                let path = match path.to_str().ok_or_else(|| DatabaseError::message("taxonomy path is not UTF-8")) {
                    Ok(value) => value,
                    Err(error) => break 'configure_library_connection Err(error.into()),
                };
                let encoded = path.bytes().map(|b| if b.is_ascii_alphanumeric() || b"/-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
                let uri = format!("file:{encoded}?mode=ro&immutable=1");
                (match connection.taxonomy_attach(&uri) {
                    Ok(value) => value,
                    Err(error) => break 'configure_library_connection Err(error.into()),
                });
                break 'configure_library_connection Ok(());
            }
            (match connection.taxonomy_attach(":memory:") {
                Ok(value) => value,
                Err(error) => break 'configure_library_connection Err(error.into()),
            });
            // SAFETY: this non-owning wrapper is confined to connection
            // configuration; no statement is active and it cannot outlive or
            // close the original handle. SQLite reads the static bundled bytes
            // directly without copying the tree.
            let mut borrowed = unsafe {
                match rusqlite::Connection::from_handle(connection.handle()) {
                    Ok(value) => value,
                    Err(error) => break 'configure_library_connection Err(error.into()),
                }
            };
            (match borrowed.deserialize_bytes("curated", subject_projection::DEFAULT_UNIFIED_TAXONOMY_SQLITE) {
                Ok(value) => value,
                Err(error) => break 'configure_library_connection Err(error.into()),
            });
            Ok::<_, DatabaseError>(())
        }
    }
    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

pub struct Database {
    pub connection: rusqlite::Connection,
    path: PathBuf,
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
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
            transaction.schema_placements_views()?;
            transaction.schema_assets_views()?;
            transaction.execute_batch(include_str!("enrichment/sql/migrate_policy.sql"))?;
            let raw: String = transaction.query_row("SELECT replica_id FROM sync_metadata WHERE singleton=1", [], |row| row.get(0))?;
            let replica_id = raw.parse().map_err(DatabaseError::operation)?;
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
        transaction.schema_placements_views()?;
        transaction.schema_assets_tables()?;
        transaction.schema_assets_views()?;
        transaction.schema_enrichment_tables()?;
        transaction.schema_annotations_tables()?;

        // Initial singleton rows and the root directory belong to a new library.
        transaction.execute_batch(
            "INSERT OR IGNORE INTO sync_clock VALUES(1,0);
            INSERT OR IGNORE INTO sync_metadata(singleton) VALUES(1);
            INSERT OR IGNORE INTO library_local_state(singleton,enrichment_policy) VALUES(1,'books_v3');
            INSERT INTO dir(id,parent_id,name,canonical_parent_id,canonical_name,lifecycle,deleted_at)
            VALUES('00000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000','','00000000-0000-0000-0000-000000000000','',0,NULL)
            ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id,name=excluded.name,deleted_at=NULL;",
        )?;

        transaction.schema_sync_index()?;
        transaction.schema_native_index()?;
        transaction.schema_placements_index()?;
        transaction.schema_enrichment_index()?;
        transaction.schema_annotations_index()?;
        let replica_id = sync_common::ReplicaId::new_v4();
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
        self.write(|transaction| {
            if reset_cloud_presence {
                transaction.owner_reset_remote_books()?;
                transaction.owner_reset_rejected_book_uploads()?;
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
                transaction.owner_refresh_book_uploads()?;
            }
            transaction.owner_cloud_storage_policy(enabled)?;
            Ok(())
        })
    }

    pub fn needs_scan(&self) -> Result<bool, DatabaseError> {
        Ok(self.connection.owner_needs_scan(|row| row.get(0))?)
    }

    pub fn book_format(&self, hash: &sync_common::ContentHash) -> Result<Option<book_model::BookFormat>, DatabaseError> {
        let mut format = None;
        self.connection.shared_book_format(hash.as_str(), |row| {
            format = Some(row.get::<_, Option<i64>>(0)?);
            Ok(())
        })?;
        format.map(|format| format.and_then(book_model::BookFormat::from_code).ok_or_else(|| DatabaseError::message(format!("book {hash} has no known format")))).transpose()
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_path_buf();
        let connection = device_sqlite::open(&path, device_sqlite::OpenOptions::new()).map_err(DatabaseError::operation)?;
        sqlite::initialize(&connection, configure_library_connection).map_err(DatabaseError::operation)?;
        Ok(Self { connection, path })
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

include_sql!("src/sql/owner.sql");

mod annotations;
pub mod assets;
pub(crate) mod books;
pub mod browse;
pub mod enrichment;
mod import;
pub mod native;
pub(crate) mod placements;
mod projection;
pub(crate) mod sync;
pub mod sync_status;

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

pub use books::audiobook::{AudiobookSql, chapters_from_navigation};
pub use books::toc::{TocSql, entry_title};

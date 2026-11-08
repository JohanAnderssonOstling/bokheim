//! The sole owner of a concrete library's SQLite connection.
//!
//! Callers may borrow the connection for inlined library operations.
//! This crate retains connection ownership and chooses the connection policy.

#[macro_use]
#[path = "client/macros.rs"]
mod client_macros;
mod client;
pub use client::DatabaseClient;

use crate::annotations::{IndexSql as AnnotationsIndexSql, TablesSql as AnnotationsTablesSql};
use crate::assets::TablesSql as AssetsTablesSql;
use crate::native::{IndexSql as NativeIndexSql, TablesSql as NativeTablesSql};
use crate::placements::{IndexSql as PlacementsIndexSql, TablesSql as PlacementsTablesSql};
use crate::query_methods::{first_column, query_rows};
use crate::shared_sql::SharedSql;
use crate::sync::{IndexSql as SyncIndexSql, TablesSql as SyncTablesSql};
use crate::sync_status::{SyncIssueDetail, SyncIssueSummary};
use book_model::BookFormat;
use std::cell::RefCell;
use std::error::Error;
use std::sync::Arc;
use sync_common::storage::LibraryStorage;
use sync_common::BookId;
#[macro_use]
mod query_methods;
mod annotation_export;
mod content_identity;
pub(crate) mod contributors;
pub(crate) mod purge;
pub mod shared_sql;
mod taxonomy_update;
mod transactions;
pub mod transfer;

pub const LIBRARY_SCHEMA_VERSION: i64 = 54;

pub use client_platform_runtime::executor;
pub use client_platform_runtime::storage_queue;
pub use contributors::ContributorNameMatcher;
pub use rusqlite;
pub use transactions::commit;

#[cfg(target_arch = "wasm32")]
pub use client_platform_web::sqlite;
#[cfg(not(target_arch = "wasm32"))]
pub use device_sqlite as sqlite;

// Connection ownership for one concrete library session.
use std::fmt;
use std::path::{Path, PathBuf};

use include_sqlite_sql::include_sql;

/// Durable settings/status data captured in one database read snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibrarySyncDetails {
    pub has_unsynced_changes: bool,
    pub last_synced_at: Option<u64>,
    pub issues: SyncIssueSummary,
    pub issue_details: Vec<SyncIssueDetail>,
    pub storage: LibraryStorage,
    pub has_download_requests: bool,
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
    text_search::register(connection)?;
    description_storage::register(connection)?;
    connection.busy_timeout(std::time::Duration::from_secs(30))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    use rusqlite::functions::FunctionFlags;
    connection.create_scalar_function("normalize_search_text", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(book_model::normalize_search_text(&context.get::<String>(0)?)))?;
    connection.create_scalar_function("portable_name_key", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| Ok(library_replica::portable_name_key(&context.get::<String>(0)?)))?;
    attach_taxonomy(connection).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.pragma_update(Some("main"), "cache_size", -subject_projection::installed::PAGE_CACHE_KIB)?;
    Ok(())
}

fn attach_taxonomy(connection: &rusqlite::Connection) -> Result<(), DatabaseError> {
    let mut attached = false;
    connection.taxonomy_attached(|row| {
        attached = row.get(0)?;
        Ok(())
    })?;
    if !attached {
        let path = subject_projection::installed::path().map_err(DatabaseError::message)?;
        let uri = subject_projection::installed::read_only_uri(&path).map_err(DatabaseError::message)?;
        connection.taxonomy_attach(&uri)?;
    }
    // The immutable taxonomy is shared by all library and catalog connections.
    // Map its pages instead of retaining a private heap copy in each page cache.
    connection.pragma_update(Some("curated"), "cache_size", -subject_projection::installed::PAGE_CACHE_KIB)?;
    #[cfg(not(target_arch = "wasm32"))]
    connection.pragma_update(Some("curated"), "mmap_size", subject_projection::installed::READ_ONLY_MMAP_BYTES)?;
    Ok(())
}

pub struct Database {
    pub(crate) connection: rusqlite::Connection,
    path: PathBuf,
    subject_trees: RefCell<browse::subjects::tree::SubjectTrees>,
    browse_results: RefCell<browse::result::BrowseResults>,
    browse_facets: RefCell<browse::facets::Facets>,
    text_index_usage: RefCell<Option<(i64, u64)>>,
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub fn initialize_library(&self) -> Result<sync_common::ReplicaId, DatabaseError> {
        self.initialize_library_with_scope(None, true)
    }

    pub fn initialize_library_for(&self, library: sync_common::LibraryId) -> Result<sync_common::ReplicaId, DatabaseError> {
        self.initialize_library_with_scope(Some(library), true)
    }

    /// Open a serving session without recomputing its subject projection.
    /// Its maintenance worker refreshes assignments after the UI is ready.
    pub fn initialize_library_for_deferred_taxonomy(&self, library: sync_common::LibraryId) -> Result<sync_common::ReplicaId, DatabaseError> {
        self.initialize_library_with_scope(Some(library), false)
    }

    fn initialize_library_with_scope(&self, library: Option<sync_common::LibraryId>, refresh_taxonomy: bool) -> Result<sync_common::ReplicaId, DatabaseError> {
        // Database::open configured this connection before initialization.
        // Each committed schema and replica identity becomes visible with its
        // matching version; failed transactions roll back.
        let transaction = self.connection.unchecked_transaction()?;
        const VERSION: i64 = LIBRARY_SCHEMA_VERSION;
        let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        // Schema 53 has the same persisted records. Its extra remote-name
        // column is unused; keep it while installing the new local audio cache.
        if version == VERSION || version == 53 {
            transaction.execute_batch(include_str!("audiobook_content.sql"))?;
            transaction.schema_placements_views()?;
            transaction.schema_assets_views()?;
            transaction.initialization_reset_enrichment_policy()?;
            let raw: String = transaction.initialization_replica_id(|row| row.get(0))?;
            let replica_id = raw.parse().map_err(DatabaseError::operation)?;
            content_identity::initialize_scope(&transaction, library, replica_id)?;
            if version != VERSION {
                transaction.pragma_update(None, "user_version", VERSION)?;
            }
            transaction.commit()?;
            if refresh_taxonomy {
                self.refresh_taxonomy_assignments()?;
            }
            return Ok(replica_id);
        }
        if version != 0 {
            return Err(DatabaseError::message(format!("unsupported library schema version {version}; expected a new library or version {VERSION}")));
        }
        // WAL persists in the database header. Establish it only for a new
        // local database, including libraries populated by import or sync.
        // SQLite requires changing journal mode outside a transaction.
        transaction.rollback()?;
        self.connection.pragma_update(Some("main"), "journal_mode", "WAL")?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.schema_sync_tables()?;
        transaction.execute_batch(include_str!("sync/sql/schema/contributor_names.sql"))?;
        transaction.execute_batch(include_str!("sync/sql/schema/mutation_issues.sql"))?;
        transaction.schema_native_tables()?;
        transaction.schema_placements_tables()?;
        transaction.schema_placements_views()?;
        transaction.schema_assets_tables()?;
        transaction.execute_batch(include_str!("content_identity.sql"))?;
        transaction.execute_batch(include_str!("audiobook_content.sql"))?;
        transaction.execute_batch(include_str!("storage.sql"))?;
        transaction.schema_assets_views()?;
        transaction.schema_annotations_tables()?;
        transaction.execute_batch(include_str!("notebook/schema.sql"))?;
        // Initial singleton rows and the root directory belong to a new library.
        transaction.initialization_seed_library()?;

        transaction.schema_sync_index()?;
        transaction.schema_native_index()?;
        transaction.schema_placements_index()?;
        transaction.execute_batch(include_str!("placements/sql/schema/path_indexes.sql"))?;
        transaction.schema_annotations_index()?;
        browse::revision::install(&transaction)?;
        let replica_id = sync_common::ReplicaId::new_v4();
        transaction.owner_set_replica_id(&replica_id.to_string())?;
        content_identity::initialize_scope(&transaction, library, replica_id)?;
        transaction.pragma_update(None, "user_version", VERSION)?;
        transaction.commit()?;
        if refresh_taxonomy {
            self.refresh_taxonomy_assignments()?;
        }
        Ok(replica_id)
    }

    /// Fails unless SQLite's quick check passes and every foreign key holds,
    /// then moves the write-ahead log into the database file.
    pub fn check_migrated(&self) -> Result<(), DatabaseError> {
        let integrity: String = self.connection.initialization_quick_check(|row| row.get(0))?;
        if integrity != "ok" {
            return Err(DatabaseError::message(integrity));
        }
        let violations: i64 = self.connection.initialization_foreign_key_violations(|row| row.get(0))?;
        if violations != 0 {
            return Err(DatabaseError::message("migrated library has invalid relationships"));
        }
        self.connection.initialization_checkpoint()?;
        Ok(())
    }

    pub fn stored_sync_status(&self) -> Result<LibrarySyncDetails, DatabaseError> {
        self.with_browse_snapshot(|| {
            if !self.connection.owner_sync_initialized(|row| row.get(0))? {
                return Ok(LibrarySyncDetails::default());
            }
            let last_synced_at = first_column::<Option<String>>(|read| self.connection.shared_owner_last_successful_sync_at(read))?.flatten();
            let last_synced_at = last_synced_at.map(|value| value.parse().map_err(DatabaseError::operation)).transpose()?;
            Ok(LibrarySyncDetails {
                has_unsynced_changes: self.connection.owner_has_unsynced_changes(|row| row.get(0))?,
                last_synced_at,
                issues: self.sync_issue_summary()?,
                issue_details: self.sync_issue_details()?,
                storage: self.storage_policy()?,
                has_download_requests: !self.book_download_requests(-1, None, 1)?.is_empty(),
            })
        })
    }

    pub fn set_asset_storage_policy(&self, enabled: bool, reset_cloud_presence: bool) -> Result<(), DatabaseError> {
        let storage = if enabled { LibraryStorage::Bokheim } else { LibraryStorage::Local };
        self.set_storage_policy(&storage, reset_cloud_presence)
    }

    pub fn set_storage_policy(&self, storage: &LibraryStorage, reset_cloud_presence: bool) -> Result<(), DatabaseError> {
        use rusqlite::OptionalExtension;
        let enabled = if storage.uses_bokheim_assets() { "1" } else { "0" };
        let json = serde_json::to_string(storage).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            if reset_cloud_presence {
                transaction.owner_reset_remote_books()?;
                let candidates = query_rows(|read| transaction.owner_cloud_storage_candidates(read), |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))?;
                let mut folders = placements::FolderPaths::for_directories(transaction, candidates.iter().map(|(_, id, _)| id.as_str()))?;
                let mut queued = std::collections::HashSet::new();
                for (book_id, dir_id, size_bytes) in candidates {
                    if !queued.contains(&book_id) && folders.folder(&dir_id)?.is_some() {
                        transaction.owner_queue_book_upload(&book_id, size_bytes)?;
                        let checksum = content_identity::expected_hash(transaction, book_id)?
                            .or(transaction.query_row("SELECT byte_hash FROM local_file_content WHERE book_id=?1 AND dir_id=?2", rusqlite::params![book_id, dir_id], |row| row.get::<_, content_address::ContentHash>(0)).optional()?)
                            .or_else(|| book_id.content_hash());
                        transaction.execute("UPDATE book_transfer SET upload_checksum=?2 WHERE content_hash=?1 AND upload_id IS NOT NULL", rusqlite::params![book_id, checksum])?;
                        queued.insert(book_id);
                    }
                }
                transaction.owner_refresh_book_uploads()?;
            }
            transaction.owner_cloud_storage_policy(enabled)?;
            transaction.execute("INSERT INTO library_storage(singleton,configuration) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET configuration=excluded.configuration", [&json])?;
            Ok(())
        })
    }

    pub fn book_format(&self, hash: &BookId) -> Result<Option<BookFormat>, DatabaseError> {
        let format = first_column::<Option<i64>>(|read| self.connection.shared_book_format(hash, read))?;
        format.map(|format| format.and_then(BookFormat::from_code).ok_or_else(|| DatabaseError::message("book has no known format"))).transpose()
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_path_buf();
        let connection = device_sqlite::open(&path, device_sqlite::OpenOptions::new()).map_err(DatabaseError::operation)?;
        configure_library_connection(&connection).map_err(DatabaseError::operation)?;
        Ok(Self { connection, path, subject_trees: Default::default(), browse_results: Default::default(), browse_facets: Default::default(), text_index_usage: Default::default() })
    }

    /// Filesystem location of this library, so detached transfer drivers can
    /// open their own commit connection to the same file. SQLite serializes
    /// the writers; every commit replays guarded, idempotent writes.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

include_sql!("src/sql/owner.sql");

mod annotations;
pub mod assets;
pub(crate) mod books;
pub mod browse;
pub mod catalog;
pub mod enrichment;
mod import;
pub mod native;
mod notebook;
pub(crate) mod placements;
mod projection;
mod storage;
mod stored_document;
pub(crate) mod sync;
pub mod sync_status;

pub use assets::*;
pub use browse::*;
pub use enrichment::*;
pub use import::*;
pub use native::*;
pub use placements::*;
pub use sync::*;

impl From<DatabaseError> for client_protocol::BackendError {
    fn from(error: DatabaseError) -> Self {
        Self::operation(error)
    }
}

#[cfg(test)]
extern crate self as library_database;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod bundled_taxonomy_tests {
    use super::configure_library_connection;
    use rusqlite::Connection;
    use std::path::Path;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn connections_share_a_read_only_disk_taxonomy_with_bounded_caches() {
        let barrier = Barrier::new(8);
        thread::scope(|scope| {
            for _ in 0..8 {
                let barrier = &barrier;
                scope.spawn(move || {
                    let connection = Connection::open_in_memory().unwrap();
                    barrier.wait();
                    configure_library_connection(&connection).unwrap();
                    let expected = subject_projection::bundled::path().unwrap().canonicalize().unwrap();
                    let path: String = connection.query_row("SELECT file FROM pragma_database_list WHERE name='curated'", [], |row| row.get(0)).unwrap();
                    assert_eq!(Path::new(&path).canonicalize().unwrap(), expected);
                    let count: i64 = connection.query_row("SELECT count(*) FROM curated.concept", [], |row| row.get(0)).unwrap();
                    assert!(count > 0);
                    assert!(connection.execute("DELETE FROM curated.concept", []).is_err());
                    let cache: i64 = connection.pragma_query_value(Some("curated"), "cache_size", |row| row.get(0)).unwrap();
                    assert_eq!(cache, -512);
                });
            }
        });
    }
}

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

include_sqlite_sql::include_sql!("src/sql/initialization.sql");

#[cfg(test)]
mod sql_fixtures {
    use crate::SqlParam;
    include_sqlite_sql::include_sql!("tests/sql/fixtures.sql");
}

pub use query_methods::SqlParam;

pub mod text_search;

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn initialize_library() -> sync_common::ReplicaId;
    arguments () = ();
    execute database, arguments => database.initialize_library();

    fn initialize_library_for(library: sync_common::LibraryId) -> sync_common::ReplicaId;
    arguments (sync_common::LibraryId,) = (library,);
    execute database, arguments => database.initialize_library_for(arguments.0);

    fn initialize_library_for_deferred_taxonomy(library: sync_common::LibraryId) -> sync_common::ReplicaId;
    arguments (sync_common::LibraryId,) = (library,);
    execute database, arguments => database.initialize_library_for_deferred_taxonomy(arguments.0);

    fn check_migrated() -> ();
    arguments () = ();
    execute database, arguments => database.check_migrated();

    fn stored_sync_status() -> LibrarySyncDetails;
    arguments () = ();
    execute database, arguments => database.stored_sync_status();

    fn set_asset_storage_policy(enabled: bool, reset_cloud_presence: bool) -> ();
    arguments (bool, bool,) = (enabled, reset_cloud_presence,);
    execute database, arguments => database.set_asset_storage_policy(arguments.0, arguments.1);

    fn set_storage_policy(storage: &LibraryStorage, reset_cloud_presence: bool) -> ();
    arguments (LibraryStorage, bool,) = (storage.clone(), reset_cloud_presence,);
    execute database, arguments => database.set_storage_policy(&arguments.0, arguments.1);

    fn book_format(hash: &BookId) -> Option<BookFormat>;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.book_format(&arguments.0);


}

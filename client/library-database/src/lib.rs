//! The sole owner of a concrete library's SQLite connection.
//!
//! Other crates use typed database operations; they never own a SQLite
//! connection or select a connection policy.

pub use client_platform_runtime::executor;
pub use client_platform_runtime::storage_queue;
pub use rusqlite;

#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_native::sqlite;
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::sqlite;

pub use library_replica::{BookPlacement, BookUploadIntent, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError};
use sync_common::ContentHash;

#[derive(Debug)]
pub struct AssetCandidate {
    pub work_id: i64, pub content_hash: ContentHash, pub live: bool, pub path: Option<RelativeBookPath>,
    pub thumbnail_upload_allowed: bool, pub thumbnail_remote: bool,
    pub thumbnail_pending: bool, pub requested: bool, pub rejected: bool, pub file_work_pending: bool,
}

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
pub struct DatabaseError(String);

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DatabaseError {}

impl DatabaseError {
    pub fn operation(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl From<rusqlite::Error> for DatabaseError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error.to_string())
    }
}

/// Connection policy owned by the database crate: busy handler, pragmas,
/// sync functions, and the read-only taxonomy attach. Callers open a path
/// and get a configured connection; no external setup function is needed.
fn configure_library_connection(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    configure_metadata_batch_flag(connection)?;
    connection.busy_timeout(std::time::Duration::from_secs(30))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let journal_mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    crate::sync::register_sync_functions(connection)?;
    attach_curated_taxonomy_to(connection).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

/// Scalar flag coalescing metadata payloads without a persistent batch flag.
fn configure_metadata_batch_flag(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let active = AtomicBool::new(false);
    connection.create_scalar_function(
        "metadata_batch_flag",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |ctx| {
            Ok(match ctx.get::<Option<bool>>(0)? {
                Some(value) => active.swap(value, Ordering::Relaxed),
                None => active.load(Ordering::Relaxed),
            })
        },
    )
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
    connection.taxonomy_attach(":memory:")?;
    // SAFETY: this non-owning wrapper is confined to connection
    // configuration; no statement is active and it cannot outlive or
    // close the original handle. SQLite reads the static bundled bytes
    // directly without copying the tree.
    let mut borrowed = unsafe { rusqlite::Connection::from_handle(connection.handle())? };
    borrowed.deserialize_bytes("curated", subject_projection::DEFAULT_UNIFIED_TAXONOMY_SQLITE)?;
    Ok(())
}

pub struct Database {
    pub(crate) connection: rusqlite::Connection,
    path: PathBuf,
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

impl std::ops::Deref for Database {
    type Target = rusqlite::Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl Database {
    pub fn initialize_library(&self) -> Result<sync_common::ReplicaId, DatabaseError> {
        crate::sync::register_sync_functions(&self.connection)?;
        self.connection.busy_timeout(std::time::Duration::from_secs(30))?;
        self.connection.pragma_update(None, "foreign_keys", "ON")?;
        self.attach_curated_taxonomy()?;
        let journal_mode: String = self.connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        if !journal_mode.eq_ignore_ascii_case("wal") { self.connection.pragma_update(None, "journal_mode", "WAL")?; }
        const VERSION: i64 = 57;
        let version: i64 = self.connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == VERSION {
            let mut raw: Option<String> = None;
            self.connection.owner_replica_id(|row| { raw = row.get::<_, Option<String>>(0)?; Ok(()) })?;
            return raw.ok_or_else(|| DatabaseError::operation("library has no replica id"))?.parse().map_err(DatabaseError::operation);
        }
        if version != 0 { return Err(DatabaseError::operation(format!("unsupported library schema version {version}; expected {VERSION}"))); }
        self.connection.schema_authors()?;
        self.connection.schema_books()?;
        self.connection.schema_descriptions()?;
        self.connection.schema_jobs()?;
        self.connection.schema_external_metadata()?;
        self.connection.schema_placements()?;
        self.connection.schema_browse()?;
        self.connection.schema_sync()?;
        self.connection.schema_marks()?;
        self.connection.schema_assets()?;
        self.connection.schema_contents()?;
        self.connection.schema_views()?;
        self.connection.schema_subjects()?;
        self.connection.schema_sync_triggers()?;
        self.connection.schema_asset_triggers()?;
        self.connection.schema_metadata_batch()?;
        self.connection.schema_description_triggers()?;
        let replica_id = sync_common::ReplicaId::new_v4();
        self.connection.owner_set_change_origin("local")?;
        self.connection.owner_set_replica_id(&replica_id.to_string())?;
        self.connection.pragma_update(None, "user_version", VERSION)?;
        Ok(replica_id)
    }

    /// Attaches the read-only bundled taxonomy used by subject browsing.
    /// Attach state belongs to the connection, not the library, so this runs
    /// on every open — including existing libraries that skip schema setup.
    fn attach_curated_taxonomy(&self) -> Result<(), DatabaseError> {
        attach_curated_taxonomy_to(&self.connection)
    }

    pub fn set_remote_library_name(&self, name: &str) -> Result<(), DatabaseError> {
        self.connection.owner_set_remote_library_name(name)?;
        Ok(())
    }

    pub fn sync_remote_library_name(&self) -> Result<Option<String>, DatabaseError> {
        let mut name = None;
        self.connection.owner_remote_library_name(|row| {
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
        self.connection.owner_server_library_created(|row| {
            created = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        if created.as_deref() == Some(identity) {
            return Ok(None);
        }
        let mut name: Option<String> = None;
        self.connection.owner_remote_library_name(|row| {
            name = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let name = name
            .ok_or_else(|| DatabaseError::operation("library has no remote name"))?;
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
        self.connection.owner_last_successful_sync_at(|row| {
            last_synced_at = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let last_synced_at = last_synced_at
            .map(|value| value.parse().map_err(DatabaseError::operation))
            .transpose()?;
        Ok(LibrarySyncDetails {
            has_unsynced_changes: self.connection.owner_has_unsynced_changes(|row| row.get(0))?,
            last_synced_at,
        })
    }

    pub fn set_asset_storage_policy(&self, enabled: bool, reset_cloud_presence: bool) -> Result<(), DatabaseError> {
        let enabled = if enabled { "1" } else { "0" };
        if reset_cloud_presence {
            let transaction = self.connection.unchecked_transaction()?;
            transaction.owner_reset_remote_books()?;
            transaction.owner_reset_rejected_book_uploads()?;
            transaction.owner_requeue_cloud_storage()?;
            transaction.owner_refresh_book_uploads()?;
            transaction.owner_refresh_asset_work()?;
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
            format = Some(row.get::<_, String>(0)?);
            Ok(())
        })?;
        format
            .map(|format| book_model::BookFormat::from_extension(&format).ok_or_else(|| DatabaseError::operation(format!("book {hash} has no known format"))))
            .transpose()
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_path_buf();
        let connection = device_sqlite::open(&path, device_sqlite::OpenOptions::new()).map_err(|error| DatabaseError(error.to_string()))?;
        sqlite::initialize(&connection, configure_library_connection).map_err(|error| DatabaseError(error.to_string()))?;
        Ok(Self { connection, path })
    }

    /// Filesystem location of this library, so detached transfer drivers can
    /// open their own commit connection to the same file. SQLite serializes
    /// the writers; every commit replays guarded, idempotent writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

}


include_sql!("src/owner.sql");

mod sync;
mod import;
mod assets;
mod placements;
mod browse;
mod enrichment;
mod native;

pub use sync::*;
pub use import::*;
pub use assets::*;
pub use placements::*;
pub use browse::*;
pub use enrichment::*;
pub use native::*;

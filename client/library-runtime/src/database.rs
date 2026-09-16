//! SQLite ownership for one concrete library session.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::executor::{BackendOutput, BackendSend};
use crate::{sqlite, storage_queue};

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

/// Wake hints only; workers still read their durable queues before doing work.
#[derive(Clone, Copy, Debug, Default)]
pub struct DatabaseWrites {
    pub metadata: bool,
    pub assets: bool,
}

impl DatabaseWrites {
    fn classify(action: rusqlite::hooks::Action, table: &str) -> u8 {
        let wake = client_library::file_work::write_notifications(action, table);
        u8::from(wake.metadata) | (u8::from(wake.assets) << 1)
    }
}

struct Connections {
    retired: AtomicBool,
    path: PathBuf,
    initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>,
    idle: Mutex<Vec<rusqlite::Connection>>,
    single_owner: bool,
    write_observer: Mutex<Option<Arc<dyn Fn(DatabaseWrites) + Send + Sync>>>,
    remote_page_observer: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Clone)]
pub struct Database(Arc<Connections>);

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub fn initialize_library(&self) -> Result<sync_common::ReplicaId, DatabaseError> {
        let connection = self.get()?;
        client_library::schema::init_db(&*connection).map_err(DatabaseError::operation)
    }

    pub fn set_remote_library_name(&self, name: &str) -> Result<(), DatabaseError> {
        let connection = self.get()?;
        client_library::sync_state::set_sync_metadata(&*connection, "remote_library_name", name).map_err(DatabaseError::operation)
    }

    pub fn sync_metadata(&self, key: &str) -> Result<Option<String>, DatabaseError> {
        let connection = self.get()?;
        client_library::sync_state::get_sync_metadata(&*connection, key).map_err(DatabaseError::operation)
    }

    pub fn set_sync_metadata(&self, key: &str, value: &str) -> Result<(), DatabaseError> {
        let connection = self.get()?;
        client_library::sync_state::set_sync_metadata(&*connection, key, value).map_err(DatabaseError::operation)
    }

    pub fn stored_sync_status(&self) -> Result<(Option<String>, client_library::sync_state::LibrarySyncDetails), DatabaseError> {
        let connection = self.get()?;
        client_library::sync_state::stored_sync_status(&*connection).map_err(DatabaseError::operation)
    }

    pub fn set_asset_storage_policy(&self, enabled: bool, reset_cloud_presence: bool) -> Result<(), DatabaseError> {
        let connection = self.get()?;
        if reset_cloud_presence {
            client_library::assets::reset_cloud_presence(&connection, enabled).map_err(DatabaseError::operation)?;
        }
        client_library::assets::set_cloud_storage_policy(&connection, enabled).map_err(DatabaseError::operation)
    }

    pub fn needs_scan(&self) -> Result<bool, DatabaseError> {
        let connection = self.get()?;
        client_library::operations::needs_scan(&*connection).map_err(DatabaseError::operation)
    }

    pub fn book_format(&self, hash: &sync_common::ContentHash) -> Result<Option<library_model::BookFormat>, DatabaseError> {
        let connection = self.get()?;
        client_library::assets::get_book_format(&*connection, hash).map_err(DatabaseError::operation)
    }

    #[track_caller]
    pub fn command<T: BackendOutput>(
        &self, priority: storage_queue::Priority, operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, DatabaseError> + BackendSend + 'static,
    ) -> impl std::future::Future<Output = Result<T, DatabaseError>> {
        let database = self.clone();
        let caller = std::panic::Location::caller();
        async move {
            storage_queue::run(priority, move || {
                let mut connection = database.get()?;
                sqlite::set_caller(&connection, caller);
                let result = operation(&mut connection);
                if result.is_err() {
                    sqlite::command_failed(&connection);
                }
                result
            })
            .await
            .map_err(DatabaseError)?
        }
    }

    pub fn open(path: impl AsRef<Path>, initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<Self, DatabaseError> {
        Self::open_with_policy(path, initialize, sqlite::SINGLE_OWNER)
    }

    fn open_with_policy(path: impl AsRef<Path>, initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>, single_owner: bool) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_owned();
        let connection = device_sqlite::open(&path, device_sqlite::OpenOptions::new()).map_err(|error| DatabaseError(error.to_string()))?;
        sqlite::initialize(&connection, initialize).map_err(|error| DatabaseError(error.to_string()))?;
        Ok(Self(Arc::new(Connections { retired: AtomicBool::new(false), path, initialize, idle: Mutex::new(vec![connection]), single_owner, write_observer: Mutex::new(None), remote_page_observer: Mutex::new(None) })))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }

    pub fn open_library(locator: &str, initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<Self, DatabaseError> {
        Self::open(Path::new(locator), initialize)
    }

    pub fn observe_writes(&self, observer: impl Fn(DatabaseWrites) + Send + Sync + 'static) {
        *self.0.write_observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(observer));
    }

    pub fn observe_remote_pages(&self, observer: impl Fn() + Send + Sync + 'static) {
        *self.0.remote_page_observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(observer));
    }

    pub fn remote_page_committed(&self) {
        let observer = self.0.remote_page_observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(observer) = observer {
            observer();
        }
    }

    #[track_caller]
    pub fn get(&self) -> Result<DatabaseConnection, DatabaseError> {
        if self.0.retired.load(Ordering::Acquire) {
            return Err(DatabaseError("library storage location changed; reopen the library".into()));
        }
        let connection = self.0.idle.lock().map_err(|error| DatabaseError(error.to_string()))?.pop();
        let connection = match connection {
            Some(connection) => connection,
            None if self.0.single_owner => return Err(DatabaseError("storage connection is already borrowed; release it before awaiting or issuing another storage command".to_owned())),
            None => {
                let connection = sqlite::open_additional(&self.0.path)?;
                sqlite::initialize(&connection, self.0.initialize).map_err(|error| DatabaseError(error.to_string()))?;
                connection
            }
        };
        let writes = Arc::new(AtomicU8::new(0));
        let observed = writes.clone();
        connection.update_hook(Some(move |action, database: &str, table: &str, _row| {
            if database == "main" {
                observed.fetch_or(DatabaseWrites::classify(action, table), Ordering::Relaxed);
            }
        }))?;
        let initial_changes = connection.total_changes();
        sqlite::set_caller(&connection, std::panic::Location::caller());
        Ok(DatabaseConnection { connection: Some(connection), owner: Arc::clone(&self.0), initial_changes, writes })
    }

    pub fn retire_location(&self) {
        self.0.retired.store(true, Ordering::Release);
        self.0.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }
}

fn open_existing_library(locator: &str) -> Result<Option<rusqlite::Connection>, DatabaseError> {
    sqlite::open_existing(Path::new(locator)).map_err(DatabaseError)
}

pub fn has_pending_outbox(locator: &str) -> Result<bool, DatabaseError> {
    let Some(connection) = open_existing_library(locator)? else {
        return Ok(false);
    };
    client_library::sync_state::has_pending_outbox(&connection).map_err(Into::into)
}

pub struct DatabaseConnection {
    connection: Option<rusqlite::Connection>,
    owner: Arc<Connections>,
    initial_changes: u64,
    writes: Arc<AtomicU8>,
}

impl Deref for DatabaseConnection {
    type Target = rusqlite::Connection;

    fn deref(&self) -> &Self::Target {
        self.connection.as_ref().expect("checked-out SQLite connection is present")
    }
}

impl DerefMut for DatabaseConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection.as_mut().expect("checked-out SQLite connection is present")
    }
}

impl Drop for DatabaseConnection {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        if self.owner.retired.load(Ordering::Acquire) {
            return;
        }
        let changed = connection.total_changes() != self.initial_changes && connection.is_autocommit();
        match self.owner.idle.lock() {
            Ok(mut idle) => idle.push(connection),
            Err(poisoned) => poisoned.into_inner().push(connection),
        }
        if changed {
            let observer = self.owner.write_observer.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            if let Some(observer) = observer {
                let writes = self.writes.load(Ordering::Relaxed);
                observer(DatabaseWrites { metadata: writes & 1 != 0, assets: writes & 2 != 0 });
            }
        }
    }
}

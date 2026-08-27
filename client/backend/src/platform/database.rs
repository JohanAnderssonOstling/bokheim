//! SQLite ownership shared by every compilation target.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub(crate) struct DatabaseError(String);

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DatabaseError {}

struct Connections {
    path: PathBuf,
    initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>,
    idle: Mutex<Vec<rusqlite::Connection>>,
}

#[derive(Clone)]
pub(crate) struct Database(Arc<Connections>);

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub(crate) fn open(path: impl AsRef<Path>, initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<Self, DatabaseError> {
        let path = path.as_ref().to_owned();
        let connection = rusqlite::Connection::open(&path).map_err(|error| DatabaseError(error.to_string()))?;
        initialize(&connection).map_err(|error| DatabaseError(error.to_string()))?;
        Ok(Self(Arc::new(Connections { path, initialize, idle: Mutex::new(vec![connection]) })))
    }

    pub(crate) fn open_library(locator: &str, initialize: fn(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<Self, DatabaseError> {
        Self::open(library_database_path(locator)?, initialize)
    }

    pub(crate) fn get(&self) -> Result<DatabaseConnection, DatabaseError> {
        let connection = self.0.idle.lock().map_err(|error| DatabaseError(error.to_string()))?.pop();
        let connection = match connection {
            Some(connection) => connection,
            None => {
                let connection = rusqlite::Connection::open(&self.0.path).map_err(|error| DatabaseError(error.to_string()))?;
                (self.0.initialize)(&connection).map_err(|error| DatabaseError(error.to_string()))?;
                connection
            }
        };
        Ok(DatabaseConnection { connection: Some(connection), owner: Arc::clone(&self.0) })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn library_database_path(locator: &str) -> Result<std::path::PathBuf, DatabaseError> {
    let requested = Path::new(locator);
    let root = requested.canonicalize().map_err(|error| DatabaseError(error.to_string()))?;
    if !root.is_dir() {
        return Err(DatabaseError(format!("library root is not a directory: {}", requested.display())));
    }
    let metadata = root.join(crate::APP_HIDDEN_DIR);
    std::fs::create_dir_all(metadata.join("thumbnails")).map_err(|error| DatabaseError(error.to_string()))?;
    Ok(metadata.join("library.db"))
}

#[cfg(target_arch = "wasm32")]
fn library_database_path(locator: &str) -> Result<std::path::PathBuf, DatabaseError> {
    Ok(locator.into())
}

pub(crate) struct DatabaseConnection {
    connection: Option<rusqlite::Connection>,
    owner: Arc<Connections>,
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
        let Some(connection) = self.connection.take() else { return };
        match self.owner.idle.lock() {
            Ok(mut idle) => idle.push(connection),
            Err(poisoned) => poisoned.into_inner().push(connection),
        }
    }
}

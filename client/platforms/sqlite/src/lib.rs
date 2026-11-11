//! Device-local SQLite opening policy.
//!
//! Callers supply storage locators and database policy without knowing whether
//! SQLite is backed by native files or a browser-installed virtual filesystem.

pub mod fts5;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::open_existing_writable;

use rusqlite::Connection;
use std::fmt;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JournalMode {
    #[default]
    PlatformDefault,
    Wal,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenOptions {
    busy_timeout: Option<Duration>,
    journal_mode: JournalMode,
}

impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn busy_timeout(mut self, timeout: Duration) -> Self {
        self.busy_timeout = Some(timeout);
        self
    }

    pub fn journal_mode(mut self, journal_mode: JournalMode) -> Self {
        self.journal_mode = journal_mode;
        self
    }
}

#[derive(Debug)]
pub struct OpenError(Box<dyn std::error::Error + Send + Sync>);

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0.as_ref(), formatter)
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(self.0.as_ref()) }
}

impl From<std::io::Error> for OpenError {
    fn from(error: std::io::Error) -> Self { Self(Box::new(error)) }
}

impl From<rusqlite::Error> for OpenError {
    fn from(error: rusqlite::Error) -> Self { Self(Box::new(error)) }
}

/// Opens a device-local database through the active platform's storage
/// implementation. Native parents are created as needed; browser locators are
/// passed directly to the SQLite VFS installed by the host.
pub fn open(path: impl AsRef<Path>, options: OpenOptions) -> Result<Connection, OpenError> {
    let path = path.as_ref();
    platform::prepare(path)?;
    let connection = Connection::open(path)?;
    if let Some(timeout) = options.busy_timeout {
        connection.busy_timeout(timeout)?;
    }
    platform::configure_journal(&connection, options.journal_mode)?;
    Ok(connection)
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::JournalMode;
    use rusqlite::Connection;
    use std::path::Path;

    pub(super) fn prepare(path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    pub(super) fn configure_journal(connection: &Connection, mode: JournalMode) -> rusqlite::Result<()> {
        if mode == JournalMode::Wal {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::JournalMode;
    use rusqlite::Connection;
    use std::path::Path;

    pub(super) fn prepare(_path: &Path) -> std::io::Result<()> {
        Ok(())
    }

    pub(super) fn configure_journal(_connection: &Connection, _mode: JournalMode) -> rusqlite::Result<()> {
        Ok(())
    }
}

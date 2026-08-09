//! Reusable SQLite persistence primitives for Bokheim application adapters.
//!
//! This crate deliberately has no HTTP, Tauri, filesystem-scanner, or sync
//! transport dependencies. An adapter supplies an opaque, validated storage
//! key and its serializable state type.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCHEMA_VERSION: i64 = 1;

/// Applies the connection-level safety settings shared by native and web
/// SQLite libraries. These settings must be applied to every new connection.
pub fn configure_connection(connection: &Connection, busy_timeout: Duration) -> Result<(), rusqlite::Error> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.busy_timeout(busy_timeout)?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct UserStateDatabase {
    path: PathBuf,
}

impl UserStateDatabase {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let database = Self { path: path.into() };
        let connection = database.connect()?;
        migrate_user_state(&connection)?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read<T: DeserializeOwned + Default>(&self) -> Result<T, String> {
        let connection = self.connect()?;
        let value: Option<String> = connection.query_row("SELECT state FROM application_state WHERE id = 1", [], |row| row.get(0)).optional().map_err(storage_error)?;
        value.map_or_else(|| Ok(T::default()), |value| serde_json::from_str(&value).map_err(|error| format!("invalid SQLite application state: {error}")))
    }

    pub fn mutate<T, R>(&self, operation: impl FnOnce(&mut T) -> Result<R, String>) -> Result<R, String>
    where
        T: Serialize + DeserializeOwned + Default,
    {
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(storage_error)?;
        let value: Option<String> = transaction.query_row("SELECT state FROM application_state WHERE id = 1", [], |row| row.get(0)).optional().map_err(storage_error)?;
        let mut state = value.map_or_else(|| Ok(T::default()), |value| serde_json::from_str(&value).map_err(|error| format!("invalid SQLite application state: {error}")))?;
        let result = operation(&mut state)?;
        let state = serde_json::to_string(&state).map_err(|error| error.to_string())?;
        transaction.execute("INSERT INTO application_state (id, state, updated_at) VALUES (1, ?1, unixepoch()) ON CONFLICT(id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at", [state]).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(result)
    }

    fn connect(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            restrict_directory(parent)?;
        }
        let connection = Connection::open(&self.path).map_err(storage_error)?;
        restrict_database_file(&self.path)?;
        configure_connection(&connection, Duration::from_secs(5)).map_err(storage_error)?;
        connection.pragma_update(None, "synchronous", "NORMAL").map_err(storage_error)?;
        Ok(connection)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredSession {
    pub encrypted_session: Vec<u8>,
    pub nonce: Vec<u8>,
    pub user_id: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug)]
pub struct SessionDatabase {
    path: PathBuf,
}

impl SessionDatabase {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let database = Self { path: path.into() };
        let connection = database.connect()?;
        migrate_sessions(&connection)?;
        Ok(database)
    }

    pub fn insert(&self, token_hash: &str, session: &StoredSession) -> Result<(), String> {
        let connection = self.connect()?;
        connection.execute("DELETE FROM web_session WHERE expires_at <= unixepoch()", []).map_err(storage_error)?;
        connection
            .execute("INSERT INTO web_session (token_hash, encrypted_session, nonce, user_id, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![token_hash, session.encrypted_session, session.nonce, session.user_id, session.expires_at])
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn get(&self, token_hash: &str) -> Result<Option<StoredSession>, String> {
        let connection = self.connect()?;
        let session = connection
            .query_row("UPDATE web_session SET last_used_at = unixepoch() WHERE token_hash = ?1 AND expires_at > unixepoch() RETURNING encrypted_session, nonce, user_id, expires_at", [token_hash], decode_session)
            .optional()
            .map_err(storage_error)?;
        Ok(session)
    }

    pub fn replace(&self, token_hash: &str, session: &StoredSession) -> Result<bool, String> {
        let connection = self.connect()?;
        let changed = connection
            .execute(
                "UPDATE web_session
                 SET encrypted_session = ?2, nonce = ?3, expires_at = ?4, last_used_at = unixepoch()
                 WHERE token_hash = ?1 AND user_id = ?5 AND expires_at > unixepoch()",
                params![token_hash, session.encrypted_session, session.nonce, session.expires_at, session.user_id],
            )
            .map_err(storage_error)?;
        Ok(changed == 1)
    }

    pub fn remove(&self, token_hash: &str) -> Result<Option<StoredSession>, String> {
        let connection = self.connect()?;
        connection.query_row("DELETE FROM web_session WHERE token_hash = ?1 RETURNING encrypted_session, nonce, user_id, expires_at", [token_hash], decode_session).optional().map_err(storage_error)
    }

    fn connect(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            restrict_directory(parent)?;
        }
        let connection = Connection::open(&self.path).map_err(storage_error)?;
        restrict_database_file(&self.path)?;
        configure_connection(&connection, Duration::from_secs(5)).map_err(storage_error)?;
        connection.pragma_update(None, "synchronous", "NORMAL").map_err(storage_error)?;
        Ok(connection)
    }
}

fn decode_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSession> {
    Ok(StoredSession { encrypted_session: row.get(0)?, nonce: row.get(1)?, user_id: row.get(2)?, expires_at: row.get(3)? })
}

fn migrate_user_state(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS schema_metadata (
                 key TEXT PRIMARY KEY,
                 value INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE IF NOT EXISTS application_state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 state TEXT NOT NULL CHECK (json_valid(state)),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             ) STRICT;
             INSERT INTO schema_metadata (key, value) VALUES ('user_state_schema_version', 1)
                 ON CONFLICT(key) DO NOTHING;
             COMMIT;",
        )
        .map_err(storage_error)?;
    validate_schema_version(connection, "user_state_schema_version")
}

fn migrate_sessions(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS schema_metadata (
                 key TEXT PRIMARY KEY,
                 value INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE IF NOT EXISTS web_session (
                 token_hash TEXT PRIMARY KEY CHECK (length(token_hash) = 64),
                 encrypted_session BLOB NOT NULL,
                 nonce BLOB NOT NULL CHECK (length(nonce) = 12),
                 user_id TEXT NOT NULL,
                 expires_at INTEGER NOT NULL,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 last_used_at INTEGER NOT NULL DEFAULT (unixepoch())
             ) STRICT;
             CREATE INDEX IF NOT EXISTS web_session_expiry_idx ON web_session(expires_at);
             INSERT INTO schema_metadata (key, value) VALUES ('session_schema_version', 1)
                 ON CONFLICT(key) DO NOTHING;
             COMMIT;",
        )
        .map_err(storage_error)?;
    validate_schema_version(connection, "session_schema_version")
}

fn validate_schema_version(connection: &Connection, key: &str) -> Result<(), String> {
    let version: i64 = connection.query_row("SELECT value FROM schema_metadata WHERE key = ?1", [key], |row| row.get(0)).map_err(storage_error)?;
    if version != SCHEMA_VERSION {
        return Err(format!("unsupported SQLite {key} {version}"));
    }
    Ok(())
}

fn storage_error(error: rusqlite::Error) -> String {
    error.to_string()
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn restrict_database_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn restrict_database_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, Deserialize, PartialEq, Serialize)]
    struct State {
        count: u64,
    }

    #[test]
    fn state_survives_reopen_and_failed_mutations_roll_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.db");
        let first = UserStateDatabase::open(&path).unwrap();
        first
            .mutate::<State, _>(|state| {
                state.count = 7;
                Ok(())
            })
            .unwrap();
        assert!(first
            .mutate::<State, _>(|state| {
                state.count = 9;
                Err::<(), _>("cancelled".to_owned())
            })
            .is_err());
        drop(first);
        assert_eq!(UserStateDatabase::open(path).unwrap().read::<State>().unwrap(), State { count: 7 });
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(directory.path().join("library.db")).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn sessions_survive_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.db");
        let record = StoredSession { encrypted_session: vec![1, 2, 3], nonce: vec![0; 12], user_id: "user-1".to_owned(), expires_at: i64::MAX };
        SessionDatabase::open(&path).unwrap().insert(&"a".repeat(64), &record).unwrap();
        let reopened = SessionDatabase::open(path).unwrap();
        assert_eq!(reopened.get(&"a".repeat(64)).unwrap(), Some(record.clone()));
        let replacement = StoredSession { encrypted_session: vec![4, 5, 6], ..record.clone() };
        assert!(reopened.replace(&"a".repeat(64), &replacement).unwrap());
        assert_eq!(reopened.get(&"a".repeat(64)).unwrap(), Some(replacement.clone()));
        assert_eq!(reopened.remove(&"a".repeat(64)).unwrap(), Some(replacement));
    }

    #[test]
    fn concurrent_writers_are_serialized_without_lost_updates() {
        let directory = tempfile::tempdir().unwrap();
        let database = UserStateDatabase::open(directory.path().join("library.db")).unwrap();
        let mut writers = Vec::new();
        for _ in 0..8 {
            let database = database.clone();
            writers.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    database
                        .mutate::<State, _>(|state| {
                            state.count += 1;
                            Ok(())
                        })
                        .unwrap();
                }
            }));
        }
        for writer in writers {
            writer.join().unwrap();
        }
        assert_eq!(database.read::<State>().unwrap().count, 400);
    }
}

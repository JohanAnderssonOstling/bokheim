//! Replica sync: pull commits, cursors, outbox, sync functions.

use crate::SqlParam;
use sync_common::StateKey;
use content_address::ContentHash;
use std::io::{Error, ErrorKind};
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) mod apply;
pub(crate) mod outbox;
pub(crate) mod state;
pub use state::CURSOR_RECOVERY_PENDING_KEY;
#[cfg(test)]
mod convergence_tests;

pub(crate) use crate::DatabaseError;
pub(crate) use include_sqlite_sql::include_sql;

include_sql!("src/sync/sql/schema/tables.sql");
include_sql!("src/sync/sql/schema/index.sql");
include_sql!("src/sync/sql/sync_apply.sql");

include_sql!("src/sync/sql/sync_state.sql");

include_sql!("src/sync/sql/sync_outbox.sql");

pub(crate) fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(Error::new(ErrorKind::InvalidData, message.into())))
}

pub(crate) fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Integer, Box::new(Error::new(ErrorKind::InvalidData, message.into())))
}

pub(crate) fn nonnegative_u64(value: i64, column: usize, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_integer_column(column, format!("{name} must not be negative")))
}

pub(crate) fn storage_i64(value: u64, name: &str) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::ToSqlConversionFailure(Box::new(Error::new(ErrorKind::InvalidData, format!("{name} exceeds the database integer range")))))
}

pub(crate) fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

pub(crate) fn toc_entry_count(entries: &[library_model::BookTocEntry]) -> usize {
    entries.iter().map(|entry| 1 + toc_entry_count(&entry.children)).sum()
}

pub(crate) fn parse_revision(value: Option<String>) -> Result<Option<sync_common::LibraryRevision>, DatabaseError> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(|error| DatabaseError::message(format!("stored revision is not readable: {error}")))?;
            sync_common::LibraryRevision::new(raw).map_err(|error| DatabaseError::message(format!("stored revision is not readable: {error}")))
        })
        .transpose()
}

pub(crate) fn read_cursor_revision(conn: &rusqlite::Connection, key: PullCursorKey) -> Result<Option<String>, DatabaseError> {
    // Single-statement read used twice by the commit (pre-transaction fast
    // path and in-transaction recheck). It takes the connection rather than
    // the transaction: it never manages transaction boundaries.
    let mut revision = None;
    match key {
        PullCursorKey::Lifecycle => conn.shared_cursor_lifecycle_select(|row| {
            revision = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?,
        PullCursorKey::Field => conn.shared_cursor_field_select(|row| {
            revision = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?,
    };
    Ok(revision)
}

#[derive(Clone, Copy)]
pub(crate) enum PullCursorKey {
    Lifecycle,
    Field,
}

pub(crate) fn write_cursor_revision(tx: &rusqlite::Connection, key: PullCursorKey, revision: Option<sync_common::LibraryRevision>) -> Result<(), DatabaseError> {
    // Counterpart to the read above: one statement, no boundary management.
    // It takes the connection rather than the transaction for the same
    // reason; the commit passes its transaction, which dereferences.
    match (key, revision) {
        (PullCursorKey::Lifecycle, Some(revision)) => {
            tx.cursor_lifecycle_upsert(&revision.get().to_string()).map_err(DatabaseError::operation)?;
        }
        (PullCursorKey::Lifecycle, None) => {
            tx.cursor_lifecycle_clear().map_err(DatabaseError::operation)?;
        }
        (PullCursorKey::Field, Some(revision)) => {
            tx.cursor_field_upsert(&revision.get().to_string()).map_err(DatabaseError::operation)?;
        }
        (PullCursorKey::Field, None) => {
            tx.cursor_field_clear().map_err(DatabaseError::operation)?;
        }
    }
    Ok(())
}

// --- Metadata projection helpers. All pure: no statement here touches
// storage. The commit method below runs every write inline. ---

pub(crate) fn normalized_description(description: &str) -> String {
    let mut normalized = String::with_capacity(description.len().min(65_536));
    for character in description.chars().filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t')) {
        if normalized.len() + character.len_utf8() > 65_536 {
            break;
        }
        normalized.push(character);
    }
    normalized
}

pub(crate) fn unix_millis() -> Result<i64, DatabaseError> {
    let millis = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(DatabaseError::operation)?.as_millis();
    i64::try_from(millis).map_err(DatabaseError::operation)
}


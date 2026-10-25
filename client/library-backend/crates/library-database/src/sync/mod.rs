//! Replica sync: pull commits, cursors, outbox, sync functions.

use crate::SqlParam;
pub(crate) use crate::shared_sql::SharedSql;
use content_address::BookId;
use std::io::{Error, ErrorKind};
use sync_common::{LibraryRevision, StateKey, SyncCursor, SyncGeneration};
pub(crate) mod apply;
pub(crate) mod outbox;
pub use outbox::OutboxScan;
mod issues;
pub(crate) mod reading_positions;
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
include_sql!("src/sync/sql/mutation_issues.sql");

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

pub(crate) fn parse_revision(value: Option<String>) -> Result<Option<LibraryRevision>, DatabaseError> {
    value
        .map(|value| {
            let raw = value.parse::<u64>().map_err(|error| DatabaseError::message(format!("stored revision is not readable: {error}")))?;
            LibraryRevision::new(raw).map_err(|error| DatabaseError::message(format!("stored revision is not readable: {error}")))
        })
        .transpose()
}

/// Reads the generation and both revisions from the same SQLite statement.
pub(crate) fn read_pull_cursor(conn: &rusqlite::Connection) -> Result<SyncCursor, DatabaseError> {
    let mut raw = None;
    conn.sync_pull_cursor(|row| {
        raw = Some((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?));
        Ok(())
    })?;
    let (generation, lifecycle, field) = raw.ok_or_else(|| DatabaseError::message("sync metadata row is missing"))?;
    Ok(SyncCursor { generation: generation.map(|value| SyncGeneration::parse_str(&value).map_err(DatabaseError::operation)).transpose()?, lifecycle_revision: parse_revision(lifecycle)?, field_revision: parse_revision(field)? })
}

/// One statement, run within the same transaction as the applied page.
pub(crate) fn write_pull_cursor(tx: &rusqlite::Connection, cursor: &SyncCursor) -> Result<(), DatabaseError> {
    let generation = cursor.generation.map(|generation| generation.to_string());
    let lifecycle = cursor.lifecycle_revision.map(|revision| storage_i64(revision.get(), "lifecycle revision")).transpose()?;
    let field = cursor.field_revision.map(|revision| storage_i64(revision.get(), "field revision")).transpose()?;
    tx.sync_write_pull_cursor(generation.as_deref(), lifecycle, field).map_err(DatabaseError::operation)?;
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

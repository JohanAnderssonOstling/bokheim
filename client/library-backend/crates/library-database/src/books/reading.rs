//! Reading-position register.

use crate::{Database, DatabaseError};
use include_sqlite_sql::include_sql;
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

// ---- reading ----
// The one synchronized reading-position register, shared by every format.

use book_model::{ReadProgress, ReadingPosition};

impl Database {
    fn stored_reading_position(&self, hash: &ContentHash) -> Result<Option<ReadingPosition>, DatabaseError> {
        let mut raw = None;
        self.connection.reading_get_position(hash.as_str(), |row| {
            raw = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let Some(raw) = raw.filter(|value| value != "0" && !value.is_empty()) else {
            return Ok(None);
        };
        ReadingPosition::parse(&raw).map(Some).map_err(DatabaseError::operation)
    }

    pub fn reading_position(&self, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.map(|position| position.as_str()))
    }

    pub fn pdf_reading_position(&self, hash: &ContentHash) -> Result<Option<(u32, f32)>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.pdf_page_index().map(|page_index| (page_index, position.pdf_page_position().unwrap_or(0.0)))))
    }

    pub fn audiobook_reading_position(&self, hash: &ContentHash) -> Result<Option<u64>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.audiobook_position_millis()))
    }

    /// Stores the canonical reading position and, when given, the navigation
    /// entry the reader resolved it to. Both writes happen inline in one
    /// transaction; `entry` travels with the position because that is when it
    /// changes, and is stored locally rather than synchronized.
    pub fn update_reading_position(&self, hash: &ContentHash, position: &str, progress: Option<f32>, entry: Option<&str>) -> Result<(), DatabaseError> {
        let position = ReadingPosition::parse(position).map_err(DatabaseError::operation)?;
        let progress = progress.map(ReadProgress::new).transpose().map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let previous: Option<f32> = transaction.query_row("SELECT read_progress FROM book WHERE content_hash=?1", [hash.as_str()], |row| row.get(0)).optional()?.flatten();
            crate::sync::apply::registers::record_local(
                transaction,
                library_replica::LibraryStateEntry::ReadingPosition { content_hash: *hash, value: library_replica::ReadingPositionState { location: position, progress: progress.map(ReadProgress::value).or(previous).unwrap_or(0.0) } },
            )?;
            if let Some(entry) = entry.map(str::trim).filter(|entry| !entry.is_empty()) {
                transaction.reading_upsert_toc_entry(hash.as_str(), entry)?;
            }
            Ok(())
        })
    }
}

include_sql!("src/books/sql/reading.sql");

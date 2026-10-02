//! Reader annotations: highlights, notes, and their anchors.

use crate::sync::apply::registers::record_local;
use crate::{Database, DatabaseError};
use book_model::ReaderAnnotation;
use include_sqlite_sql::include_sql;
use rusqlite::{Row, types::Type};
use sync_common::ContentHash;

impl Database {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, DatabaseError> {
        let mut values = Vec::new();
        self.connection.get_annotations(content_hash.as_str(), |row| {
            values.push(read_annotation(row)?);
            Ok(())
        })?;
        Ok(values)
    }

    pub fn upsert_annotation(&self, value: &ReaderAnnotation) -> Result<(), DatabaseError> {
        value.validate(false).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let mut state = value.state.clone();
            if let Some(existing) = {
                let id: &str = &value.id;
                let mut existing = None;
                transaction.get_annotation_by_id(id, |row| {
                    existing = Some(read_annotation(row)?);
                    Ok(())
                })?;
                existing
            } {
                state.detail.created_at = existing.state.detail.created_at;
            }
            state.deleted = false;
            if !transaction.query_row("SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=?1)", [state.content_hash.as_str()], |row| row.get::<_, bool>(0))? {
                return Err(DatabaseError::message("annotation book not found"));
            }
            record_local(transaction, library_replica::LibraryStateEntry::Annotation { annotation_id: value.id.clone(), value: state })?;
            Ok(())
        })
    }

    pub fn delete_annotation(&self, id: &str, modified_at: i64) -> Result<(), DatabaseError> {
        u64::try_from(modified_at).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let mut existing = None;
            transaction.get_annotation_by_id(id, |row| {
                existing = Some(read_annotation(row)?);
                Ok(())
            })?;
            let mut value = existing.ok_or_else(|| DatabaseError::message("annotation not found"))?.state;
            value.modified_at = modified_at as u64;
            value.deleted = true;
            record_local(transaction, library_replica::LibraryStateEntry::Annotation { annotation_id: id.to_owned(), value })?;
            Ok(())
        })
    }
}

fn read_annotation(row: &Row<'_>) -> rusqlite::Result<ReaderAnnotation> {
    ReaderAnnotation::from_row(row.get(0)?, ContentHash::new(&row.get::<_, String>(1)?), row.get(2)?, row.get(3)?, row.get(4)?, &row.get::<_, Vec<u8>>(5)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(5, Type::Blob, error))
}

include_sql!("src/annotations/sql/schema/tables.sql");
include_sql!("src/annotations/sql/schema/index.sql");
include_sql!("src/annotations/sql/annotations.sql");

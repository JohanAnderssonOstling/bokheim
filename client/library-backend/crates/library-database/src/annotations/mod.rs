//! Reader annotations: highlights, notes, and their anchors.

use crate::SqlParam;
use crate::sync::apply::registers::record_local;
use crate::{Database, DatabaseError};
use book_model::Annotation;
use include_sqlite_sql::include_sql;
use rusqlite::{Row, types::Type};
use sync_common::ContentHash;

impl Database {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<Annotation>, DatabaseError> {
        let mut values = Vec::new();
        self.connection.get_annotations(&content_hash, |row| {
            values.push(read_annotation(row)?);
            Ok(())
        })?;
        Ok(values)
    }

    pub fn upsert_annotation(&self, value: &Annotation) -> Result<(), DatabaseError> {
        value.validate().map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let mut value = value.clone();
            if let Some(existing) = existing_annotation(transaction, &value.id)? {
                value.created_at = existing.created_at;
            }
            value.deleted = false;
            if !transaction.annotation_book_exists(&(&value.content_hash), |row| row.get::<_, bool>(0))? {
                return Err(DatabaseError::message("annotation book not found"));
            }
            record_local(transaction, library_replica::LibraryStateEntry::Annotation { value })?;
            Ok(())
        })
    }

    pub fn delete_annotation(&self, id: &str, modified_at: i64) -> Result<(), DatabaseError> {
        u64::try_from(modified_at).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let mut value = existing_annotation(transaction, id)?.ok_or_else(|| DatabaseError::message("annotation not found"))?;
            value.modified_at = modified_at as u64;
            value.deleted = true;
            record_local(transaction, library_replica::LibraryStateEntry::Annotation { value })?;
            Ok(())
        })
    }
}

fn existing_annotation(transaction: &rusqlite::Transaction<'_>, id: &str) -> Result<Option<Annotation>, DatabaseError> {
    let mut existing = None;
    transaction.get_annotation_by_id(id, |row| {
        existing = Some(read_annotation(row)?);
        Ok(())
    })?;
    Ok(existing)
}

fn read_annotation(row: &Row<'_>) -> rusqlite::Result<Annotation> {
    let bytes: Vec<u8> = row.get(0)?;
    Annotation::decode(&bytes).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Blob, Box::new(error)))
}

include_sql!("src/annotations/sql/schema/tables.sql");
include_sql!("src/annotations/sql/schema/index.sql");
include_sql!("src/annotations/sql/annotations.sql");

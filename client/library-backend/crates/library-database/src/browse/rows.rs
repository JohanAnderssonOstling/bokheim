//! Decode database rows without masking malformed values.

use std::io::{Error, ErrorKind};
pub(crate) use library_model::BookCardRow;
pub(crate) use sync_common::ContentHash;

pub fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(Error::new(ErrorKind::InvalidData, message.into())))
}

pub fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Integer, Box::new(Error::new(ErrorKind::InvalidData, message.into())))
}

pub(crate) fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

pub(crate) fn read_book_card_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BookCardRow> {
    let raw_content_hash: ContentHash = row.get("content_hash")?;
    let title: Option<String> = row.get("title")?;
    let duration_column = row.as_ref().column_index("audiobook_duration_ms")?;
    let duration_ms: Option<i64> = row.get(duration_column)?;
    Ok(BookCardRow {
        source: library_model::BookSource::Library { content_hash: raw_content_hash },
        title: super::text::display_title(title.as_deref()).to_owned(),
        subtitle: row.get("subtitle")?,
        author: row.get::<_, Option<String>>("author")?.unwrap_or_default(),
        progress: row.get::<_, Option<f32>>("progress")?.unwrap_or_default(),
        downloaded: row.get("downloaded")?,
        download_requested: row.get("download_requested")?,
        format_category: crate::browse::options::file_type_of(row.get("format")?),
        audiobook_duration_ms: duration_ms.map(u64::try_from).transpose().map_err(|e| invalid_integer_column(duration_column, e.to_string()))?,
        added_at: row.get::<_, Option<i64>>("added_at")?.unwrap_or_default(),
        source_directory: None,
    })
}

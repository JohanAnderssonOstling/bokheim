//! Persistence for metadata derived from audiobook files.

use super::*;
use rusqlite::OptionalExtension;

pub fn audiobook_chapters(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<app_contract::library::AudiobookChapter>, rusqlite::Error> {
    let chapters = conn.query_row("SELECT chapters_json FROM audiobook_metadata WHERE content_hash=?1", [content_hash.as_str()], |row| row.get::<_, String>(0)).optional()?;
    chapters.map_or_else(|| Ok(Vec::new()), |chapters| serde_json::from_str(&chapters).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))))
}

#[cfg(feature = "audiobook-metadata")]
pub fn upsert_audiobook_metadata(conn: &Connection, content_hash: &ContentHash, metadata: &book_metadata::AudiobookMetadata) -> Result<(), rusqlite::Error> {
    let duration_ms = i64::try_from(metadata.duration_ms).map_err(|_| invalid_integer_column(1, "audiobook duration exceeds SQLite integer range"))?;
    let chapter_count = i64::try_from(metadata.chapters.len()).map_err(|_| invalid_integer_column(2, "audiobook chapter count exceeds SQLite integer range"))?;
    let chapters_json = serde_json::to_string(&metadata.chapters).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let chapter_source = match metadata.chapter_origin {
        book_metadata::ChapterOrigin::EmbeddedM4b => "embedded_m4b",
        book_metadata::ChapterOrigin::Audnexus => "audnexus",
        book_metadata::ChapterOrigin::FullAudiobookFallback => "full_audiobook_fallback",
    };
    conn.upsert_audiobook_metadata(content_hash.as_str(), duration_ms, chapter_count, chapter_source, &chapters_json)?;
    Ok(())
}

//! Audiobook playback state and chapter navigation.

use content_address::ContentHash;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::{Database, DatabaseError};
pub(crate) use include_sqlite_sql::include_sql;
pub(crate) use library_model::BookTocEntry;

// ---- audiobook ----
// Cached audiobook playback metadata: chapters, duration, and recording narrator.
//
// Reading is one call; committing freshly inspected metadata is one
// write call with its own transaction. Callers never hold a connection or
// transaction across the inspection work that produces the metadata.

pub(crate) use book_model::AudiobookChapter;

/// A chapter is an interval; a navigation entry is a point. The end is the next
/// chapter's start, and the last chapter runs to the end of the audio.
pub fn chapters_from_navigation(entries: &[BookTocEntry], duration_ms: u64) -> Vec<AudiobookChapter> {
    let starts = entries.iter().filter_map(|entry| Some((entry.title.as_str(), book_model::audiobook_toc_position(&entry.target)?))).collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .map(|(index, (title, start_ms))| AudiobookChapter { title: book_model::audiobook_chapter_display_title(title, index + 1), start_ms: *start_ms, end_ms: starts.get(index + 1).map_or(duration_ms, |(_, next)| *next) })
        .collect()
}

/// The file's chapters as the navigation every format shares: a title and the
/// offset it opens at, flat, because an M4B chapter list has no nesting.
pub(crate) fn navigation_from_chapters(chapters: &[AudiobookChapter]) -> Vec<BookTocEntry> {
    chapters.iter().map(|chapter| BookTocEntry { title: chapter.title.clone(), target: book_model::audiobook_toc_target(chapter.start_ms), children: Vec::new() }).collect()
}

impl Database {
    /// Everything a player needs to open the book without inspecting its
    /// file, except the cover, which lives in asset storage. `None` until the
    /// file's chapters and duration have been cached.
    pub fn audiobook_playback_metadata(&self, hash: &sync_common::ContentHash) -> Result<Option<library_model::PlaybackMetadata>, DatabaseError> {
        let mut cached_row = None;
        self.connection.audiobook_cached_metadata(hash, |row| {
            cached_row = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let Some((duration_ms, toc_json)) = cached_row else { return Ok(None) };
        let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(DatabaseError::operation)?;
        let chapters = chapters_from_navigation(&entries, duration_ms.max(0) as u64);
        let mut evidence = None;
        self.connection.shared_audiobook_recording_evidence(hash, |row| {
            evidence = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let narrator = evidence.and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok()).and_then(|request| request.recording.narrators.into_iter().next());
        let mut title_author = None;
        self.connection.audiobook_playback_title_author(hash, |row| {
            title_author = Some((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let (title, author) = title_author.unwrap_or_default();
        Ok(Some(library_model::PlaybackMetadata {
            format: self.book_format(hash)?.unwrap_or(book_model::BookFormat::M4b),
            title,
            author,
            narrator,
            duration_ms: duration_ms as u64,
            chapters,
            tracks: self.audiobook_tracks(hash)?,
            cover: Vec::new(),
        }))
    }

    pub fn audiobook_tracks(&self, hash: &sync_common::ContentHash) -> Result<Vec<book_model::AudiobookTrack>, DatabaseError> {
        let mut tracks = None;
        self.connection.audiobook_track_index_select(hash, |row| {
            tracks = Some(serde_json::from_str::<Vec<book_model::AudiobookTrack>>(&row.get::<_, String>(0)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))?);
            Ok(())
        })?;
        Ok(tracks.unwrap_or_default())
    }
}

include_sql!("src/books/sql/audiobook.sql");

//! Audiobook playback state and chapter navigation.

use crate::query_methods::{first_column, query_last};
pub(crate) use crate::{Database, DatabaseError};
use crate::stored_document;
use content_address::BookId;
pub(crate) use include_sqlite_sql::include_sql;
pub(crate) use library_model::BookTocEntry;
use library_model::PlaybackMetadata;

// ---- audiobook ----
// Cached audiobook playback metadata: chapters, duration, and recording narrator.
//
// Reading is one call; committing freshly inspected metadata is one
// write call with its own transaction. Callers never hold a connection or
// transaction across the inspection work that produces the metadata.

pub(crate) use book_model::{AudiobookChapter, AudiobookTrack, BookFormat};

/// A verified download already has immutable playback facts keyed by its
/// content hash. Reuse complete facts without claiming this device inspected
/// the audio. Missing or inconsistent metadata must still go through inspection.
#[cfg(feature = "scanner")]
pub(crate) fn reusable_playback_format(hash: BookId, format: Option<BookFormat>, duration_ms: Option<i64>, toc: Option<&[u8]>, tracks: Option<&[u8]>) -> Option<BookFormat> {
    use book_model::{AudiobookTrack, audiobook_toc_position, tracks_match_archive};
    use std::collections::HashSet;

    let format = format.filter(|format| matches!(format, BookFormat::M4b | BookFormat::Mp3Folder))?;
    let duration = u64::try_from(duration_ms?).ok().filter(|duration| *duration > 0)?;
    let entries: Vec<BookTocEntry> = stored_document::decode(toc?).ok()?;
    if entries.is_empty() {
        return None;
    }
    let mut previous = None;
    for entry in entries {
        let start = audiobook_toc_position(&entry.target)?;
        if !entry.children.is_empty() || start >= duration || previous.is_some_and(|previous| start <= previous) {
            return None;
        }
        previous = Some(start);
    }
    if format == BookFormat::Mp3Folder {
        let tracks: Vec<AudiobookTrack> = stored_document::decode(tracks?).ok()?;
        if !tracks_match_archive(&tracks, hash) {
            return None;
        }
        let mut end = 0;
        let mut names = HashSet::new();
        for track in tracks {
            if track.start_ms != end || !names.insert(track.name) {
                return None;
            }
            end = track.end_ms;
        }
        if end != duration {
            return None;
        }
    }
    Some(format)
}

/// A chapter is an interval; a navigation entry is a point. The end is the next
/// chapter's start, and the last chapter runs to the end of the audio.
pub fn chapters_from_navigation(entries: &[BookTocEntry], duration_ms: u64) -> impl Iterator<Item = AudiobookChapter> + '_ {
    let mut starts = entries.iter().filter_map(|entry| Some((entry.title.as_str(), book_model::audiobook_toc_position(&entry.target)?))).peekable();
    let mut position = 0;
    std::iter::from_fn(move || {
        let (title, start_ms) = starts.next()?;
        position += 1;
        Some(AudiobookChapter { title: book_model::audiobook_chapter_display_title(title, position), start_ms, end_ms: starts.peek().map_or(duration_ms, |(_, next)| *next) })
    })
}

/// The file's chapters as the navigation every format shares: a title and the
/// offset it opens at, flat, because an M4B chapter list has no nesting.
pub(crate) fn navigation_from_chapters(chapters: &[AudiobookChapter]) -> Vec<BookTocEntry> {
    chapters.iter().map(|chapter| BookTocEntry { title: chapter.title.clone(), target: book_model::audiobook_toc_target(chapter.start_ms), children: Vec::new() }).collect()
}

impl Database {
    /// Naming hints only: logical track names and content identity stay intact.
    /// Uses the book format already captured for the download.
    pub fn audiobook_download_track_titles(&self, hash: &BookId, format: BookFormat) -> Result<Vec<(String, String)>, DatabaseError> {
        if format != BookFormat::Mp3Folder {
            return Ok(Vec::new());
        }
        let Some(metadata) = self.audiobook_playback_metadata(hash)? else { return Ok(Vec::new()) };
        Ok(track_title_hints(*hash, &metadata.chapters, metadata.tracks))
    }

    /// Everything a player needs to open the book without inspecting its
    /// file, except the cover, which lives in asset storage. `None` until the
    /// file's chapters and duration have been cached.
    pub fn audiobook_playback_metadata(&self, hash: &BookId) -> Result<Option<PlaybackMetadata>, DatabaseError> {
        let cached_row = query_last(
            |read| self.connection.audiobook_cached_metadata(hash, read),
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Option<i64>>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<Vec<u8>>>(6)?)),
        )?;
        let Some((duration_ms, toc, format, title, author, evidence, tracks)) = cached_row else { return Ok(None) };
        let entries: Vec<BookTocEntry> = stored_document::decode(&toc)?;
        let narrator = evidence.and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok()).and_then(|request| request.recording.narrators.into_iter().next());
        Ok(Some(PlaybackMetadata {
            format: format.and_then(BookFormat::from_code).ok_or_else(|| DatabaseError::message("book has no known format"))?,
            title,
            author,
            narrator,
            duration_ms: duration_ms as u64,
            chapters: chapters_from_navigation(&entries, duration_ms as u64).collect(),
            tracks: tracks.map(|bytes| stored_document::decode(&bytes)).transpose()?.unwrap_or_default(),
            cover: Vec::new(),
        }))
    }

    pub fn audiobook_tracks(&self, hash: &sync_common::BookId) -> Result<Vec<book_model::AudiobookTrack>, DatabaseError> {
        let tracks = first_column::<Vec<u8>>(|read| self.connection.audiobook_track_index_select(hash, read))?;
        tracks.map(|bytes| stored_document::decode(&bytes)).transpose().map(Option::unwrap_or_default)
    }
}

pub(crate) fn track_title_hints(hash: BookId, chapters: &[AudiobookChapter], tracks: Vec<AudiobookTrack>) -> Vec<(String, String)> {
    if !book_model::tracks_match_archive(&tracks, hash) {
        return Vec::new();
    }
    tracks.into_iter().enumerate().map(|(index, track)| {
        let title = chapters.iter().find(|chapter| chapter.start_ms >= track.start_ms && chapter.start_ms < track.end_ms)
            .map(|chapter| chapter.title.clone()).filter(|title| !title.trim().is_empty()).unwrap_or_else(|| format!("Track {}", index + 1));
        (track.name, title)
    }).collect()
}

include_sql!("src/books/sql/audiobook.sql");

#[cfg(all(test, feature = "scanner"))]
mod tests {
    use super::*;
    use book_model::{AudiobookTrack, audiobook_toc_target};

    #[test]
    fn playback_reuse_requires_complete_metadata_for_the_same_archive() {
        let hash = BookId::new(&"a".repeat(64));
        let toc = serde_json::to_string(&vec![BookTocEntry { title: "Chapter".into(), target: audiobook_toc_target(0), children: vec![] }]).unwrap();
        let track = AudiobookTrack { name: "01.mp3".into(), start_ms: 0, end_ms: 1000, offset: 0, length: 100, archive_hash: Some(hash) };
        let tracks = serde_json::to_string(&vec![track.clone()]).unwrap();
        let reuse = |format, duration, toc, tracks| reusable_playback_format(hash, Some(format), duration, toc, tracks);
        assert_eq!(reuse(BookFormat::Mp3Folder, Some(1000), Some(toc.as_bytes()), Some(tracks.as_bytes())), Some(BookFormat::Mp3Folder));
        assert_eq!(reuse(BookFormat::M4b, Some(1000), Some(toc.as_bytes()), None), Some(BookFormat::M4b));
        for duration in [None, Some(0), Some(-1)] {
            assert_eq!(reuse(BookFormat::M4b, duration, Some(toc.as_bytes()), None), None);
        }
        for incomplete_toc in [None, Some(b"[]".as_slice()), Some(b"invalid".as_slice())] {
            assert_eq!(reuse(BookFormat::M4b, Some(1000), incomplete_toc, None), None);
        }
        for incomplete_tracks in [None, Some(b"[]".as_slice()), Some(b"invalid".as_slice())] {
            assert_eq!(reuse(BookFormat::Mp3Folder, Some(1000), Some(toc.as_bytes()), incomplete_tracks), None);
        }
        let mut wrong_archive = track.clone();
        wrong_archive.archive_hash = Some(BookId::new(&"b".repeat(64)));
        let wrong_archive = serde_json::to_string(&vec![wrong_archive]).unwrap();
        assert_eq!(reuse(BookFormat::Mp3Folder, Some(1000), Some(toc.as_bytes()), Some(wrong_archive.as_bytes())), None);
        assert_eq!(reuse(BookFormat::Mp3Folder, Some(2000), Some(toc.as_bytes()), Some(tracks.as_bytes())), None);
        assert_eq!(reuse(BookFormat::Epub, Some(1000), Some(toc.as_bytes()), Some(tracks.as_bytes())), None);
        let binary_toc = stored_document::encode(&serde_json::from_str::<Vec<BookTocEntry>>(&toc).unwrap()).unwrap();
        let binary_tracks = stored_document::encode(&vec![track]).unwrap();
        assert_eq!(reuse(BookFormat::Mp3Folder, Some(1000), Some(binary_toc.as_slice()), Some(binary_tracks.as_slice())), Some(BookFormat::Mp3Folder));
        assert_eq!(reuse(BookFormat::M4b, Some(1000), Some(binary_toc.as_slice()), None), Some(BookFormat::M4b));
    }
}

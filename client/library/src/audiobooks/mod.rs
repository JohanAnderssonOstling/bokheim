//! Persistence for metadata derived from audiobook files.

use crate::audible_jobs;
use crate::audiobooks::queries::AudiobooksSql;
#[cfg(test)]
use crate::schema::library_write_transaction;
use crate::storage_values::invalid_integer_column;
use crate::storage_values::invalid_text_column;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

pub fn audiobook_chapters(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<library_model::AudiobookChapter>, rusqlite::Error> {
    let chapters = conn.query_row(include_str!("sql/audiobooks/audiobook_chapters_select.sql"), [content_hash.as_str()], |row| row.get::<_, String>(0)).optional()?;
    chapters.map_or_else(|| Ok(Vec::new()), |chapters| serde_json::from_str(&chapters).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))))
}

/// Persist imported chapters, recording evidence and lookup work in the caller's transaction.
#[cfg(feature = "audiobook-metadata")]
pub fn upsert_audiobook_metadata(conn: &rusqlite::Transaction<'_>, content_hash: &ContentHash, metadata: &book_metadata::AudiobookMetadata) -> Result<(), rusqlite::Error> {
    let duration_ms = i64::try_from(metadata.duration_ms).map_err(|_| invalid_integer_column(1, "audiobook duration exceeds SQLite integer range"))?;
    let chapter_count = i64::try_from(metadata.chapters.len()).map_err(|_| invalid_integer_column(2, "audiobook chapter count exceeds SQLite integer range"))?;
    let chapters_json = serde_json::to_string(&metadata.chapters).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let chapter_source = match metadata.chapter_origin {
        book_metadata::ChapterOrigin::EmbeddedM4b => "embedded_m4b",
        book_metadata::ChapterOrigin::Audnexus => "audnexus",
        book_metadata::ChapterOrigin::FullAudiobookFallback => "full_audiobook_fallback",
    };
    // Preserve a database-only chapter plan while the imported evidence is unchanged.
    let applied_request: Option<String> = conn.query_row(include_str!("sql/audiobooks/upsert_audiobook_metadata_select.sql"), [content_hash.as_str()], |row| row.get(0)).optional()?;
    let request = metadata.audible_lookup_request();
    let preserve_chapters = applied_request.and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok()).is_some_and(|applied| applied == request);
    if !preserve_chapters {
        conn.upsert_audiobook_metadata(content_hash.as_str(), duration_ms, chapter_count, chapter_source, &chapters_json)?;
    }
    // Every inspection path (including browser imports and playback recovery)
    // queues from the original file evidence, never the enriched chapter overlay.
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    audible_jobs::discover(conn, content_hash, metadata_contract::audible::POLICY_VERSION, &request, now)?;
    Ok(())
}

#[cfg(all(test, feature = "audiobook-metadata"))]
mod evidence_tests {
    use super::*;

    #[test]
    fn failed_job_insert_rolls_back_metadata_and_recording_evidence() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let hash = ContentHash::new(&"2".repeat(64));
        let metadata = book_metadata::AudiobookMetadata {
            title: "Book".into(),
            authors: Vec::new(),
            description: String::new(),
            duration_ms: 1000,
            chapters: vec![book_metadata::AudiobookChapterMetadata { title: "Full audiobook".into(), start_ms: 0, end_ms: 1000 }],
            chapter_origin: book_metadata::ChapterOrigin::FullAudiobookFallback,
            recording: Default::default(),
        };
        conn.execute_batch(include_str!("sql/audiobooks/failed_job_insert_rolls_back_metadata_and_recording_evidence_create.sql")).unwrap();
        {
            let tx = library_write_transaction(&mut conn).unwrap();
            assert!(upsert_audiobook_metadata(&tx, &hash, &metadata).is_err());
        }
        for (table, sql) in [
            ("audiobook_metadata", include_str!("sql/mod/failed_job_insert_rolls_back_metadata_and_recording_evidence_query_audiobook_metadata.sql")),
            ("audible_enrichment_jobs", include_str!("sql/mod/failed_job_insert_rolls_back_metadata_and_recording_evidence_query_audible_enrichment_jobs.sql")),
        ] {
            assert_eq!(conn.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap(), 0, "unexpected rows in {table}");
        }
        conn.execute_batch(include_str!("sql/audiobooks/failed_job_insert_rolls_back_metadata_and_recording_evidence_drop.sql")).unwrap();
        let tx = library_write_transaction(&mut conn).unwrap();
        upsert_audiobook_metadata(&tx, &hash, &metadata).unwrap();
        tx.commit().unwrap();
        assert_eq!(audiobook_chapters(&conn, &hash).unwrap().len(), 1);
        assert_eq!(conn.query_row(include_str!("sql/audiobooks/failed_job_insert_rolls_back_metadata_and_recording_evidence_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "pending");
    }

    #[test]
    fn recording_evidence_is_persisted_and_removed_with_local_metadata() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let hash = ContentHash::new(&"1".repeat(64));
        let mut metadata = book_metadata::AudiobookMetadata {
            title: "Book".into(),
            authors: Vec::new(),
            description: String::new(),
            duration_ms: 1000,
            chapters: vec![book_metadata::AudiobookChapterMetadata { title: "Full audiobook".into(), start_ms: 0, end_ms: 1000 }],
            chapter_origin: book_metadata::ChapterOrigin::FullAudiobookFallback,
            recording: metadata_contract::audible::RecordingEvidence { asins: vec!["B0DWXY6C69".into()], narrators: vec!["Narrator".into()], ..Default::default() },
        };
        {
            let tx = conn.unchecked_transaction().unwrap();
            upsert_audiobook_metadata(&tx, &hash, &metadata).unwrap();
            tx.commit().unwrap();
        }
        let read = || {
            let json: String = conn.query_row(include_str!("sql/audiobooks/recording_evidence_is_persisted_and_removed_with_local_metadata_select.sql"), [hash.as_str()], |row| row.get(0)).unwrap();
            serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).unwrap().recording
        };
        assert_eq!(read(), metadata.recording);
        conn.execute(include_str!("sql/audiobooks/recording_evidence_is_persisted_and_removed_with_local_metadata_update.sql"), []).unwrap();
        {
            let tx = conn.unchecked_transaction().unwrap();
            upsert_audiobook_metadata(&tx, &hash, &metadata).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(conn.query_row(include_str!("sql/audiobooks/failed_job_insert_rolls_back_metadata_and_recording_evidence_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "no_match");
        metadata.recording.subtitle = Some("Preserved subtitle".into());
        {
            let tx = conn.unchecked_transaction().unwrap();
            upsert_audiobook_metadata(&tx, &hash, &metadata).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(read(), metadata.recording);
        assert_eq!(conn.query_row(include_str!("sql/audiobooks/failed_job_insert_rolls_back_metadata_and_recording_evidence_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "pending");
        let queued: String = conn.query_row(include_str!("sql/audiobooks/recording_evidence_is_persisted_and_removed_with_local_metadata_select_4.sql"), [], |row| row.get(0)).unwrap();
        let queued: metadata_contract::audible::LookupRequest = serde_json::from_str(&queued).unwrap();
        assert_eq!(queued.recording, metadata.recording);
        conn.execute(include_str!("sql/audiobooks/recording_evidence_is_persisted_and_removed_with_local_metadata_delete.sql"), [hash.as_str()]).unwrap();
        assert_eq!(conn.query_row(include_str!("sql/audiobooks/recording_evidence_is_persisted_and_removed_with_local_metadata_select_5.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
}

pub(crate) mod queries;

/// Persisted playback information, independent of the application's audio host.
pub struct CachedPlaybackMetadata {
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    pub duration_ms: u64,
    pub chapters: Vec<library_model::AudiobookChapter>,
}

pub fn cached_playback_metadata(conn: &Connection, hash: ContentHash) -> rusqlite::Result<Option<CachedPlaybackMetadata>> {
    let row = conn.query_row(include_str!("sql/cached_playback_metadata.sql"), [hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?))).optional()?;
    let Some((title, author, duration_ms, chapters)) = row else {
        return Ok(None);
    };
    let chapters = serde_json::from_str(&chapters).map_err(|error| invalid_text_column(3, format!("Invalid cached chapters: {error}")))?;
    let narrator = recording_narrator(conn, hash)?;
    Ok(Some(CachedPlaybackMetadata { title, author, narrator, duration_ms: duration_ms as u64, chapters }))
}

/// Display metadata is available before a publication has been inspected.
pub fn playback_title_author(conn: &Connection, hash: ContentHash) -> rusqlite::Result<(String, String)> {
    conn.query_row(include_str!("sql/playback_title_author.sql"), [hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
}

pub fn recording_narrator(conn: &Connection, hash: ContentHash) -> rusqlite::Result<Option<String>> {
    let evidence: Option<String> = conn.query_row(include_str!("sql/cached_recording_evidence.sql"), [hash.as_str()], |row| row.get(0)).optional()?;
    let narrator = evidence.and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok()).and_then(|request| request.recording.narrators.into_iter().next());
    Ok(narrator)
}

#[cfg(all(test, feature = "audiobook-metadata"))]
mod playback_tests {
    use super::*;

    #[test]
    fn playback_cache_preserves_uncached_inspection_and_validates_chapters() {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        let hash = ContentHash::new(&"a".repeat(64));
        conn.execute(include_str!("sql/playback_test_book.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("sql/playback_test_placement.sql"), [hash.as_str()]).unwrap();
        assert_eq!(playback_title_author(&conn, hash).unwrap(), ("Audio".into(), "".into()));
        assert!(cached_playback_metadata(&conn, hash).unwrap().is_none());
        assert!(recording_narrator(&conn, hash).unwrap().is_none());
        let mut metadata = book_metadata::AudiobookMetadata {
            title: "Audio".into(),
            authors: Vec::new(),
            description: String::new(),
            duration_ms: 0,
            chapters: vec![book_metadata::AudiobookChapterMetadata { title: "Chapter".into(), start_ms: 0, end_ms: 1000 }],
            chapter_origin: book_metadata::ChapterOrigin::EmbeddedM4b,
            recording: metadata_contract::audible::RecordingEvidence { narrators: vec!["First".into(), "Second".into()], ..Default::default() },
        };
        for duration in [0, 1000] {
            metadata.duration_ms = duration;
            let tx = conn.unchecked_transaction().unwrap();
            upsert_audiobook_metadata(&tx, &hash, &metadata).unwrap();
            tx.commit().unwrap();
            let cached = cached_playback_metadata(&conn, hash).unwrap();
            if duration == 0 {
                assert!(cached.is_none(), "zero duration must trigger inspection");
            } else {
                let cached = cached.unwrap();
                assert_eq!(cached.duration_ms, 1000);
                assert_eq!(cached.chapters[0].title, "Chapter");
                assert_eq!(cached.narrator.as_deref(), Some("First"));
            }
        }
        conn.execute(include_str!("sql/playback_test_invalid_chapters.sql"), [hash.as_str()]).unwrap();
        assert!(cached_playback_metadata(&conn, hash).is_err());
    }
}

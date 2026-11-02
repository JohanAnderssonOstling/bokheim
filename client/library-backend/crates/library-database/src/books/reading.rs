//! Reading-position register.

use crate::books::toc::TocSql;
use crate::query_methods::{first_column, query_last};
use crate::sync::apply::registers::{canonical_body, record_local};
use crate::{Database, DatabaseError};
use include_sqlite_sql::include_sql;
use library_replica::{LibraryStateEntry, ReadingPositionState};
use sync_common::BookId;
use sync_common::{StateKey, wire};

// ---- reading ----
// The one synchronized reading-position register, shared by every format.

use book_model::{BookTocEntry, ChapterReadStatus, ReadProgress, ReadingPosition};

fn parse_stored_position(raw: Option<String>) -> Result<Option<ReadingPosition>, DatabaseError> {
    raw.filter(|value| value != "0" && !value.is_empty()).map(|raw| ReadingPosition::parse(&raw)).transpose().map_err(DatabaseError::operation)
}

impl Database {
    pub fn chapter_read_status(&self, hash: &BookId) -> Result<ChapterReadStatus, DatabaseError> {
        let Some((bytes, _)) = canonical_body(&self.connection, "chapter_read_status", &StateKey::Book(*hash), "")? else { return Ok(ChapterReadStatus::default()) };
        match wire::decode(&bytes, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
            LibraryStateEntry::ChapterReadStatus { value, .. } => Ok(value),
            _ => Err(DatabaseError::message("invalid chapter read-status register")),
        }
    }

    pub fn reset_chapter_read_status(&self, hash: &BookId) -> Result<bool, DatabaseError> {
        self.reset_chapter_read_status_at(hash, None)
    }

    fn reading_toc(&self, hash: &BookId) -> Result<Vec<BookTocEntry>, DatabaseError> {
        let toc = first_column::<Vec<u8>>(|read| self.connection.book_toc_select(hash, read))?.unwrap_or_default();
        if toc.is_empty() { Ok(Vec::new()) } else { crate::stored_document::decode(&toc) }
    }

    pub fn reset_chapter_read_status_at(&self, hash: &BookId, target: Option<&str>) -> Result<bool, DatabaseError> {
        self.write(|transaction| {
            let mut status = self.chapter_read_status(hash)?;
            let previous = status.clone();
            if let Some(target) = target {
                status.reset_chapter(&self.reading_toc(hash)?, target);
            } else {
                status.reset();
            }
            if status == previous {
                return Ok(false);
            }
            record_local(transaction, LibraryStateEntry::ChapterReadStatus { book_id: *hash, value: status })?;
            Ok(true)
        })
    }

    /// Marks the whole book read — at 100%, every chapter read — or unread: at
    /// 0%, no chapter read. Where the reader left off stays where it is; only a
    /// book never opened is given a place, its start, to be read at. Returns
    /// whether anything changed, so its owner can schedule publication.
    pub fn set_book_read(&self, hash: &BookId, read: bool) -> Result<bool, DatabaseError> {
        let format = self.book_format(hash)?.ok_or_else(|| DatabaseError::message("book is not in the library"))?;
        self.write(|transaction| {
            let previous_chapters = self.chapter_read_status(hash)?;
            let (previous_position, previous) = self.stored_reading_state(hash)?;
            let previous = previous.unwrap_or(0.0);
            let mut chapters = ChapterReadStatus::default();
            chapters.mark_all(&self.reading_toc(hash)?, read);
            let chapters_changed = previous_chapters != chapters;
            let progress = if read { 100.0 } else { 0.0 };
            // An unopened book is unread already; it gets no position for it.
            let position_changed = (read || previous_position.is_some()) && (previous_position.is_none() || previous != progress);
            if chapters_changed {
                record_local(transaction, LibraryStateEntry::ChapterReadStatus { book_id: *hash, value: chapters })?;
            }
            if position_changed {
                let position = previous_position.unwrap_or_else(|| ReadingPosition::start(format));
                record_local(transaction, LibraryStateEntry::ReadingPosition { book_id: *hash, value: ReadingPositionState { location: position, progress } })?;
            }
            Ok(position_changed || chapters_changed)
        })
    }

    fn stored_reading_state(&self, hash: &BookId) -> Result<(Option<ReadingPosition>, Option<f32>), DatabaseError> {
        let state: (Option<String>, Option<f32>) = query_last(|read| self.connection.reading_state(hash, read), |row| Ok((row.get(0)?, row.get(1)?)))?.unwrap_or_default();
        Ok((parse_stored_position(state.0)?, state.1))
    }

    fn stored_reading_position(&self, hash: &BookId) -> Result<Option<ReadingPosition>, DatabaseError> {
        let raw = first_column::<Option<String>>(|read| self.connection.reading_state(hash, read))?.flatten();
        parse_stored_position(raw)
    }

    pub fn reading_position(&self, hash: &BookId) -> Result<Option<String>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.map(|position| position.as_str()))
    }

    pub fn pdf_reading_position(&self, hash: &BookId) -> Result<Option<(u32, f32)>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.pdf_page_index().map(|page_index| (page_index, position.pdf_page_position().unwrap_or(0.0)))))
    }

    pub fn audiobook_reading_position(&self, hash: &BookId) -> Result<Option<u64>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.audiobook_position_millis()))
    }

    /// Stores the canonical reading position and, when given, the navigation
    /// entry the reader resolved it to. Both writes happen inline in one
    /// transaction; `entry` travels with the position because that is when it
    /// changes, and is stored locally rather than synchronized. Returns whether
    /// the synchronized position changed, so its owner can schedule publication.
    pub fn update_reading_position(&self, hash: &BookId, position: &str, progress: Option<f32>, entry: Option<&str>) -> Result<bool, DatabaseError> {
        let position = ReadingPosition::parse(position).map_err(DatabaseError::operation)?;
        let progress = progress.map(ReadProgress::new).transpose().map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let previous_chapters = self.chapter_read_status(hash)?;
            let (previous_position, previous) = self.stored_reading_state(hash)?;
            let progress = progress.map(ReadProgress::value).or(previous).unwrap_or(0.0);
            let mut chapters = previous_chapters.clone();
            let entries = self.reading_toc(hash)?;
            chapters.update_position(&entries, &position, entry, progress >= 100.0);
            let chapters_changed = previous_chapters != chapters;
            let changed = previous_position.as_ref() != Some(&position) || previous.unwrap_or(0.0) != progress || chapters_changed;
            if chapters_changed {
                record_local(transaction, LibraryStateEntry::ChapterReadStatus { book_id: *hash, value: chapters })?;
            }
            record_local(transaction, LibraryStateEntry::ReadingPosition { book_id: *hash, value: ReadingPositionState { location: position, progress } })?;
            if let Some(entry) = entry.map(str::trim).filter(|entry| !entry.is_empty()) {
                transaction.reading_upsert_toc_entry(hash, entry)?;
            }
            Ok(changed)
        })
    }
}

include_sql!("src/books/sql/reading.sql");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_fixtures::FixturesSql;
    use book_model::BookFormat;

    #[test]
    fn reading_updates_retain_omitted_progress_and_identify_unchanged_positions() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = BookId::from_bytes([7; 32]);
        assert!(db.reading_position(&hash).unwrap().is_none());
        db.connection.fixtures_insert_book_7(&hash).unwrap();
        assert!(db.update_reading_position(&hash, "epubcfi(/6/2)", Some(70.0), None).unwrap());
        assert!(db.update_reading_position(&hash, "epubcfi(/6/4)", None, None).unwrap());
        let stored: (String, f32) = db.connection.fixtures_read_book_read_pos_read_progress(&hash, |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(stored, ("epubcfi(/6/4)".into(), 70.0));
        assert!(!db.update_reading_position(&hash, "epubcfi(/6/4)", None, None).unwrap());
        assert!(db.update_reading_position(&hash, "epubcfi(/6/4)", Some(0.0), None).unwrap());
        assert!(!db.update_reading_position(&hash, "epubcfi(/6/4)", None, None).unwrap());
    }

    #[test]
    fn marking_read_preserves_resume_positions_and_initializes_legacy_empty_positions() {
        for raw in [None, Some(""), Some("0"), Some("epubcfi(/6/8)")] {
            let db = Database::open(":memory:").unwrap();
            db.initialize_library().unwrap();
            let hash = BookId::from_bytes([7; 32]);
            db.connection.fixtures_insert_book_7(&hash).unwrap();
            db.connection.execute("UPDATE book SET format=?1,read_pos=?2,read_progress=NULL WHERE content_hash=?3", (BookFormat::Epub as i64, raw, hash)).unwrap();
            let previous = raw.filter(|raw| !raw.is_empty() && *raw != "0").map(str::to_owned);
            assert_eq!(db.reading_position(&hash).unwrap(), previous);
            assert!(!db.set_book_read(&hash, false).unwrap());
            assert_eq!(db.reading_position(&hash).unwrap(), previous);
            assert!(db.set_book_read(&hash, true).unwrap());
            let resume = previous.unwrap_or_else(|| ReadingPosition::start(BookFormat::Epub).as_str());
            assert_eq!(db.reading_position(&hash).unwrap(), Some(resume.clone()));
            assert!(!db.set_book_read(&hash, true).unwrap());
            assert!(db.set_book_read(&hash, false).unwrap());
            assert_eq!(db.reading_position(&hash).unwrap(), Some(resume));
            assert!(!db.set_book_read(&hash, false).unwrap());
            assert_eq!(db.connection.fixtures_read_book_read_pos_read_progress(&hash, |row| row.get::<_, f32>(1)).unwrap(), 0.0);
        }
    }

    #[test]
    fn hidden_resume_positions_retain_progress_and_position_reads_ignore_unused_progress() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = BookId::from_bytes([7; 32]);
        db.connection.fixtures_insert_book_7(&hash).unwrap();
        db.update_reading_position(&hash, "epubcfi(/6/2)", Some(65.0), None).unwrap();
        db.connection.execute("UPDATE book SET deleted_at=1 WHERE content_hash=?1", [hash]).unwrap();
        assert!(db.reading_position(&hash).unwrap().is_none());
        db.update_reading_position(&hash, "epubcfi(/6/4)", None, None).unwrap();
        assert!(db.reading_position(&hash).unwrap().is_none());
        assert_eq!(db.connection.fixtures_read_book_read_pos_read_progress(&hash, |row| row.get::<_, f32>(1)).unwrap(), 65.0);
        db.connection.execute("UPDATE book SET deleted_at=NULL,read_progress='unreadable' WHERE content_hash=?1", [hash]).unwrap();
        assert_eq!(db.reading_position(&hash).unwrap().as_deref(), Some("epubcfi(/6/4)"));
        assert!(db.update_reading_position(&hash, "epubcfi(/6/6)", None, None).is_err());
        assert_eq!(db.reading_position(&hash).unwrap().as_deref(), Some("epubcfi(/6/4)"));
    }

    #[test]
    fn resetting_one_chapter_keeps_siblings_and_clears_parent_completion() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = BookId::from_bytes([7; 32]);
        db.connection.fixtures_insert_book_7(&hash).unwrap();
        let leaf = |target: &str| BookTocEntry { title: target.into(), target: target.into(), children: vec![] };
        let toc = vec![BookTocEntry { title: "Part".into(), target: "part".into(), children: vec![leaf("one"), leaf("two")] }, leaf("other"), leaf("after")];
        db.connection.navigation_upsert(&hash, 5, &crate::stored_document::encode(&toc).unwrap(), None, None).unwrap();
        for (index, target) in ["one", "two", "other", "after"].into_iter().enumerate() {
            db.update_reading_position(&hash, &format!("epubcfi(/6/{})", (index + 1) * 2), Some(index as f32 * 20.0), Some(target)).unwrap();
        }
        assert!(db.chapter_read_status(&hash).unwrap().completed.contains("part"));
        let position = db.reading_position(&hash).unwrap();
        assert!(db.reset_chapter_read_status_at(&hash, Some("one")).unwrap());
        let status = db.chapter_read_status(&hash).unwrap();
        assert_eq!(status.completed, ["two".into(), "other".into()].into_iter().collect());
        assert_eq!(db.reading_position(&hash).unwrap(), position);
        db.update_reading_position(&hash, position.as_deref().unwrap(), None, Some("after")).unwrap();
        assert_eq!(db.chapter_read_status(&hash).unwrap(), status);
        assert!(db.reset_chapter_read_status_at(&hash, Some("part")).unwrap());
        assert_eq!(db.chapter_read_status(&hash).unwrap().completed, ["other".into()].into_iter().collect());
    }

    #[test]
    fn chapter_read_status_survives_reopen_and_reset_preserves_the_resume_position() {
        let path = std::env::temp_dir().join(format!("chapter-read-{}.sqlite", uuid::Uuid::new_v4()));
        let hash = BookId::from_bytes([7; 32]);
        let toc: Vec<_> = (0..5).map(|index| BookTocEntry { title: format!("Chapter {index}"), target: book_model::audiobook_toc_target(index * 1000), children: vec![] }).collect();
        {
            let db = Database::open(&path).unwrap();
            db.initialize_library().unwrap();
            db.connection.fixtures_insert_book_7(&hash).unwrap();
            db.connection.navigation_upsert(&hash, 5, &crate::stored_document::encode(&toc).unwrap(), Some(5000), None).unwrap();
            db.update_reading_position(&hash, &book_model::audiobook_reading_position(2500), Some(50.0), None).unwrap();
            assert!(db.chapter_read_status(&hash).unwrap().completed.is_empty());
            db.update_reading_position(&hash, &book_model::audiobook_reading_position(4000), Some(80.0), None).unwrap();
            assert_eq!(db.chapter_read_status(&hash).unwrap().completed.into_iter().collect::<Vec<_>>(), [toc[2].target.clone()]);
        }
        {
            let db = Database::open(&path).unwrap();
            assert!(db.chapter_read_status(&hash).unwrap().completed.contains(&toc[2].target));
            let position = db.reading_position(&hash).unwrap();
            assert!(db.reset_chapter_read_status(&hash).unwrap());
            assert_eq!(db.reading_position(&hash).unwrap(), position);
            db.update_reading_position(&hash, position.as_deref().unwrap(), Some(80.0), None).unwrap();
            assert!(db.chapter_read_status(&hash).unwrap().completed.is_empty());
            db.update_reading_position(&hash, &book_model::audiobook_reading_position(5000), Some(100.0), None).unwrap();
            assert_eq!(db.chapter_read_status(&hash).unwrap().completed.into_iter().collect::<Vec<_>>(), [toc[4].target.clone()]);
        }
        std::fs::remove_file(path).unwrap();
    }
}

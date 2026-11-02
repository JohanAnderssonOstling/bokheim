//! Library views: book detail.

use crate::books::toc::{TocSql, entry_title};
use crate::browse::{DetailSql, invalid_integer_column, invalid_text_column, parse_uuid_column};
use crate::query_methods::{first_column, query_last, query_rows};
use crate::{Database, DatabaseError, chapters_from_navigation};
use book_model::{AUTHOR_MARC_RELATOR_CODE, NARRATOR_MARC_RELATOR_CODE};
use description_storage::{Source as DescriptionSource, read as read_description};
use library_model::{BookDetail, BookDetailAuthor, BookTocEntry};
use library_replica::LibraryStateEntry;
use sync_common::{BookId, wire};

impl Database {
    /// Everything one book's page states, in one snapshot rather than a request
    /// per panel: the card, its folder trail, its credits, its navigation, and
    /// where the reader is inside it.
    pub fn browse_book_detail(&self, book_id: &BookId) -> Result<BookDetail, DatabaseError> {
        let hash = &book_id;
        self.with_browse_snapshot(|| {
            let found = query_last(
                |read| self.connection.book_detail_book(hash, read),
                |row| {
                    let chapter_count: Option<i64> = row.get(1)?;
                    let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                    Ok((row.get::<_, i64>(0)?, chapter_count, row.get::<_, Option<Vec<u8>>>(2)?, row.get::<_, Option<i64>>(3)?))
                },
            )?;
            // A book the library does not hold may be in the catalog.
            let Some((id, audiobook_chapter_count, record, duration_ms)) = found else {
                let detail: Result<BookDetail, DatabaseError> = super::catalog::detail(book_id)?.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows.into());
                return self.with_catalog_subjects(detail?);
            };
            let year = year_from_record(record)?;
            let book = self.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            let subjects = self.book_subjects(super::subjects::Books::Library, id)?;

            // Empty when the book has no stored blurb.
            let description = read_description(&self.connection, DescriptionSource::Library, id)?.unwrap_or_default();

            let source_directory_id = first_column::<Option<String>>(|read| self.connection.book_detail_directory(hash, read))?.flatten();
            let path = match source_directory_id.as_deref() {
                Some(directory) => self.dir_path_components(&sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?)?,
                None => Vec::new(),
            };
            let path = path.into_iter().map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name }).collect();

            let authors = query_rows(|read| self.connection.book_detail_authors(hash, AUTHOR_MARC_RELATOR_CODE.0.as_slice(), read), |row| Ok(BookDetailAuthor { id: parse_uuid_column(row.get(0)?, 0)?, name: row.get(1)? }))?;

            // Narrators are the only non-author credits shown in book details.
            let credits = query_rows(
                |read| self.connection.book_detail_credits(hash, NARRATOR_MARC_RELATOR_CODE.0.as_slice(), read),
                |row| {
                    let id: String = row.get(0)?;
                    let role: Vec<u8> = row.get(2)?;
                    Ok(library_model::BookDetailCredit { id: parse_uuid_column(id, 0)?, name: row.get(1)?, role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))? })
                },
            )?;

            let stored_toc = first_column::<Option<Vec<u8>>>(|read| self.connection.book_toc_select(hash, read))?.flatten();
            let mut toc = stored_toc.map(|bytes| crate::stored_document::decode::<Vec<BookTocEntry>>(&bytes)).transpose()?.unwrap_or_default();
            let is_audiobook = book.format_category == library_model::LibraryFileTypeFilter::Audiobook;
            // Chapter intervals use the original stored headings, before
            // display numbering accounts for any non-chapter navigation entries.
            let chapter = if is_audiobook {
                match (self.audiobook_reading_position(book_id)?, duration_ms) {
                    (Some(position), Some(duration_ms)) => chapters_from_navigation(&toc, duration_ms.max(0) as u64).find(|chapter| position >= chapter.start_ms && position < chapter.end_ms),
                    _ => None,
                }
            } else {
                None
            };
            for (index, entry) in toc.iter_mut().enumerate() {
                if book_model::audiobook_toc_position(&entry.target).is_some() {
                    entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                }
            }

            // An audiobook's position is a time; the current entry is the chapter
            // that contains it. Other formats store the entry the reader resolved.
            let (current_entry, current_target) = if is_audiobook {
                match chapter {
                    Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                    None => (None, None),
                }
            } else {
                let target = first_column::<Option<String>>(|read| self.connection.reading_entry_select(hash, read))?.flatten();
                match target {
                    Some(target) => (entry_title(&toc, &target), Some(target)),
                    None => (None, None),
                }
            };

            let mut chapter_read_status = self.chapter_read_status(book_id)?;
            chapter_read_status.update_parents(&toc);
            Ok(BookDetail { book, external_url: None, audiobook_chapter_count, description, subjects, year, source_directory_id, path, authors, credits, current_entry, current_target, toc, chapter_read_status })
        })
    }
}

/// The publication year in a book's addition record: the one the file states
/// wins, and the metadata server's answer stands in where the file has none.
/// None for a book added offline, which has no record until it syncs.
fn year_from_record(record: Option<Vec<u8>>) -> Result<Option<i32>, DatabaseError> {
    let Some(record) = record else { return Ok(None) };
    match wire::decode::<LibraryStateEntry>(&record, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
        LibraryStateEntry::BookAdd { value, .. } => Ok(value.record.book.year),
        _ => Err(DatabaseError::message("a book's year requires an immutable book addition")),
    }
}

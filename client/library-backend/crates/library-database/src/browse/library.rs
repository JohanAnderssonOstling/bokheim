//! Library views: home, authors, detail.

use crate::books::toc::{entry_title, TocSql};
use crate::browse::{AuthorsSql, DetailSql, HomeSql, invalid_integer_column, invalid_text_column};
use crate::{chapters_from_navigation, AudiobookSql, Database, DatabaseError};
use book_model::{AUTHOR_MARC_RELATOR_CODE, AgentId, AuthorId};
use library_model::{AuthorSort, AuthorSummary, BookCardRow, BookDetail, BookDetailAuthor, BookTocEntry, LibraryHomeView};
use std::collections::HashMap;
use sync_common::ContentHash;

/// How far an author's shelf carousel can be paged. The shelf is one row of
/// book cards, so a wide window draws a dozen or so at once and this is a few
/// pages past that; the cap exists because the index loads every author's shelf
/// in one query, not because the row runs out of space.
const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;

impl Database {
    /// Shared application projection for a library home screen. Platform hosts
    /// supply only the user's local-download preference.
    /// A negative limit is rejected; zero returns empty shelves.
    pub fn browse_home(&self, limit: i32, downloaded_only: bool) -> Result<LibraryHomeView, DatabaseError> {
        if limit < 0 {
            return Err(DatabaseError::message("browse home limit must not be negative"));
        }
        let downloaded_only = i32::from(downloaded_only);
        self.with_browse_snapshot(|| {
            // The most recently read books: up to `limit` still in progress, then
            // up to `limit` finished. The two groups arrive in one statement and
            // stay in reading order within each, so a caller splits them by the
            // same threshold the query ranked them with.
            let mut read = Vec::new();
            self.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                read.push(row.get(0)?);
                Ok(())
            })?;
            let mut added = Vec::new();
            self.connection.recently_added_books(limit, downloaded_only, |row| {
                added.push(row.get(0)?);
                Ok(())
            })?;
            Ok(LibraryHomeView { recently_read: self.load_cards(&read)?, recently_added: self.load_cards(&added)? })
        })
    }

    /// The author index: every credited identity with its shelf, already
    /// ordered. One query for the whole page rather than a shelf lookup per
    /// row, so the cost does not scale with the number of authors.
    pub fn browse_authors(&self, sort: AuthorSort, downloaded_only: bool) -> Result<Vec<AuthorSummary>, DatabaseError> {
        let downloaded_only = i32::from(downloaded_only);
        self.with_browse_snapshot(|| {
            // The books each author's shelf draws, keyed by author, in shelf order.
            let (mut shelf_authors, mut ids) = (Vec::new(), Vec::new());
            self.connection.author_shelf_books(downloaded_only, AUTHOR_SHELF_BOOK_COUNT, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                let raw_id: String = row.get(0)?;
                shelf_authors.push(AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?);
                ids.push(row.get(1)?);
                Ok(())
            })?;
            let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
            for (author, card) in shelf_authors.into_iter().zip(self.load_cards(&ids)?) {
                shelves.entry(author).or_default().push(card);
            }

            let mut authors = Vec::new();
            self.connection.get_authors(downloaded_only, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                let raw_id: String = row.get(0)?;
                let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
                let name: String = row.get(1)?;
                let count: i64 = row.get(2)?;
                let count = usize::try_from(count).map_err(|_| invalid_integer_column(2, "author book count must not be negative"))?;
                authors.push((id, name, count));
                Ok(())
            })?;
            let mut authors = authors
                .into_iter()
                .map(|(id, name, book_count)| {
                    let books = shelves.remove(&id).unwrap_or_default();
                    AuthorSummary::try_new(id, name, book_count, books).map_err(|error| invalid_text_column(0, error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            sort.sort(&mut authors);
            Ok(authors)
        })
    }

    /// Everything one book's page states, in one snapshot rather than a request
    /// per panel: the card, its folder trail, its credits, its navigation, and
    /// where the reader is inside it.
    pub fn browse_book_detail(&self, content_hash: &ContentHash) -> Result<BookDetail, DatabaseError> {
        let hash = content_hash.as_str();
        self.with_browse_snapshot(|| {
            let mut found = None;
            self.connection.book_detail_book(hash, |row| {
                let chapter_count: Option<i64> = row.get(1)?;
                let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                found = Some((row.get::<_, i64>(0)?, chapter_count));
                Ok(())
            })?;
            let (id, audiobook_chapter_count) = found.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            let book = self.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;

            // Empty when the book has no stored blurb.
            let mut description = String::new();
            self.connection.book_detail_description(hash, |row| {
                description = row.get(0)?;
                Ok(())
            })?;

            let mut source_directory_id: Option<String> = None;
            self.connection.book_detail_directory(hash, |row| {
                source_directory_id = row.get(0)?;
                Ok(())
            })?;
            let path = match source_directory_id.as_deref() {
                Some(directory) => self.dir_path_components(&sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?)?,
                None => Vec::new(),
            };
            let path = path.into_iter().map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name }).collect();

            let mut authors = Vec::new();
            self.connection.book_detail_authors(hash, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                let id: String = row.get(0)?;
                authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                Ok(())
            })?;

            // Only narrator and translator: a reader picks an audiobook by
            // its narrator and an edition by its translator, and nothing else in the
            // contributor list answers a question a reader is asking.
            let mut credits = Vec::new();
            let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
            let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
            self.connection.book_detail_credits(hash, narrator, translator, |row| {
                let id: String = row.get(0)?;
                let role: Vec<u8> = row.get(2)?;
                credits.push(library_model::BookDetailCredit {
                    id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                    name: row.get(1)?,
                    role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                });
                Ok(())
            })?;

            let mut stored_toc: Option<String> = None;
            self.connection.book_toc_select(hash, |row| {
                stored_toc = row.get(0)?;
                Ok(())
            })?;
            let mut toc = stored_toc.map(|json| serde_json::from_str::<Vec<BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))).transpose()?.unwrap_or_default();
            for (index, entry) in toc.iter_mut().enumerate() {
                if book_model::audiobook_toc_position(&entry.target).is_some() {
                    entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                }
            }

            // An audiobook's position is a time; the current entry is the chapter
            // that contains it. Other formats store the entry the reader resolved.
            let (current_entry, current_target) = if book.format_category == library_model::LibraryFileTypeFilter::Audiobook {
                let mut chapters: Option<(String, i64)> = None;
                self.connection.audiobook_chapters_select(hash, |row| {
                    chapters = Some((row.get(0)?, row.get(1)?));
                    Ok(())
                })?;
                let chapter = match (self.audiobook_reading_position(content_hash)?, chapters) {
                    (Some(position), Some((toc_json, duration_ms))) => {
                        let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                        chapters_from_navigation(&entries, duration_ms.max(0) as u64).into_iter().find(|chapter| position >= chapter.start_ms && position < chapter.end_ms)
                    }
                    _ => None,
                };
                match chapter {
                    Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                    None => (None, None),
                }
            } else {
                let mut target: Option<String> = None;
                self.connection.reading_entry_select(hash, |row| {
                    target = row.get(0)?;
                    Ok(())
                })?;
                match target {
                    Some(target) => (entry_title(&toc, &target), Some(target)),
                    None => (None, None),
                }
            };

            Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
        })
    }
}

//! Library views: home, authors, detail, counts.

use crate::books::toc::TocSql;
use crate::books::toc::entry_title;
use crate::browse::{AuthorsSql, CountsSql, DetailSql, HomeSql, invalid_integer_column, invalid_text_column};
use crate::{Database, DatabaseError};
use book_model::{AUTHOR_MARC_RELATOR_CODE, AgentId, AuthorId};
use library_model::{AuthorSort, AuthorSummary, BookCardRow, BookDetail, BookDetailAuthor, LibraryHomeView};
use std::collections::HashMap;
use sync_common::ContentHash;

/// How far an author's shelf carousel can be paged. The shelf is one row of
/// book cards, so a wide window draws a dozen or so at once and this is a few
/// pages past that; the cap exists because the index loads every author's shelf
/// in one query, not because the row runs out of space.
const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;
struct SourceLocation {
    source_directory_id: Option<String>,
    path: Vec<library_model::BrowsePathSegment>,
}

impl Database {
    /// Number of visible books with at least one live library placement.
    ///
    /// Retained metadata rows and books that only remain in Trash do not make a
    /// library non-empty.
    pub fn browse_book_count(&self) -> Result<usize, DatabaseError> {
        let mut count = 0_i64;
        self.connection.count_all_books(|row| {
            count = row.get(0)?;
            Ok(())
        })?;
        Ok(usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))?)
    }

    /// Shared application projection for a library home screen. Platform hosts
    /// supply only the user's local-download preference.
    /// A negative limit is rejected; zero returns empty shelves.
    pub fn browse_home(&self, limit: i32, downloaded_only: bool) -> Result<LibraryHomeView, DatabaseError> {
        if limit < 0 {
            return Err(DatabaseError::message("browse home limit must not be negative"));
        }
        self.with_browse_snapshot(|| {
            Ok(LibraryHomeView {
                most_progress: self.get_most_progress_cards(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD))?,
                recently_read: self.get_recently_read_cards(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD))?,
                recently_added: self.get_recently_added_cards(limit, downloaded_only)?,
            })
        })
    }

    pub fn browse_authors(&self, sort: AuthorSort, downloaded_only: bool) -> Result<Vec<AuthorSummary>, DatabaseError> {
        self.with_browse_snapshot(|| {
            let mut shelves = self.get_author_book_cards_scoped(downloaded_only, AUTHOR_SHELF_BOOK_COUNT)?;
            let mut authors = self
                .get_authors_scoped(downloaded_only)?
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

    pub fn browse_book_detail(&self, content_hash: ContentHash) -> Result<BookDetail, DatabaseError> {
        self.with_browse_snapshot(|| {
            let (book, audiobook_chapter_count) = self.read_detail_card(&content_hash)?;
            let description = self.read_detail_description(&content_hash)?;
            let SourceLocation { source_directory_id, path } = self.read_detail_source(&content_hash)?;
            let authors = self.read_detail_authors(&content_hash)?;
            let credits = self.read_detail_credits(&content_hash)?;
            let toc = self.browse_book_navigation(&content_hash)?;
            let (current_entry, current_target) = self.browse_current_entry(&content_hash, book.format_category, &toc)?;
            Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
        })
    }

    fn read_detail_card(&self, content_hash: &ContentHash) -> Result<(BookCardRow, Option<usize>), DatabaseError> {
        let mut book = None;
        self.connection.book_detail_book(content_hash.as_str(), |row| {
            let chapter_count: Option<i64> = row.get(1)?;
            let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
            book = Some((row.get::<_, i64>(0)?, chapter_count));
            Ok(())
        })?;
        let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let card = self.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        Ok((card, chapter_count))
    }

    /// Empty when the book has no stored blurb.
    fn read_detail_description(&self, content_hash: &ContentHash) -> Result<String, DatabaseError> {
        let mut description = String::new();
        self.connection.book_detail_description(content_hash.as_str(), |row| {
            description = row.get(0)?;
            Ok(())
        })?;
        Ok(description)
    }

    fn read_detail_source(&self, content_hash: &ContentHash) -> Result<SourceLocation, DatabaseError> {
        let mut source_directory_id: Option<String> = None;
        self.connection.book_detail_directory(content_hash.as_str(), |row| {
            source_directory_id = row.get(0)?;
            Ok(())
        })?;

        let path: Vec<library_model::BrowsePathSegment> = source_directory_id
            .as_deref()
            .map(|directory| {
                let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                self.dir_path_components(&directory)
            })
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|component| library_model::BrowsePathSegment { id: component.id.to_string(), name: component.name.clone() })
            .collect();

        Ok(SourceLocation { source_directory_id, path })
    }

    fn read_detail_authors(&self, content_hash: &ContentHash) -> Result<Vec<BookDetailAuthor>, DatabaseError> {
        let mut authors = Vec::new();
        self.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
            let id: String = row.get(0)?;
            authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
            Ok(())
        })?;

        Ok(authors)
    }

    fn read_detail_credits(&self, content_hash: &ContentHash) -> Result<Vec<library_model::BookDetailCredit>, DatabaseError> {
        // Only narrator and translator: a reader picks an audiobook by
        // its narrator and an edition by its translator, and nothing else in the
        // contributor list answers a question a reader is asking.
        let mut credits = Vec::new();
        let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
        let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
        self.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
            let id: String = row.get(0)?;
            let role: Vec<u8> = row.get(2)?;
            credits.push(library_model::BookDetailCredit {
                id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                name: row.get(1)?,
                role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
            });
            Ok(())
        })?;

        Ok(credits)
    }

    fn get_authors_scoped(&self, downloaded_only: bool) -> Result<Vec<(AuthorId, String, usize)>, DatabaseError> {
        let mut authors = Vec::new();
        self.connection.get_authors(i32::from(downloaded_only), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
            let raw_id: String = row.get(0)?;
            let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
            let name: String = row.get(1)?;
            let count: i64 = row.get(2)?;
            let count = usize::try_from(count).map_err(|_| invalid_integer_column(2, "author book count must not be negative"))?;
            authors.push((id, name, count));
            Ok(())
        })?;
        Ok(authors)
    }
    /// The books each author's shelf draws, keyed by author, in shelf order.

    fn get_author_book_cards_scoped(&self, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<BookCardRow>>, DatabaseError> {
        let (mut authors, mut ids) = (Vec::new(), Vec::new());
        self.connection.author_shelf_books(i32::from(downloaded_only), per_author, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
            let raw_id: String = row.get(0)?;
            authors.push(AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?);
            ids.push(row.get(1)?);
            Ok(())
        })?;
        let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
        for (author, card) in authors.into_iter().zip(self.load_cards(&ids)?) {
            shelves.entry(author).or_default().push(card);
        }
        Ok(shelves)
    }
    /// The most recently read books: up to `limit` still in progress, then up to
    /// `limit` finished. The two groups arrive in one statement and stay in
    /// reading order within each, so a caller splits them by the same threshold
    /// the query ranked them with.
    fn get_recently_read_cards(&self, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut ids = Vec::new();
        self.connection.recently_read_books(limit, i32::from(downloaded_only), finished_threshold, |row| {
            ids.push(row.get(0)?);
            Ok(())
        })?;
        self.load_cards(&ids)
    }

    fn get_most_progress_cards(&self, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut ids = Vec::new();
        self.connection.most_progress_books(limit, i32::from(downloaded_only), finished_threshold, |row| {
            ids.push(row.get(0)?);
            Ok(())
        })?;
        self.load_cards(&ids)
    }

    fn get_recently_added_cards(&self, limit: i32, downloaded_only: bool) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut ids = Vec::new();
        self.connection.recently_added_books(limit, i32::from(downloaded_only), |row| {
            ids.push(row.get(0)?);
            Ok(())
        })?;
        self.load_cards(&ids)
    }

    fn browse_current_entry(&self, content_hash: &ContentHash, format: library_model::LibraryFileTypeFilter, toc: &[library_model::BookTocEntry]) -> Result<(Option<String>, Option<String>), DatabaseError> {
        if format == library_model::LibraryFileTypeFilter::Audiobook {
            let Some(position) = self.audiobook_reading_position(content_hash)? else { return Ok((None, None)) };
            let chapter = self.audiobook_chapters(content_hash)?.into_iter().find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
            return Ok(match chapter {
                Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                None => (None, None),
            });
        }
        let mut target: Option<String> = None;
        self.connection.reading_entry_select(content_hash.as_str(), |row| {
            target = row.get(0)?;
            Ok(())
        })?;
        let Some(target) = target else { return Ok((None, None)) };
        Ok((entry_title(toc, &target), Some(target)))
    }

    fn browse_book_navigation(&self, content_hash: &ContentHash) -> Result<Vec<library_model::BookTocEntry>, DatabaseError> {
        let mut stored: Option<String> = None;
        self.connection.book_toc_select(content_hash.as_str(), |row| {
            stored = row.get(0)?;
            Ok(())
        })?;
        let mut entries =
            stored.map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))).transpose()?.unwrap_or_default();
        for (index, entry) in entries.iter_mut().enumerate() {
            if book_model::audiobook_toc_position(&entry.target).is_some() {
                entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
            }
        }
        Ok(entries)
    }
}

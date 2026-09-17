//! Read-side views: browsing, navigation, playback, marks.


// ---- browse ----
use include_sqlite_sql::include_sql;
use crate::{Database, DatabaseError};
use book_model::AgentId;
use book_model::AuthorId;
use library_model::BookCardRow;
use subject_projection::ROOT_SUBJECT_PATH;
use library_model::{AuthorSort, AuthorSummary, BookDetail, BookDetailAuthor, BrowseBookSort, BrowseChipSort, BrowseRow, LibraryFileTypeFilter, LibraryFormatCount, LibraryHomeView, LibraryLanguageCount, LibrarySearchResult, SubjectPath};
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::collections::HashSet;
use sync_common::ContentHash;
use sync_common::DirId;

#[derive(Clone, serde::Serialize)]
pub struct LibraryAuthorsView {
    pub authors: Vec<AuthorSummary>,
    pub books: Option<Vec<BookCardRow>>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryHierarchyEntry<Id> {
    pub id: Id,
    pub name: String,
    pub book_count: usize,
    pub downloaded_book_count: usize,
}

#[derive(Clone, serde::Serialize)]
pub struct LibraryHierarchyView<Id> {
    pub children: Vec<LibraryHierarchyEntry<Id>>,
    pub books: Vec<BookCardRow>,
}

pub type LibraryFolderEntry = LibraryHierarchyEntry<DirId>;
pub type LibrarySubjectEntry = LibraryHierarchyEntry<SubjectPath>;
pub type LibraryFolderView = LibraryHierarchyView<DirId>;
pub type LibrarySubjectView = LibraryHierarchyView<SubjectPath>;

fn invalid_text_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn invalid_integer_column(column: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())))
}

fn parse_uuid_column(value: String, column: usize) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(&value).map_err(|error| invalid_text_column(column, error.to_string()))
}

/// Complete library-owned browse projection returned to any UI transport.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct LibraryBrowseData {
    #[serde(default)]
    pub chip_groups: Vec<library_model::BrowseChipGroup>,
    pub contents: library_model::BrowseContents,
    pub path: Vec<library_model::BrowsePathSegment>,
    pub language_counts: Vec<library_model::LibraryLanguageCount>,
    pub format_counts: Vec<library_model::LibraryFormatCount>,
}




// Virtual roots are not prefixes of curated paths. Always resolve their
// children first, even when a top-level label is shorter than "Subject".
fn read_book_card_row(row: &rusqlite::Row<'_>) -> Result<BookCardRow, rusqlite::Error> {
    let raw_content_hash: String = row.get(0)?;
    let content_hash = ContentHash::new(&raw_content_hash.clone());
    let title: Option<String> = row.get(1).ok();
    let title = title.unwrap_or_else(|| format!("Untitled ({raw_content_hash})"));
    let author: Option<String> = row.get(2).ok();
    let description: Option<String> = row.get(3)?;
    let progress: Option<f32> = row.get(4)?;
    let duration_ms: Option<i64> = row.get(8)?;
    let chapter_count: Option<i64> = row.get(9)?;
    Ok(BookCardRow {
        content_hash,
        title,
        subtitle: row.get("subtitle")?,
        author: author.unwrap_or_default(),
        description: description.unwrap_or_default(),
        progress: progress.unwrap_or_default(),
        downloaded: row.get(5)?,
        download_requested: row.get(6)?,
        format_category: match row.get::<_, i64>(7)? {
            1 => library_model::LibraryFileTypeFilter::Book,
            2 => library_model::LibraryFileTypeFilter::Pdf,
            4 => library_model::LibraryFileTypeFilter::Audiobook,
            value => return Err(invalid_integer_column(7, format!("invalid book format category {value}"))),
        },
        audiobook_duration_ms: duration_ms.and_then(|value| u64::try_from(value).ok()),
        audiobook_chapter_count: chapter_count.and_then(|value| usize::try_from(value).ok()),
        added_at: row.get(10).unwrap_or_default(),
        source_directory: row.get::<_, Option<String>>("source_directory").ok().flatten().and_then(|value| DirId::parse_str(&value).ok()),
    })
}





fn subject_path_depth(path: &str) -> usize {
    if path.is_empty() {
        0
    } else if path == ROOT_SUBJECT_PATH {
        1
    } else {
        path.split(" / ").count() + 1
    }
}

const PLACES_SUBJECT_PATH: &str = "__places__";
/// How far an author's shelf carousel can be paged. The shelf is one row of
/// book cards, so a wide window draws a dozen or so at once and this is a few
/// pages past that; the cap exists because the index loads every author's shelf
/// in one query, not because the row runs out of space.
const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;

/// How many subject chips an author's row states. The chips share the heading
/// line with the name, so this is what fits beside it rather than everything an
/// author has been filed under.
const AUTHOR_SUBJECT_COUNT: i32 = 3;




fn read_language_facet_rows(run: impl FnOnce(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>) -> Result<Vec<LibraryLanguageCount>, rusqlite::Error> {
    let mut counts = Vec::new();
    run(&mut |row| {
        let count = row.get::<_, i64>(1)?;
        counts.push(LibraryLanguageCount { language: row.get(0)?, book_count: u32::try_from(count).map_err(|_| invalid_integer_column(1, "language facet count must fit in u32"))? });
        Ok(())
    })?;
    Ok(counts)
}

fn read_format_facet_rows(run: impl FnOnce(&mut dyn FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>) -> Result<Vec<LibraryFormatCount>, rusqlite::Error> {
    let mut counts = Vec::new();
    run(&mut |row| {
        let format = match row.get::<_, i64>(0)? {
            1 => LibraryFileTypeFilter::Book,
            2 => LibraryFileTypeFilter::Pdf,
            4 => LibraryFileTypeFilter::Audiobook,
            _ => return Err(invalid_integer_column(0, "unknown browse format category")),
        };
        let count = row.get::<_, i64>(1)?;
        counts.push(LibraryFormatCount { format, book_count: u32::try_from(count).map_err(|_| invalid_integer_column(1, "format facet count must fit in u32"))? });
        Ok(())
    })?;
    Ok(counts)
}





fn retain_selected_facet_choices(language_counts: &mut Vec<LibraryLanguageCount>, format_counts: &mut Vec<LibraryFormatCount>, languages: &str, file_types: i32) {
    for language in languages.trim_matches(',').split(',').filter(|language| !language.is_empty()) {
        if !language_counts.iter().any(|count| count.language == language) {
            language_counts.push(LibraryLanguageCount { language: language.to_owned(), book_count: 0 });
        }
    }
    for (mask, format) in [(1, LibraryFileTypeFilter::Book), (2, LibraryFileTypeFilter::Pdf), (4, LibraryFileTypeFilter::Audiobook)] {
        if file_types & mask != 0 && !format_counts.iter().any(|count| count.format == format) {
            format_counts.push(LibraryFormatCount { format, book_count: 0 });
        }
    }
    language_counts.sort_by(|left, right| left.language.cmp(&right.language));
    format_counts.sort_by_key(|count| file_type_query_value(count.format));
}

fn file_type_query_value(filter: LibraryFileTypeFilter) -> i32 {
    match filter {
        LibraryFileTypeFilter::All => 0,
        LibraryFileTypeFilter::Book => 1,
        LibraryFileTypeFilter::Pdf => 2,
        LibraryFileTypeFilter::Audiobook => 4,
    }
}

fn file_types_query_value(filters: &[LibraryFileTypeFilter]) -> i32 {
    if filters.is_empty() || filters.contains(&LibraryFileTypeFilter::All) {
        return 0;
    }
    filters.iter().fold(0, |mask, filter| mask | file_type_query_value(*filter))
}

fn languages_query_value(languages: &[&str]) -> String {
    if languages.is_empty() {
        return String::new();
    }
    let mut languages = languages.iter().map(|language| language.trim().to_lowercase()).filter(|language| !language.is_empty()).collect::<Vec<_>>();
    languages.sort();
    languages.dedup();
    format!(",{},", languages.join(","))
}

const fn chip_sort_query_value(sort: BrowseChipSort) -> i32 {
    match sort {
        BrowseChipSort::Alphabetical => 0,
        BrowseChipSort::BookCount => 1,
    }
}

const fn book_sort_query_value(sort: BrowseBookSort) -> i32 {
    match sort {
        BrowseBookSort::Alphabetical => 0,
        BrowseBookSort::RecentlyRead => 1,
    }
}

struct FileTypeScope {
    filter: LibraryFileTypeFilter,
    matching: HashSet<ContentHash>,
    downloaded: HashSet<ContentHash>,
}

impl FileTypeScope {

    fn is_all(&self) -> bool {
        self.filter == LibraryFileTypeFilter::All
    }

    fn contains(&self, content_hash: &ContentHash) -> bool {
        self.matching.contains(content_hash)
    }

    fn retain_books(&self, books: &mut Vec<BookCardRow>) {
        if !self.is_all() {
            books.retain(|book| self.matching.contains(&book.content_hash));
        }
    }

    fn counts(&self, hashes: impl IntoIterator<Item = ContentHash>) -> (usize, usize) {
        let hashes = hashes.into_iter().collect::<HashSet<_>>();
        (hashes.intersection(&self.matching).count(), hashes.intersection(&self.downloaded).count())
    }
}















fn external_path_id<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    path.strip_prefix(root)?.strip_prefix('/')?.rsplit('/').next().filter(|id| {
        (id.starts_with('Q') && id[1..].bytes().all(|byte| byte.is_ascii_digit())) || (["OLP-", "OLR-"].iter().any(|prefix| id.starts_with(prefix)) && id[4..].len() % 2 == 0 && id[4..].bytes().all(|byte| byte.is_ascii_hexdigit()))
    })
}







fn retain_search_results(result: &mut LibrarySearchResult, allowed: &HashSet<ContentHash>) {
    result.title_matches.retain(|book| allowed.contains(&book.content_hash));
    result.author_matches.retain(|book| allowed.contains(&book.content_hash));
}











/// Storage-backed portion of a book detail, including the book's navigation:
/// a document's is whatever import stored, an audiobook's is its chapters
/// projected into the same hierarchical form. A document with no stored row has
/// not been inspected for one yet, and comes back empty rather than wrong — the
/// caller may parse the file and store the result.
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
        Ok(usize::try_from(count).map_err(|_| DatabaseError::operation("library book count must not be negative"))?)
    }
    /// Counts visible books by normalized base language across the whole library.
    /// A book with several regional variants of the same language is counted once.
    fn library_language_counts(&self) -> Result<Vec<LibraryLanguageCount>, DatabaseError> {
        let mut rows = Vec::new();
        self.connection.library_language_counts_with(|row| {
            rows.push((row.get::<_, String>(0)?, row.get::<_, i64>(1)?));
            Ok(())
        })?;
        Ok(rows.into_iter().map(|row| {
            let (language, count) = row;
            let book_count = u32::try_from(count).map_err(|_| invalid_integer_column(1, "language book count must fit in u32"))?;
            Ok(LibraryLanguageCount { language, book_count })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Counts visible books using the same broad format categories as browsing.
    fn library_format_counts(&self) -> Result<Vec<LibraryFormatCount>, DatabaseError> {
        let mut rows = Vec::new();
        self.connection.library_format_counts_select(|row| {
            rows.push((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?));
            Ok(())
        })?;
        Ok(rows.into_iter().map(|row| {
            let (format_category, count) = row;
            let format = match format_category {
                1 => LibraryFileTypeFilter::Book,
                2 => LibraryFileTypeFilter::Pdf,
                3 => LibraryFileTypeFilter::Audiobook,
                _ => return Err(invalid_integer_column(0, "unknown library format category")),
            };
            let book_count = u32::try_from(count).map_err(|_| invalid_integer_column(1, "format book count must fit in u32"))?;
            Ok(LibraryFormatCount { format, book_count })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn get_book_cards_in_dir_scoped(&self, dir_id: &str, downloaded_only: bool, normalized_query: &str, file_type: library_model::LibraryFileTypeFilter) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.get_book_cards_in_dir_sorted_scoped( dir_id, downloaded_only, normalized_query, file_type, library_model::BrowseBookSort::Alphabetical)
    }
    fn get_book_cards_in_dir_sorted_scoped(&self, dir_id: &str, downloaded_only: bool, normalized_query: &str, file_type: library_model::LibraryFileTypeFilter, book_sort: library_model::BrowseBookSort,
    ) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.get_book_cards_in_dir_filtered_sorted_scoped( dir_id, downloaded_only, normalized_query, &[file_type], book_sort, &[])
    }
    fn get_book_cards_in_dir_filtered_sorted_scoped(&self, dir_id: &str, downloaded_only: bool, normalized_query: &str, file_types: &[library_model::LibraryFileTypeFilter], book_sort: library_model::BrowseBookSort, languages: &[&str],
    ) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut cards = Vec::new();
        let languages = languages_query_value(languages);
        self.connection.get_book_cards_in_dir(dir_id, i32::from(downloaded_only), normalized_query, file_types_query_value(file_types), book_sort_query_value(book_sort), &languages, |row| {
            cards.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(cards)
    }
    /// A book placed in several folders yields one row per placement, so results
    /// are collapsed to one card per book.
    fn get_book_cards_by_author_scoped(&self, author_id: AuthorId, downloaded_only: bool) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut cards = Vec::new();
        let mut seen = std::collections::HashSet::new();
        self.connection.get_book_cards_by_author(&author_id.to_string(), i32::from(downloaded_only), |row| {
            let card = read_book_card_row(row)?;
            if seen.insert(card.content_hash.clone()) {
                cards.push(card);
            }
            Ok(())
        })?;
        Ok(cards)
    }
    pub fn browse_folder_facet_counts(
            &self,
            folder_id: &str,
            query: &str,
            file_types: &[LibraryFileTypeFilter],
            languages: &[&str],
            downloaded_only: bool,
        ) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), DatabaseError> {
            let folder_id = DirId::parse_str(folder_id).map_err(DatabaseError::operation)?.to_string();
            let normalized_query = catalog_model::normalize_search_text(query);
            let file_types = file_types_query_value(file_types);
        let languages = languages_query_value(languages);
        let mut language_counts = read_language_facet_rows(|rows| self.connection.get_folder_language_facets(&folder_id, &normalized_query, file_types, i32::from(downloaded_only), rows))?;
        let mut format_counts = read_format_facet_rows(|rows| self.connection.get_folder_format_facets(&folder_id, &normalized_query, &languages, i32::from(downloaded_only), rows))?;
        retain_selected_facet_choices(&mut language_counts, &mut format_counts, &languages, file_types);
        Ok((language_counts, format_counts))
    }
    pub fn browse_subject_facet_counts(
            &self,
            subject_path: &str,
            query: &str,
            file_types: &[LibraryFileTypeFilter],
            languages: &[&str],
            downloaded_only: bool,
        ) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), DatabaseError> {
            let normalized_query = catalog_model::normalize_search_text(query);
            self.ensure_projection()?;
        let mut dirty = false;
        self.connection.subject_facet_counts_scoped_select(|row| {
            dirty = row.get(0)?;
            Ok(())
        })?;
        if dirty {
            // Keep the derived counts and their dirty flag in one main-database
            // write transaction so an import cannot be lost between those writes.
            let transaction = self.connection.unchecked_transaction()?;
            transaction.subject_facet_counts_scoped_update_2()?;
            transaction.rebuild_subject_book_ownership()?;
            transaction.ensure_projection_update_2()?;
            transaction.commit()?;
        }
        let file_types = file_types_query_value(file_types);
        let languages = languages_query_value(languages);
        let mut language_counts = read_language_facet_rows(|rows| self.connection.get_subject_language_facets(subject_path, &normalized_query, file_types, i32::from(downloaded_only), rows))?;
        let mut format_counts = read_format_facet_rows(|rows| self.connection.get_subject_format_facets(subject_path, &normalized_query, &languages, i32::from(downloaded_only), rows))?;
        retain_selected_facet_choices(&mut language_counts, &mut format_counts, &languages, file_types);
        Ok((language_counts, format_counts))
    }
    fn read_file_type_scope(&self, filter: LibraryFileTypeFilter) -> Result<FileTypeScope, DatabaseError> {
            let mut matching = HashSet::new();
            let mut downloaded = HashSet::new();
            let mut rows = Vec::new();
            self.connection.load_select(|row| {
                rows.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?));
                Ok(())
            })?;
            for row in rows {
                let (hash, format, is_downloaded) = row;
                if filter.matches_extension(&format) {
                    let hash = ContentHash::new(&hash);
                    if is_downloaded {
                        downloaded.insert(hash.clone());
                    }
                    matching.insert(hash);
                }
            }
            Ok(FileTypeScope { filter, matching, downloaded })
        }
    /// Legacy entry point for the unsearched view; returns actual children and
    /// directly assigned books, with no compaction or promotion.
    fn compact_subject_children(&self, parent_path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<(Vec<LibrarySubjectEntry>, Vec<BookCardRow>), DatabaseError> {
        let query = library_model::LibraryBrowseQuery {
            location: parent_path.to_owned(),
            search: String::new(),
            file_types: vec![scope.filter],
            chip_sort: BrowseChipSort::Alphabetical,
            book_sort: BrowseBookSort::Alphabetical,
            languages: Vec::new(),
            hide_finished: false,
            include_direct_child_books: false,
        };
        let contents = self.browse_subject_contents(&query, downloaded_only)?;
        let children = contents.children.into_iter().map(|child| LibrarySubjectEntry { id: SubjectPath::new(child.id), name: child.name, book_count: child.book_count, downloaded_book_count: child.downloaded_book_count }).collect();
        Ok((children, contents.books))
    }
    fn get_folder_list_rows_filtered_sorted(&self, parent_id: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, languages: &[&str]) -> Result<Vec<BrowseRow>, DatabaseError> {
        let mut folders = Vec::new();
        let languages = languages_query_value(languages);
        let file_types = file_types_query_value(file_types);
        let mut append_row = |row: &rusqlite::Row<'_>| {
            let raw_id: String = row.get(0)?;
            let raw_book_count: i64 = row.get(2)?;
            let raw_downloaded_book_count: i64 = row.get(3)?;
            folders.push(BrowseRow {
                id: parse_uuid_column(raw_id, 0)?.to_string(),
                name: row.get(1)?,
                path: row.get::<_, Option<String>>(4)?.filter(|path| !path.is_empty()),
                book_count: usize::try_from(raw_book_count).map_err(|_| invalid_integer_column(2, "folder book count must not be negative"))?,
                downloaded_book_count: usize::try_from(raw_downloaded_book_count).map_err(|_| invalid_integer_column(3, "downloaded folder book count must not be negative"))?,
            });
            Ok(())
        };
        if normalized_query.is_empty() && file_types == 0 && languages.is_empty() {
            self.connection.get_immediate_sub_dirs(parent_id, chip_sort_query_value(chip_sort), &mut append_row)?;
        } else {
            self.connection.get_sub_dirs(parent_id, normalized_query, file_types, chip_sort_query_value(chip_sort), &languages, &mut append_row)?;
        }
        Ok(folders)
    }
    fn get_subject_browse_rows_filtered_sorted(&self, parent_path: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, languages: &[&str]) -> Result<Vec<BrowseRow>, DatabaseError> {
        let mut subjects = Vec::new();
        let languages = languages_query_value(languages);
        self.connection.get_subject_children(parent_path, normalized_query, file_types_query_value(file_types), chip_sort_query_value(chip_sort), &languages, |row| {
            let raw_book_count: i64 = row.get(2)?;
            let raw_downloaded_book_count: i64 = row.get(3)?;
            subjects.push(BrowseRow {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get::<_, Option<String>>(4)?.filter(|path| !path.is_empty()),
                book_count: usize::try_from(raw_book_count).map_err(|_| invalid_integer_column(2, "subject book count must not be negative"))?,
                downloaded_book_count: usize::try_from(raw_downloaded_book_count).map_err(|_| invalid_integer_column(3, "downloaded subject book count must not be negative"))?,
            });
            Ok(())
        })?;
        Ok(subjects)
    }
    fn get_subject_children(&self, parent_path: &str) -> Result<Vec<LibrarySubjectEntry>, DatabaseError> {
        let mut subjects = Vec::new();
        self.connection.get_subject_children(parent_path, "", file_type_query_value(LibraryFileTypeFilter::All), chip_sort_query_value(BrowseChipSort::Alphabetical), "", |row| {
            let raw_book_count: i64 = row.get(2)?;
            let raw_downloaded_book_count: i64 = row.get(3)?;
            subjects.push(LibrarySubjectEntry {
                id: SubjectPath::new(row.get::<_, String>(0)?),
                name: row.get(1)?,
                book_count: usize::try_from(raw_book_count).map_err(|_| invalid_integer_column(2, "subject book count must not be negative"))?,
                downloaded_book_count: usize::try_from(raw_downloaded_book_count).map_err(|_| invalid_integer_column(3, "downloaded subject book count must not be negative"))?,
            });
            Ok(())
        })?;
        Ok(subjects)
    }
    fn get_book_cards_in_subject_scoped(&self, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_type: LibraryFileTypeFilter) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.get_book_cards_in_subject_sorted_scoped( subject_path, downloaded_only, normalized_query, file_type, BrowseBookSort::Alphabetical)
    }
    fn get_book_cards_in_subject_sorted_scoped(&self, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_type: LibraryFileTypeFilter, book_sort: BrowseBookSort) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.get_book_cards_in_subject_filtered_sorted_scoped( subject_path, downloaded_only, normalized_query, &[file_type], book_sort, &[])
    }
    fn get_book_cards_in_subject_filtered_sorted_scoped(&self, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_types: &[LibraryFileTypeFilter], book_sort: BrowseBookSort, languages: &[&str],
    ) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.subject_book_cards( subject_path, downloaded_only, normalized_query, file_types, book_sort, languages, false)
    }
    fn subject_book_cards(&self, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_types: &[LibraryFileTypeFilter], book_sort: BrowseBookSort, languages: &[&str], include_descendants: bool,
    ) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.ensure_projection()?;
        let mut cards = Vec::new();
        let languages = languages_query_value(languages);
        self.connection.get_book_cards_in_subject(subject_path, i32::from(downloaded_only), normalized_query, file_types_query_value(file_types), book_sort_query_value(book_sort), &languages, i32::from(include_descendants), |row| {
            cards.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(cards)
    }
    fn get_descendant_content_hashes(&self, dir_id: &str) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut content_hashes = Vec::new();
        self.connection.get_descendant_content_hashes(dir_id, |row| {
            let raw: String = row.get(0)?;
            content_hashes.push(ContentHash::new(&raw));
            Ok(())
        })?;
        Ok(content_hashes)
    }
    fn get_descendant_subject_content_hashes(&self, subject_path: &str) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut content_hashes = Vec::new();
        self.connection.get_descendant_subject_content_hashes(subject_path, |row| {
            let raw: String = row.get(0)?;
            content_hashes.push(ContentHash::new(&raw));
            Ok(())
        })?;
        Ok(content_hashes)
    }
    fn external_kind_counts(&self, kind: &str) -> Result<(usize, usize), DatabaseError> {
        let mut total = 0;
        let mut downloaded = 0;
        self.connection.external_kind_counts_select(kind, |row| {
            total = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "external entity book count must not be negative"))?;
            downloaded = usize::try_from(row.get::<_, i64>(1)?).map_err(|_| invalid_integer_column(1, "external entity downloaded count must not be negative"))?;
            Ok(())
        })?;
        Ok((total, downloaded))
    }
    fn external_entity_counts(&self, wikidata_id: &str, recursive_places: bool) -> Result<(usize, usize), DatabaseError> {
        let mut counts = (0, 0);
        self.connection.get_external_entity_book_counts(wikidata_id, i32::from(recursive_places), |row| {
            counts = (
                usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "external entity book count must not be negative"))?,
                usize::try_from(row.get::<_, i64>(1)?).map_err(|_| invalid_integer_column(1, "external entity downloaded count must not be negative"))?,
            );
            Ok(())
        })?;
        Ok(counts)
    }
    fn external_entity_books(&self, wikidata_id: &str, downloaded_only: bool) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut books = Vec::new();
        self.connection.get_book_cards_for_external_entity(wikidata_id, i32::from(downloaded_only), |row| {
            books.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(books)
    }
    fn external_places(&self, path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<LibrarySubjectView, DatabaseError> {
        if let Some(wikidata_id) = external_path_id(path, PLACES_SUBJECT_PATH) {
            let mut books = self.external_entity_books( wikidata_id, downloaded_only)?;
            scope.retain_books(&mut books);
            return Ok(LibrarySubjectView { children: Vec::new(), books });
        }
        let mut raw_places = Vec::<(String, String)>::new();
        self.connection.get_external_places(|row| {
            raw_places.push((row.get(0)?, row.get(1)?));
            Ok(())
        })?;
        let mut children = Vec::new();
        for (wikidata_id, name) in raw_places {
            let (mut book_count, downloaded_book_count) = if scope.is_all() { self.external_entity_counts( &wikidata_id, false)? } else { scope.counts(self.get_external_entity_content_hashes( &wikidata_id)?) };
            if downloaded_only {
                book_count = downloaded_book_count;
            }
            if book_count > 0 {
                children.push(LibrarySubjectEntry { id: SubjectPath::new(format!("{PLACES_SUBJECT_PATH}/{wikidata_id}")), name, book_count, downloaded_book_count });
            }
        }
        Ok(LibrarySubjectView { children, books: Vec::new() })
    }
    fn get_external_entity_content_hashes(&self, wikidata_id: &str) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut content_hashes = Vec::new();
        self.connection.get_external_entity_content_hashes_select(wikidata_id, |row| {
            content_hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(content_hashes)
    }
    fn external_kind_hashes(&self, kind: &str) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut content_hashes = Vec::new();
        self.connection.external_kind_hashes_select(kind, |row| {
            content_hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(content_hashes)
    }
    /// The most recently read books: up to `limit` still in progress, then up to
    /// `limit` finished. The two groups arrive in one statement and stay in
    /// reading order within each, so a caller splits them by the same threshold
    /// the query ranked them with.
    fn get_recently_read_cards(&self, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut cards = Vec::new();
        self.connection.get_recently_read_cards(limit, i32::from(downloaded_only), finished_threshold, |row| {
            cards.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(cards)
    }
    fn get_most_progress_cards(&self, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut cards = Vec::new();
        self.connection.get_most_progress_cards(limit, i32::from(downloaded_only), finished_threshold, |row| {
            cards.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(cards)
    }
    fn get_recently_added_cards(&self, limit: i32, downloaded_only: bool) -> Result<Vec<BookCardRow>, DatabaseError> {
        let mut cards = Vec::new();
        self.connection.get_recently_added_cards(limit, i32::from(downloaded_only), |row| {
            cards.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(cards)
    }
    fn search_book_cards_scoped(&self, query: &str, downloaded_only: bool) -> Result<LibrarySearchResult, DatabaseError> {
        let normalized_query = catalog_model::normalize_search_text(query);
        if normalized_query.is_empty() {
            return Ok(LibrarySearchResult::default());
        }
        let mut title_matches = Vec::new();
        self.connection.search_book_cards_by_title(&normalized_query, i32::from(downloaded_only), |row| {
            title_matches.push(read_book_card_row(row)?);
            Ok(())
        })?;
        let mut author_matches = Vec::new();
        self.connection.search_book_cards_by_author(&normalized_query, i32::from(downloaded_only), |row| {
            author_matches.push(read_book_card_row(row)?);
            Ok(())
        })?;
        Ok(LibrarySearchResult { title_matches, author_matches })
    }
    fn get_authors_scoped(&self, downloaded_only: bool) -> Result<Vec<(AuthorId, String, Option<String>, usize)>, DatabaseError> {
        let mut authors = Vec::new();
        self.connection.get_authors(i32::from(downloaded_only), |row| {
            let raw_id: String = row.get(0)?;
            let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
            let name: String = row.get(1)?;
            let sort_name: Option<String> = row.get(2)?;
            let count: i64 = row.get(3)?;
            let count = usize::try_from(count).map_err(|_| invalid_integer_column(3, "author book count must not be negative"))?;
            authors.push((id, name, sort_name, count));
            Ok(())
        })?;
        Ok(authors)
    }
    /// The books each author's shelf draws, keyed by author, in shelf order.
    fn get_author_book_cards_scoped(&self, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<BookCardRow>>, DatabaseError> {
        let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
        self.connection.get_author_book_cards(i32::from(downloaded_only), per_author, |row| {
            let card = read_book_card_row(row)?;
            // The author is the last column, so the card reader keeps the column
            // order every other book-card query already uses.
            let raw_id: String = row.get(12)?;
            let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(12, error.to_string()))?;
            shelves.entry(id).or_default().push(card);
            Ok(())
        })?;
        Ok(shelves)
    }
    /// The subject chips each author's row carries, keyed by author, most-credited
    /// first.
    fn get_author_subjects_scoped(&self, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<library_model::AuthorSubject>>, DatabaseError> {
        let mut subjects: HashMap<AuthorId, Vec<library_model::AuthorSubject>> = HashMap::new();
        self.connection.get_author_subjects(i32::from(downloaded_only), per_author, |row| {
            let raw_id: String = row.get(0)?;
            let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
            subjects.entry(id).or_default().push(library_model::AuthorSubject::new(row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?));
            Ok(())
        })?;
        Ok(subjects)
    }
    /// Shared application projection for a library home screen. Platform hosts
    /// supply only the user's local-download preference.
    pub fn browse_home(&self, limit: i32, downloaded_only: bool) -> Result<LibraryHomeView, DatabaseError> {
        Ok(LibraryHomeView {
            most_progress: self.get_most_progress_cards( limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD))?,
            recently_read: self.get_recently_read_cards( limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD))?,
            recently_added: self.get_recently_added_cards( limit, downloaded_only)?,
        })
    }
    pub fn browse_library_folder(&self, directory_id: &DirId, downloaded_only: bool, file_type: LibraryFileTypeFilter) -> Result<LibraryFolderView, DatabaseError> {
        let scope = self.read_file_type_scope(file_type)?;
        let directory_id = directory_id.to_string();
        let directories = self.get_folder_list_rows_filtered_sorted( &directory_id, "", &[file_type], BrowseChipSort::Alphabetical, &[])?;
        let mut books = self.get_book_cards_in_dir_scoped( &directory_id, downloaded_only, "", LibraryFileTypeFilter::All)?;
        scope.retain_books(&mut books);
        let mut filtered = Vec::with_capacity(directories.len());
        for directory in directories {
            if !downloaded_only || directory.downloaded_book_count > 0 {
                filtered.push(LibraryFolderEntry { id: parse_uuid_column(directory.id, 0)?, name: directory.name, book_count: directory.book_count, downloaded_book_count: directory.downloaded_book_count });
            }
        }
        Ok(LibraryFolderView { children: filtered, books })
    }
    pub fn browse_authors(&self, sort: AuthorSort, downloaded_only: bool) -> Result<LibraryAuthorsView, DatabaseError> {
        let mut shelves = self.get_author_book_cards_scoped(downloaded_only, AUTHOR_SHELF_BOOK_COUNT)?;
        let mut subjects = self.get_author_subjects_scoped(downloaded_only, AUTHOR_SUBJECT_COUNT)?;
        let mut authors = self.get_authors_scoped(downloaded_only)?
            .into_iter()
            .map(|(id, name, sort_name, book_count)| {
                let books = shelves.remove(&id).unwrap_or_default();
                let subjects = subjects.remove(&id).unwrap_or_default();
                AuthorSummary::try_new(id, name, sort_name, book_count, books, subjects).map_err(|error| invalid_text_column(0, error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        sort.sort(&mut authors);
        Ok(LibraryAuthorsView { authors, books: None })
    }
    pub fn browse_library_search(&self, query: &str, downloaded_only: bool) -> Result<LibrarySearchResult, DatabaseError> {
        self.search_book_cards_scoped( query, downloaded_only)
    }
    pub fn browse_library_search_folder(&self, directory_id: &DirId, query: &str, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySearchResult, DatabaseError> {
        let mut result = self.search_book_cards_scoped( query, downloaded_only)?;
        let scope = self.read_file_type_scope(file_type)?;
        let allowed = self.get_descendant_content_hashes( &directory_id.to_string())?.into_iter().filter(|hash| scope.contains(hash)).collect::<HashSet<_>>();
        retain_search_results(&mut result, &allowed);
        Ok(result)
    }
    pub fn browse_library_search_subject(&self, subject_path: Option<&str>, query: &str, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySearchResult, DatabaseError> {
        let mut result = self.search_book_cards_scoped( query, downloaded_only)?;
        let scope = self.read_file_type_scope(file_type)?;
        let Some(subject_path) = subject_path else {
            result.title_matches.retain(|book| scope.contains(&book.content_hash));
            result.author_matches.retain(|book| scope.contains(&book.content_hash));
            return Ok(result);
        };
        let mut allowed: HashSet<ContentHash> = if subject_path == PLACES_SUBJECT_PATH {
            self.external_kind_hashes( "place")?.into_iter().collect()
        } else if let Some(wikidata_id) = external_path_id(subject_path, PLACES_SUBJECT_PATH) {
            self.get_external_entity_content_hashes( wikidata_id)?.into_iter().collect()
        } else {
            self.get_descendant_subject_content_hashes( subject_path)?.into_iter().collect()
        };
        allowed.retain(|hash| scope.contains(hash));
        retain_search_results(&mut result, &allowed);
        Ok(result)
    }
    pub fn browse_library_subjects(&self, subject_path: Option<&str>, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySubjectView, DatabaseError> {
        let scope = self.read_file_type_scope(file_type)?;
        if subject_path.is_some_and(|path| path == PLACES_SUBJECT_PATH || path.starts_with(&format!("{PLACES_SUBJECT_PATH}/"))) {
            return self.external_places( subject_path.expect("checked above"), downloaded_only, &scope);
        }
        let parent_path = subject_path.unwrap_or(ROOT_SUBJECT_PATH);
        let mut books = subject_path.map(|path| self.get_book_cards_in_subject_scoped( path, downloaded_only, "", LibraryFileTypeFilter::All)).transpose()?.unwrap_or_default();
        scope.retain_books(&mut books);
        let (mut subjects, hoisted_books) = self.compact_subject_children( parent_path, downloaded_only, &scope)?;
        let mut seen_books = books.iter().map(|book| book.content_hash.clone()).collect::<HashSet<_>>();
        books.extend(hoisted_books.into_iter().filter(|book| seen_books.insert(book.content_hash.clone())));
        if subject_path.is_none() {
            let (mut book_count, downloaded_book_count) = if scope.is_all() { self.external_kind_counts( "place")? } else { scope.counts(self.external_kind_hashes( "place")?) };
            if downloaded_only {
                book_count = downloaded_book_count;
            }
            if book_count > 0 {
                subjects.push(LibrarySubjectEntry { id: SubjectPath::new(PLACES_SUBJECT_PATH), name: "Places".to_owned(), book_count, downloaded_book_count });
            }
        }
        subjects.sort_by(|left, right| {
            subject_projection::unified_subject_sort_order(left.id.as_str())
                .cmp(&subject_projection::unified_subject_sort_order(right.id.as_str()))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(LibrarySubjectView { children: subjects, books })
    }
    /// Read the immediate children of all eligible folders in one snapshot/query.
    /// Eligibility ignores filters: hiding a direct book must not turn its folder into a heading.
    fn group_folder_children(&self, children: &mut Vec<BrowseRow>, file_types: &[LibraryFileTypeFilter], languages: &[&str], sort: BrowseChipSort, downloaded_only: bool) -> rusqlite::Result<Vec<library_model::BrowseChipGroup>> {
        let ids = serde_json::to_string(&children.iter().map(|row| &row.id).collect::<Vec<_>>()).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let types = file_types_query_value(file_types);
        let languages = languages_query_value(languages);
        let mut replacements = std::collections::HashMap::<String, Vec<BrowseRow>>::new();
        self.connection.group_folder_children_with(&ids, types, &languages, chip_sort_query_value(sort), downloaded_only, |row| {
            let total = row.get::<_, i64>(3)? as usize;
            let downloaded = row.get::<_, i64>(4)? as usize;
            if (downloaded_only && downloaded == 0) || ((types != 0 || !languages.is_empty()) && total == 0) {
                return Ok(());
            }
            replacements.entry(row.get(0)?).or_default().push(BrowseRow { id: row.get(1)?, name: row.get(2)?, path: None, book_count: if downloaded_only { downloaded } else { total }, downloaded_book_count: downloaded });
            Ok(())
        })?;
        Ok(expand_chip_groups(children, replacements))
    }

    pub fn browse_book_detail(&self, content_hash: ContentHash) -> Result<BookDetail, DatabaseError> {
    let mut book = None;
    self.connection.book_detail_summary_select(content_hash.as_str(), |row| {
        book = Some(read_book_card_row(row)?);
        Ok(())
    })?;
    let book = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    let mut source_directory_id: Option<String> = None;
    self.connection.book_detail_summary_select_2(content_hash.as_str(), |row| {
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

    let mut authors = Vec::new();
    self.connection.book_detail_summary_select_3(content_hash.as_str(), |row| {
        let id: String = row.get(0)?;
        authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
        Ok(())
    })?;

    // The same table the `author_credit` view reads, with the roles that view
    // filters out. Only narrator and translator: a reader picks an audiobook by
    // its narrator and an edition by its translator, and nothing else in the
    // contributor list answers a question a reader is asking.
    let mut credits = Vec::new();
    let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
    let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
    self.connection.book_detail_summary_select_4(content_hash.as_str(), narrator, translator, |row| {
        let id: String = row.get(0)?;
        let role: Vec<u8> = row.get(2)?;
        credits.push(library_model::BookDetailCredit {
            id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
            name: row.get(1)?,
            role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
        });
        Ok(())
    })?;

    let mut subject_paths = Vec::new();
    self.connection.book_detail_summary_select_5(content_hash.as_str(), |row| {
        subject_paths.push(row.get::<_, String>(0)?);
        Ok(())
    })?;

    let author_ids = authors.iter().map(|author| author.id.to_string()).collect::<Vec<_>>();
    let mut related = Vec::new();
    if !author_ids.is_empty() {
        let author_ids = serde_json::to_string(&author_ids).map_err(|error| invalid_text_column(0, error.to_string()))?;
        self.connection.related_books(&author_ids, content_hash.as_str(), |row| {
            related.push(read_book_card_row(row)?);
            Ok(())
        })?;
    }
    let toc = self.browse_book_navigation(&content_hash)?;
    let (current_entry, current_target) = self.browse_current_entry(&content_hash, book.format_category, &toc)?;
    Ok(BookDetail { book, source_directory_id, path, authors, subject_paths, credits, current_entry, current_target, toc, related })
}

/// The entry the reader is inside, for either medium: its wording and the entry
/// it is, which are wanted by different parts of a page.
///
/// An audiobook needs nothing stored: the position is a millisecond offset and
/// the chapters carry their bounds, so the answer is arithmetic. A document's
/// position is a CFI or a page, which only the renderer can resolve against a
/// navigation document — so the reader records the entry it resolved, and this
/// looks up the wording.
///
/// A document can be inside an entry whose wording is unknown — a stored target
/// that is no longer in the navigation, after a book was replaced — so the two
/// are returned independently rather than as a pair.
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

/// One shape for every medium, read from one place. An audiobook's chapters are
/// a flat ordered list, which is a tree one level deep, so nothing about a
/// format survives into how its navigation is stored or fetched.
    pub fn browse_directory_path(&self, directory_id: &DirId) -> Result<Vec<library_model::BrowsePathSegment>, DatabaseError> {
        Ok(self
            .dir_path_components(directory_id)?
            .into_iter()
            .map(|component| library_model::BrowsePathSegment { id: component.id.to_string(), name: component.name })
            .collect())
    }

    fn browse_book_navigation(&self, content_hash: &ContentHash) -> Result<Vec<library_model::BookTocEntry>, DatabaseError> {
        let mut stored: Option<String> = None;
        self.connection.book_toc_select(content_hash.as_str(), |row| {
            stored = row.get(0)?;
            Ok(())
        })?;
        Ok(stored
            .map(|json| {
                serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json)
                    .map_err(|error| DatabaseError::operation(format!("stored table of contents is not readable: {error}")))
            })
            .transpose()?
            .unwrap_or_default())
    }
}

/// Flatten one explicitly supplied level without inspecting promoted children.
/// Keep ungrouped chips together before headed groups, retaining input order
/// within both sections and within each group's children.
fn expand_chip_groups(children: &mut Vec<BrowseRow>, mut replacements: std::collections::HashMap<String, Vec<BrowseRow>>) -> Vec<library_model::BrowseChipGroup> {
    let mut flat = Vec::new();
    let mut expanded = Vec::new();
    for parent in std::mem::take(children) {
        match replacements.remove(&parent.id).filter(|rows| !rows.is_empty()) {
            Some(rows) => expanded.push((parent, rows)),
            None => flat.push(parent),
        }
    }
    let mut groups = Vec::with_capacity(expanded.len());
    for (parent, rows) in expanded {
        let start = flat.len();
        flat.extend(rows);
        groups.push(library_model::BrowseChipGroup { parent, start, end: flat.len() });
    }
    *children = flat;
    groups
}


include_sql!("src/browse/browse.sql");

#[cfg(test)]
mod browse_tests {
    use crate::browse::*;

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-browse-{}-{id}", std::process::id()));
        let database = Database::open(&path, |_| Ok(())).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    #[test]
    fn directory_hierarchy_browses() {
        let holder = open_test_database();
        let database = &holder.database;
        let parent = database
            .create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Parent".to_owned())
            .expect("seed parent");
        let child = database
            .create_directory_with_id(&uuid::Uuid::new_v4(), &parent.id, &"Child".to_owned())
            .expect("seed child");
        assert_eq!(database.browse_book_count().expect("book count"), 0);

        let contents = database
            .browse_folder_contents_filtered_sorted(
                &sync_common::ROOT_DIR_ID.to_string(),
                "",
                &[],
                library_model::BrowseChipSort::Alphabetical,
                library_model::BrowseBookSort::Alphabetical,
                &[],
                false,
            )
            .expect("folder contents");
        assert!(contents.books.is_empty());
        assert!(contents.children.iter().any(|row| row.name == "Parent" && row.book_count == 0));

        let path = database.browse_directory_path(&child.id).expect("directory path");
        assert_eq!(path.last().map(|segment| segment.name.as_str()), Some("Child"));

        let (languages, formats) = database
            .browse_folder_facet_counts(&sync_common::ROOT_DIR_ID.to_string(), "", &[], &[], false)
            .expect("facet counts");
        assert!(languages.is_empty() && formats.is_empty());

        let home = database.browse_home(10, false).expect("home view");
        assert!(home.most_progress.is_empty() && home.recently_read.is_empty() && home.recently_added.is_empty());
        assert!(database.browse_authors(library_model::AuthorSort::Name, false).expect("authors").authors.is_empty());
    }
}

// ---- toc ----
// Stored navigation document reads.
//
// Writes live with the import pipeline that produces them; what is read here
// is one book's navigation in the form inspection already produced it.

use library_model::BookTocEntry;

include_sql!("src/browse/toc.sql");

/// The title of one entry in a stored tree. The target is the identity; the
/// wording comes from whatever navigation document is stored now.
pub(crate) fn entry_title(entries: &[BookTocEntry], target: &str) -> Option<String> {
    for entry in entries {
        if entry.target == target {
            return Some(entry.title.clone());
        }
        if let Some(found) = entry_title(&entry.children, target) {
            return Some(found);
        }
    }
    None
}

// ---- audiobook ----
// Cached audiobook playback metadata: chapters, duration, and recording narrator.
//
// Reading is one snapshot call; committing freshly inspected metadata is one
// write call with its own transaction. Callers never hold a connection or
// transaction across the inspection work that produces the metadata.

use library_model::{AudiobookChapter, BookTocEntry};

/// Everything needed to open playback: whether the book is an audiobook, its
/// display title/author, and any already-cached chapter/duration/narrator data.
pub struct AudiobookPlaybackSnapshot {
    pub title: String,
    pub author: String,
    pub cached: Option<AudiobookPlaybackMetadata>,
}

pub struct AudiobookPlaybackMetadata {
    pub duration_ms: u64,
    pub chapters: Vec<AudiobookChapter>,
    pub narrator: Option<String>,
}

fn count(entries: &[BookTocEntry]) -> usize {
    entries.iter().map(|entry| 1 + count(&entry.children)).sum()
}

/// A chapter is an interval; a navigation entry is a point. The end is the next
/// chapter's start, and the last chapter runs to the end of the audio.
fn chapters_from_navigation(entries: &[BookTocEntry], duration_ms: u64) -> Vec<AudiobookChapter> {
    let starts = entries.iter().filter_map(|entry| Some((entry.title.as_str(), book_model::audiobook_toc_position(&entry.target)?))).collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .map(|(index, (title, start_ms))| AudiobookChapter { title: (*title).to_owned(), start_ms: *start_ms, end_ms: starts.get(index + 1).map_or(duration_ms, |(_, next)| *next) })
        .collect()
}

/// The file's chapters as the navigation every format shares: a title and the
/// offset it opens at, flat, because an M4B chapter list has no nesting.
pub(crate) fn navigation_from_chapters(chapters: &[book_metadata::AudiobookChapterMetadata]) -> Vec<BookTocEntry> {
    chapters.iter().map(|chapter| BookTocEntry { title: chapter.title.clone(), target: book_model::audiobook_toc_target(chapter.start_ms), children: Vec::new() }).collect()
}

impl Database {
    /// Everything a playback session needs to decide whether it must inspect
    /// the file: title/author for display, and cached chapters if present.
    pub fn audiobook_playback_snapshot(&self, hash: &sync_common::ContentHash) -> Result<AudiobookPlaybackSnapshot, DatabaseError> {
        let mut title_author = None;
        self.connection.audiobook_playback_title_author(hash.as_str(), |row| {
            title_author = Some((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let (title, author) = title_author.unwrap_or_default();
        let mut cached_row = None;
        self.connection.audiobook_cached_metadata(hash.as_str(), |row| {
            cached_row = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let cached = match cached_row {
            Some((duration_ms, toc_json)) => {
                let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(DatabaseError::operation)?;
                let chapters = chapters_from_navigation(&entries, duration_ms.max(0) as u64);
                let mut evidence = None;
                self.connection.audiobook_recording_evidence(hash.as_str(), |row| {
                    evidence = row.get::<_, Option<String>>(0)?;
                    Ok(())
                })?;
                let narrator = evidence.and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok()).and_then(|request| request.recording.narrators.into_iter().next());
                Some(AudiobookPlaybackMetadata { duration_ms: duration_ms as u64, chapters, narrator })
            }
            None => None,
        };
        Ok(AudiobookPlaybackSnapshot { title, author, cached })
    }

    /// Commits freshly inspected audiobook metadata and returns the resulting
    /// playback facts in one call. Opens and commits its own transaction; the
    /// inspection that produced `metadata` must already be finished.
    pub fn commit_audiobook_metadata(&self, hash: &sync_common::ContentHash, metadata: &book_metadata::AudiobookMetadata) -> Result<AudiobookPlaybackMetadata, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let duration_ms = i64::try_from(metadata.duration_ms).map_err(|_| DatabaseError::operation("audiobook duration exceeds SQLite integer range"))?;
        transaction.audiobook_upsert_metadata(hash.as_str(), duration_ms)?;
        // Preserve a database-only chapter plan while the imported evidence is unchanged.
        let mut applied_request = None;
        transaction.audiobook_applied_request(hash.as_str(), |row| {
            applied_request = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let request = metadata.audible_lookup_request();
        let preserve_chapters = applied_request.and_then(|json| serde_json::from_str::<metadata_contract::audible::LookupRequest>(&json).ok()).is_some_and(|applied| applied == request);
        let navigation = if preserve_chapters {
            None
        } else {
            let entries = navigation_from_chapters(&metadata.chapters);
            let entry_count = i64::try_from(count(&entries)).map_err(|_| DatabaseError::operation("table of contents entry count exceeds SQLite integer range"))?;
            let toc_json = serde_json::to_string(&entries).map_err(DatabaseError::operation)?;
            transaction.audiobook_upsert_toc(hash.as_str(), entry_count, &toc_json)?;
            Some(entries)
        };
        let request_json = serde_json::to_string(&request).map_err(DatabaseError::operation)?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(DatabaseError::operation)?.as_secs() as i64;
        transaction.audiobook_discover_job(hash.as_str(), metadata_contract::audible::POLICY_VERSION, &request_json, now)?;
        transaction.commit()?;
        let entries = match navigation {
            Some(entries) => entries,
            None => navigation_from_chapters(&metadata.chapters),
        };
        Ok(AudiobookPlaybackMetadata {
            duration_ms: metadata.duration_ms,
            chapters: chapters_from_navigation(&entries, metadata.duration_ms),
            narrator: metadata.recording.narrators.first().cloned(),
        })
    }

    /// An audiobook's chapters projected into the shared navigation form. A
    /// book with no stored navigation comes back empty rather than wrong.
    pub fn audiobook_chapters(&self, hash: &sync_common::ContentHash) -> Result<Vec<AudiobookChapter>, DatabaseError> {
        let mut stored: Option<(String, i64)> = None;
        self.connection.audiobook_chapters_select(hash.as_str(), |row| {
            stored = Some((row.get(0)?, row.get(1)?));
            Ok(())
        })?;
        let Some((toc_json, duration_ms)) = stored else { return Ok(Vec::new()) };
        let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json)
            .map_err(|error| DatabaseError::operation(format!("stored table of contents is not readable: {error}")))?;
        Ok(chapters_from_navigation(&entries, duration_ms.max(0) as u64))
    }
}

include_sql!("src/browse/audiobook.sql");

// ---- reading ----
// The one synchronized reading-position register, shared by every format.

use book_model::{EpubCfi, ReadProgress, ReadingPosition};

impl Database {
    fn stored_reading_position(&self, hash: &ContentHash) -> Result<Option<ReadingPosition>, DatabaseError> {
        let mut raw = None;
        self.connection.reading_get_position(hash.as_str(), |row| {
            raw = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        let Some(raw) = raw.filter(|value| value != "0" && !value.is_empty()) else {
            return Ok(None);
        };
        ReadingPosition::parse(&raw).map(Some).map_err(DatabaseError::operation)
    }

    pub fn reading_position(&self, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.map(|position| position.as_str()))
    }

    pub fn epub_reading_position(&self, hash: &ContentHash) -> Result<Option<EpubCfi>, DatabaseError> {
        self.stored_reading_position(hash)?.and_then(|position| position.epub_cfi_value().map(EpubCfi::parse)).transpose().map_err(DatabaseError::operation)
    }

    pub fn pdf_reading_page(&self, hash: &ContentHash) -> Result<Option<u32>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.pdf_page_index()))
    }

    pub fn pdf_reading_position(&self, hash: &ContentHash) -> Result<Option<(u32, f32)>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.pdf_page_index().map(|page_index| (page_index, position.pdf_page_position().unwrap_or(0.0)))))
    }

    pub fn audiobook_reading_position(&self, hash: &ContentHash) -> Result<Option<u64>, DatabaseError> {
        Ok(self.stored_reading_position(hash)?.and_then(|position| position.audiobook_position_millis()))
    }

    /// Stores the canonical reading position and, when given, the navigation
    /// entry the reader resolved it to. Both writes happen inline in one
    /// transaction; `entry` travels with the position because that is when it
    /// changes, and is stored locally rather than synchronized.
    pub fn update_reading_position(&self, hash: &ContentHash, position: &str, progress: Option<f32>, entry: Option<&str>) -> Result<(), DatabaseError> {
        let position = ReadingPosition::parse(position).map_err(DatabaseError::operation)?.as_str();
        let progress = progress.map(ReadProgress::new).transpose().map_err(DatabaseError::operation)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.reading_set_position(hash.as_str(), &position, progress.map(ReadProgress::value))?;
        if let Some(entry) = entry.map(str::trim).filter(|entry| !entry.is_empty()) {
            transaction.reading_upsert_toc_entry(hash.as_str(), entry)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

include_sql!("src/browse/reading.sql");

// ---- annotations ----
use book_model::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation};
use rusqlite::{Row, types::Type};

impl Database {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, DatabaseError> {
        let mut values = Vec::new();
        self.connection.get_annotations(content_hash.as_str(), |row| { values.push(read_annotation(row)?); Ok(()) })?;
        Ok(values)
    }
    pub fn upsert_annotation(&self, value: &ReaderAnnotation) -> Result<(), DatabaseError> {
        value.validate(false).map_err(DatabaseError::operation)?;
        let anchor = stored_anchor(&value.anchor)?;
        self.connection.upsert_annotation(&value.id, value.content_hash.as_str(), anchor.kind, anchor.cfi.as_deref(), anchor.page, anchor.rects.as_deref(), anchor.fallback_cfi.as_deref(), &value.exact_text, value.style.storage(), &value.color, &value.note, value.created_at, value.modified_at)?;
        Ok(())
    }
    pub fn delete_annotation(&self, id: &str, modified_at: i64) -> Result<(), DatabaseError> {
        let mut existing = None; self.connection.get_annotation_by_id(id, |row| { existing = Some(read_annotation(row)?); Ok(()) })?;
        let mut annotation = existing.ok_or_else(|| DatabaseError::operation("annotation not found"))?;
        annotation.modified_at = modified_at; annotation.validate(true).map_err(DatabaseError::operation)?;
        self.connection.delete_annotation(id, modified_at)?;
        Ok(())
    }
}

fn read_annotation(row: &Row<'_>) -> rusqlite::Result<ReaderAnnotation> {
    Ok(ReaderAnnotation { id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), anchor: read_anchor(row, 2)?, exact_text: row.get(7)?, style: AnnotationStyle::parse_storage(&row.get::<_, String>(8)?).ok_or(rusqlite::Error::InvalidQuery)?, color: row.get(9)?, note: row.get(10)?, created_at: row.get(11)?, modified_at: row.get(12)? })
}
fn read_anchor(row: &Row<'_>, index: usize) -> rusqlite::Result<AnnotationAnchor> {
    match row.get::<_, String>(index)?.as_str() {
        "epub_cfi" => Ok(AnnotationAnchor::epub_cfi(row.get::<_, String>(index + 1)?)),
        "pdf" => { let page = u32::try_from(row.get::<_, i64>(index + 2)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(index + 2, Type::Integer, Box::new(error)))?; let rects: Vec<PdfAnnotationRect> = sync_common::wire::decode(&row.get::<_, Vec<u8>>(index + 3)?, 1024 * 1024).map_err(|error| rusqlite::Error::FromSqlConversionFailure(index + 3, Type::Blob, Box::new(error)))?; Ok(AnnotationAnchor::pdf(PdfAnnotationAnchor::new(page, rects, row.get(index + 4)?))) }
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
pub(super) struct StoredAnchor { pub(super) kind: &'static str, pub(super) cfi: Option<String>, pub(super) page: Option<i64>, pub(super) rects: Option<Vec<u8>>, pub(super) fallback_cfi: Option<String> }
pub(super) fn stored_anchor(anchor: &AnnotationAnchor) -> Result<StoredAnchor, DatabaseError> { match anchor { AnnotationAnchor::EpubCfi { cfi } => Ok(StoredAnchor { kind: "epub_cfi", cfi: Some(cfi.clone()), page: None, rects: None, fallback_cfi: None }), AnnotationAnchor::Pdf { page_index, rects, fallback_cfi } => Ok(StoredAnchor { kind: "pdf", cfi: None, page: Some(i64::from(*page_index)), rects: Some(sync_common::wire::encode(rects).map_err(DatabaseError::operation)?), fallback_cfi: fallback_cfi.clone() }) } }

include_sql!("src/browse/annotations.sql");

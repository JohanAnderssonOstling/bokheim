mod folders;
#[cfg(feature = "scanner")]
use library_replica::RelativeDirPath;
#[cfg(feature = "scanner")]
use crate::scanner::ScannerSql;
pub use folders::{folder_contents_filtered_sorted, grouped_folder_contents};
use include_sqlite_sql::include_sql;
#[cfg(feature = "scanner")]
use sync_common::FileName;
mod subject_incremental;
use super::books::{get_book_cards_by_author_scoped, get_book_cards_in_dir_scoped};
use crate::books::read_book_card_row;
use crate::storage_values::invalid_integer_column;
use crate::storage_values::invalid_text_column;
use crate::storage_values::parse_uuid_column;
use crate::LibraryAuthorsView;
use crate::LibraryFolderEntry;
use crate::LibraryFolderView;
use crate::LibrarySubjectEntry;
use crate::LibrarySubjectView;
use book_model::AgentId;
use book_model::AuthorId;
use library_model::BookCardRow;
use library_model::Dir;
use subject_projection::ROOT_SUBJECT_PATH;
use library_model::{AuthorSort, AuthorSummary, BookDetail, BookDetailAuthor, BrowseBookSort, BrowseChipSort, BrowseRow, LibraryFileTypeFilter, LibraryFormatCount, LibraryHomeView, LibraryLanguageCount, LibrarySearchResult, SubjectPath};
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::collections::HashSet;
use sync_common::ContentHash;
use sync_common::DirId;

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

#[path = "subject_compaction.rs"]
mod subject_compaction;
pub use subject_compaction::{compacted_subject_contents, grouped_subject_contents, subject_contents};

// Virtual roots are not prefixes of curated paths. Always resolve their
// children first, even when a top-level label is shorter than "Subject".
fn subject_path_depth(path: &str) -> usize {
    if path.is_empty() {
        0
    } else if path == ROOT_SUBJECT_PATH {
        1
    } else {
        path.split(" / ").count() + 1
    }
}

pub const PLACES_SUBJECT_PATH: &str = "__places__";
/// How far an author's shelf carousel can be paged. The shelf is one row of
/// book cards, so a wide window draws a dozen or so at once and this is a few
/// pages past that; the cap exists because the index loads every author's shelf
/// in one query, not because the row runs out of space.
const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;

/// How many subject chips an author's row states. The chips share the heading
/// line with the name, so this is what fits beside it rather than everything an
/// author has been filed under.
const AUTHOR_SUBJECT_COUNT: i32 = 3;

/// Number of visible books with at least one live library placement.
///
/// Retained metadata rows and books that only remain in Trash do not make a
/// library non-empty.
pub fn library_book_count(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let mut count = 0_i64;
    conn.count_all_books(|row| {
        count = row.get(0)?;
        Ok(())
    })?;
    usize::try_from(count).map_err(|_| invalid_integer_column(0, "library book count must not be negative"))
}

/// Counts visible books by normalized base language across the whole library.
/// A book with several regional variants of the same language is counted once.
pub fn library_language_counts(conn: &Connection) -> Result<Vec<LibraryLanguageCount>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/browse/library_language_counts_with.sql"))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    rows.map(|row| {
        let (language, count) = row?;
        let book_count = u32::try_from(count).map_err(|_| invalid_integer_column(1, "language book count must fit in u32"))?;
        Ok(LibraryLanguageCount { language, book_count })
    })
    .collect()
}

/// Counts visible books using the same broad format categories as browsing.
pub fn library_format_counts(conn: &Connection) -> Result<Vec<LibraryFormatCount>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/browse/library_format_counts_select.sql"))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    rows.map(|row| {
        let (format_category, count) = row?;
        let format = match format_category {
            1 => LibraryFileTypeFilter::Book,
            2 => LibraryFileTypeFilter::Pdf,
            3 => LibraryFileTypeFilter::Audiobook,
            _ => return Err(invalid_integer_column(0, "unknown library format category")),
        };
        let book_count = u32::try_from(count).map_err(|_| invalid_integer_column(1, "format book count must fit in u32"))?;
        Ok(LibraryFormatCount { format, book_count })
    })
    .collect()
}

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

pub fn folder_facet_counts(conn: &Connection, folder_id: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str]) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), rusqlite::Error> {
    folder_facet_counts_scoped(conn, folder_id, normalized_query, file_types, languages, false)
}

pub fn folder_facet_counts_scoped(
    conn: &Connection, folder_id: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str], downloaded_only: bool,
) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), rusqlite::Error> {
    let file_types = file_types_query_value(file_types);
    let languages = languages_query_value(languages);
    let mut language_counts = read_language_facet_rows(|rows| conn.get_folder_language_facets(folder_id, normalized_query, file_types, i32::from(downloaded_only), rows))?;
    let mut format_counts = read_format_facet_rows(|rows| conn.get_folder_format_facets(folder_id, normalized_query, &languages, i32::from(downloaded_only), rows))?;
    retain_selected_facet_choices(&mut language_counts, &mut format_counts, &languages, file_types);
    Ok((language_counts, format_counts))
}

pub fn subject_facet_counts(conn: &Connection, subject_path: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str]) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), rusqlite::Error> {
    subject_facet_counts_scoped(conn, subject_path, normalized_query, file_types, languages, false)
}

pub fn subject_facet_counts_scoped(
    conn: &Connection, subject_path: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], languages: &[&str], downloaded_only: bool,
) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), rusqlite::Error> {
    subject_compaction::ensure_projection(conn)?;
    let dirty = conn.query_row(include_str!("sql/browse/subject_facet_counts_scoped_select.sql"), [], |row| row.get::<_, bool>(0))?;
    if dirty {
        // Keep the derived counts and their dirty flag in one main-database
        // write transaction so an import cannot be lost between those writes.
        let transaction = conn.unchecked_transaction()?;
        transaction.execute(include_str!("sql/browse/subject_facet_counts_scoped_update_2.sql"), [])?;
        transaction.rebuild_subject_book_ownership()?;
        transaction.execute(include_str!("sql/subject_compaction/ensure_projection_update_2.sql"), [])?;
        transaction.commit()?;
    }
    let file_types = file_types_query_value(file_types);
    let languages = languages_query_value(languages);
    let mut language_counts = read_language_facet_rows(|rows| conn.get_subject_language_facets(subject_path, normalized_query, file_types, i32::from(downloaded_only), rows))?;
    let mut format_counts = read_format_facet_rows(|rows| conn.get_subject_format_facets(subject_path, normalized_query, &languages, i32::from(downloaded_only), rows))?;
    retain_selected_facet_choices(&mut language_counts, &mut format_counts, &languages, file_types);
    Ok((language_counts, format_counts))
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

pub fn file_type_query_value(filter: LibraryFileTypeFilter) -> i32 {
    match filter {
        LibraryFileTypeFilter::All => 0,
        LibraryFileTypeFilter::Book => 1,
        LibraryFileTypeFilter::Pdf => 2,
        LibraryFileTypeFilter::Audiobook => 4,
    }
}

pub fn file_types_query_value(filters: &[LibraryFileTypeFilter]) -> i32 {
    if filters.is_empty() || filters.contains(&LibraryFileTypeFilter::All) {
        return 0;
    }
    filters.iter().fold(0, |mask, filter| mask | file_type_query_value(*filter))
}

pub fn languages_query_value(languages: &[&str]) -> String {
    if languages.is_empty() {
        return String::new();
    }
    let mut languages = languages.iter().map(|language| language.trim().to_lowercase()).filter(|language| !language.is_empty()).collect::<Vec<_>>();
    languages.sort();
    languages.dedup();
    format!(",{},", languages.join(","))
}

pub const fn chip_sort_query_value(sort: BrowseChipSort) -> i32 {
    match sort {
        BrowseChipSort::Alphabetical => 0,
        BrowseChipSort::BookCount => 1,
    }
}

pub const fn book_sort_query_value(sort: BrowseBookSort) -> i32 {
    match sort {
        BrowseBookSort::Alphabetical => 0,
        BrowseBookSort::RecentlyRead => 1,
    }
}

pub struct FileTypeScope {
    filter: LibraryFileTypeFilter,
    matching: HashSet<ContentHash>,
    downloaded: HashSet<ContentHash>,
}

impl FileTypeScope {
    pub fn load(conn: &Connection, filter: LibraryFileTypeFilter) -> Result<Self, rusqlite::Error> {
        let mut matching = HashSet::new();
        let mut downloaded = HashSet::new();
        let mut statement = conn.prepare(include_str!("sql/browse/load_select.sql"))?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?;
        for row in rows {
            let (hash, format, is_downloaded) = row?;
            if filter.matches_extension(&format) {
                let hash = ContentHash::new(&hash);
                if is_downloaded {
                    downloaded.insert(hash.clone());
                }
                matching.insert(hash);
            }
        }
        Ok(Self { filter, matching, downloaded })
    }

    pub fn is_all(&self) -> bool {
        self.filter == LibraryFileTypeFilter::All
    }

    pub fn contains(&self, content_hash: &ContentHash) -> bool {
        self.matching.contains(content_hash)
    }

    pub fn retain_books(&self, books: &mut Vec<BookCardRow>) {
        if !self.is_all() {
            books.retain(|book| self.matching.contains(&book.content_hash));
        }
    }

    pub fn counts(&self, hashes: impl IntoIterator<Item = ContentHash>) -> (usize, usize) {
        let hashes = hashes.into_iter().collect::<HashSet<_>>();
        (hashes.intersection(&self.matching).count(), hashes.intersection(&self.downloaded).count())
    }
}

/// Legacy entry point for the unsearched view; returns actual children and
/// directly assigned books, with no compaction or promotion.
pub fn compact_subject_children(conn: &Connection, parent_path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<(Vec<LibrarySubjectEntry>, Vec<BookCardRow>), rusqlite::Error> {
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
    let contents = compacted_subject_contents(conn, &query, downloaded_only)?;
    let children = contents.children.into_iter().map(|child| LibrarySubjectEntry { id: SubjectPath::new(child.id), name: child.name, book_count: child.book_count, downloaded_book_count: child.downloaded_book_count }).collect();
    Ok((children, contents.books))
}

pub fn populated_subject_children(conn: &Connection, parent_path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<Vec<LibrarySubjectEntry>, rusqlite::Error> {
    let mut populated = Vec::new();
    for mut subject in get_subject_children(conn, parent_path)? {
        if !scope.is_all() {
            (subject.book_count, subject.downloaded_book_count) = scope.counts(get_descendant_subject_content_hashes(conn, subject.id.as_str())?);
        }
        if downloaded_only {
            subject.book_count = subject.downloaded_book_count;
        }
        if subject.book_count > 0 {
            populated.push(subject);
        }
    }
    Ok(populated)
}

#[cfg(feature = "scanner")]
#[derive(Clone)]
pub enum KnownDirectoryState {
    /// `None` means the library root, which has no relative directory path of its own.
    Live(Option<RelativeDirPath>),
    Deleted,
}
#[cfg(feature = "scanner")]
pub struct KnownDirectory {
    pub parent_id: DirId,
    pub name: FileName,
    pub state: KnownDirectoryState,
}
#[cfg(feature = "scanner")]
pub fn read_known_directory_state(row: &rusqlite::Row<'_>, live_column: usize, path_column: usize) -> Result<KnownDirectoryState, rusqlite::Error> {
    let live: bool = row.get(live_column)?;
    if !live {
        return Ok(KnownDirectoryState::Deleted);
    }
    let raw_path: Option<String> = row.get(path_column)?;
    let path = match raw_path.as_deref() {
        None | Some("") => None,
        Some(raw_path) => Some(RelativeDirPath::parse(raw_path).map_err(|error| invalid_text_column(path_column, error.to_string()))?),
    };
    Ok(KnownDirectoryState::Live(path))
}
#[cfg(feature = "scanner")]
pub fn get_known_directory_scan_state(conn: &Connection, dir_id: &DirId) -> Result<Option<KnownDirectoryState>, rusqlite::Error> {
    let mut state = None;
    conn.get_known_directory_scan_state(&dir_id.to_string(), |row| {
        state = Some(read_known_directory_state(row, 0, 1)?);
        Ok(())
    })?;
    Ok(state)
}
#[cfg(feature = "scanner")]
pub fn get_known_directories_for_scan(conn: &Connection) -> Result<HashMap<DirId, KnownDirectory>, rusqlite::Error> {
    let mut directories = HashMap::new();
    conn.get_known_directories_for_scan(|row| {
        let id = parse_uuid_column(row.get::<_, String>(0)?, 0)?;
        let parent_id = parse_uuid_column(row.get::<_, String>(1)?, 1)?;
        let name = row.get(2)?;
        let state = read_known_directory_state(row, 3, 4)?;
        directories.insert(id, KnownDirectory { parent_id, name, state });
        Ok(())
    })?;
    Ok(directories)
}
#[cfg(feature = "scanner")]
pub fn is_book_file_tombstoned(conn: &Connection, dir_id: &DirId, file_name: &FileName) -> Result<bool, rusqlite::Error> {
    let mut tombstoned = false;
    conn.is_book_file_tombstoned(&dir_id.to_string(), file_name.as_str(), |row| {
        tombstoned = row.get(0)?;
        Ok(())
    })?;
    Ok(tombstoned)
}
pub fn get_child_directories(conn: &Connection, dir_path: &str) -> Result<Vec<Dir>, rusqlite::Error> {
    let mut sub_dirs: Vec<Dir> = Vec::new();
    conn.get_immediate_sub_dirs(dir_path, chip_sort_query_value(BrowseChipSort::Alphabetical), |row| {
        let raw_id: String = row.get(0)?;
        let raw_name: String = row.get(1)?;
        let raw_book_count: i64 = row.get(2)?;
        let raw_downloaded_book_count: i64 = row.get(3)?;
        let id = parse_uuid_column(raw_id, 0)?;
        let name = raw_name;
        let book_count = usize::try_from(raw_book_count).map_err(|_| invalid_integer_column(2, "folder book count must not be negative"))?;
        let downloaded_book_count = usize::try_from(raw_downloaded_book_count).map_err(|_| invalid_integer_column(3, "downloaded folder book count must not be negative"))?;
        sub_dirs.push(Dir::new(id, name, book_count, downloaded_book_count));
        Ok(())
    })?;
    Ok(sub_dirs)
}

#[cfg(test)]
pub fn get_folder_list_rows(conn: &Connection, parent_id: &str, normalized_query: &str, file_type: LibraryFileTypeFilter) -> Result<Vec<BrowseRow>, rusqlite::Error> {
    get_folder_list_rows_sorted(conn, parent_id, normalized_query, file_type, BrowseChipSort::Alphabetical)
}

#[cfg(test)]
pub fn get_folder_list_rows_sorted(conn: &Connection, parent_id: &str, normalized_query: &str, file_type: LibraryFileTypeFilter, chip_sort: BrowseChipSort) -> Result<Vec<BrowseRow>, rusqlite::Error> {
    get_folder_list_rows_filtered_sorted(conn, parent_id, normalized_query, &[file_type], chip_sort, &[])
}

pub fn get_folder_list_rows_filtered_sorted(conn: &Connection, parent_id: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, languages: &[&str]) -> Result<Vec<BrowseRow>, rusqlite::Error> {
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
        conn.get_immediate_sub_dirs(parent_id, chip_sort_query_value(chip_sort), &mut append_row)?;
    } else {
        conn.get_sub_dirs(parent_id, normalized_query, file_types, chip_sort_query_value(chip_sort), &languages, &mut append_row)?;
    }
    Ok(folders)
}

#[cfg(test)]
pub fn get_subject_browse_rows(conn: &Connection, parent_path: &str, normalized_query: &str, file_type: LibraryFileTypeFilter) -> Result<Vec<BrowseRow>, rusqlite::Error> {
    get_subject_browse_rows_sorted(conn, parent_path, normalized_query, file_type, BrowseChipSort::Alphabetical)
}

#[cfg(test)]
pub fn get_subject_browse_rows_sorted(conn: &Connection, parent_path: &str, normalized_query: &str, file_type: LibraryFileTypeFilter, chip_sort: BrowseChipSort) -> Result<Vec<BrowseRow>, rusqlite::Error> {
    get_subject_browse_rows_filtered_sorted(conn, parent_path, normalized_query, &[file_type], chip_sort, &[])
}

pub fn get_subject_browse_rows_filtered_sorted(conn: &Connection, parent_path: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, languages: &[&str]) -> Result<Vec<BrowseRow>, rusqlite::Error> {
    let mut subjects = Vec::new();
    let languages = languages_query_value(languages);
    conn.get_subject_children(parent_path, normalized_query, file_types_query_value(file_types), chip_sort_query_value(chip_sort), &languages, |row| {
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

pub fn get_subject_children(conn: &Connection, parent_path: &str) -> Result<Vec<LibrarySubjectEntry>, rusqlite::Error> {
    let mut subjects = Vec::new();
    conn.get_subject_children(parent_path, "", file_type_query_value(LibraryFileTypeFilter::All), chip_sort_query_value(BrowseChipSort::Alphabetical), "", |row| {
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

pub fn get_book_cards_in_subject_scoped(conn: &Connection, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_type: LibraryFileTypeFilter) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    get_book_cards_in_subject_sorted_scoped(conn, subject_path, downloaded_only, normalized_query, file_type, BrowseBookSort::Alphabetical)
}

pub fn get_book_cards_in_subject_sorted_scoped(conn: &Connection, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_type: LibraryFileTypeFilter, book_sort: BrowseBookSort) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    get_book_cards_in_subject_filtered_sorted_scoped(conn, subject_path, downloaded_only, normalized_query, &[file_type], book_sort, &[])
}

pub fn get_book_cards_in_subject_filtered_sorted_scoped(
    conn: &Connection, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_types: &[LibraryFileTypeFilter], book_sort: BrowseBookSort, languages: &[&str],
) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    subject_book_cards(conn, subject_path, downloaded_only, normalized_query, file_types, book_sort, languages, false)
}

fn subject_book_cards(
    conn: &Connection, subject_path: &str, downloaded_only: bool, normalized_query: &str, file_types: &[LibraryFileTypeFilter], book_sort: BrowseBookSort, languages: &[&str], include_descendants: bool,
) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    subject_compaction::ensure_projection(conn)?;
    let mut cards = Vec::new();
    let languages = languages_query_value(languages);
    conn.get_book_cards_in_subject(subject_path, i32::from(downloaded_only), normalized_query, file_types_query_value(file_types), book_sort_query_value(book_sort), &languages, i32::from(include_descendants), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

pub fn get_descendant_content_hashes(conn: &Connection, dir_id: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut content_hashes = Vec::new();
    conn.get_descendant_content_hashes(dir_id, |row| {
        let raw: String = row.get(0)?;
        content_hashes.push(ContentHash::new(&raw));
        Ok(())
    })?;
    Ok(content_hashes)
}
pub fn get_descendant_subject_content_hashes(conn: &Connection, subject_path: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut content_hashes = Vec::new();
    conn.get_descendant_subject_content_hashes(subject_path, |row| {
        let raw: String = row.get(0)?;
        content_hashes.push(ContentHash::new(&raw));
        Ok(())
    })?;
    Ok(content_hashes)
}

pub fn external_path_id<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    path.strip_prefix(root)?.strip_prefix('/')?.rsplit('/').next().filter(|id| {
        (id.starts_with('Q') && id[1..].bytes().all(|byte| byte.is_ascii_digit())) || (["OLP-", "OLR-"].iter().any(|prefix| id.starts_with(prefix)) && id[4..].len() % 2 == 0 && id[4..].bytes().all(|byte| byte.is_ascii_hexdigit()))
    })
}

pub fn external_kind_counts(conn: &Connection, kind: &str) -> Result<(usize, usize), rusqlite::Error> {
    conn.query_row(include_str!("sql/browse/external_kind_counts_select.sql"), [kind], |row| {
        let total = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "external entity book count must not be negative"))?;
        let downloaded = usize::try_from(row.get::<_, i64>(1)?).map_err(|_| invalid_integer_column(1, "external entity downloaded count must not be negative"))?;
        Ok((total, downloaded))
    })
}

pub fn external_entity_counts(conn: &Connection, wikidata_id: &str, recursive_places: bool) -> Result<(usize, usize), rusqlite::Error> {
    let mut counts = (0, 0);
    conn.get_external_entity_book_counts(wikidata_id, i32::from(recursive_places), |row| {
        counts = (
            usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "external entity book count must not be negative"))?,
            usize::try_from(row.get::<_, i64>(1)?).map_err(|_| invalid_integer_column(1, "external entity downloaded count must not be negative"))?,
        );
        Ok(())
    })?;
    Ok(counts)
}

pub fn external_entity_books(conn: &Connection, wikidata_id: &str, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut books = Vec::new();
    conn.get_book_cards_for_external_entity(wikidata_id, i32::from(downloaded_only), |row| {
        books.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(books)
}

pub fn external_places(conn: &Connection, path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<LibrarySubjectView, rusqlite::Error> {
    if let Some(wikidata_id) = external_path_id(path, PLACES_SUBJECT_PATH) {
        let mut books = external_entity_books(conn, wikidata_id, downloaded_only)?;
        scope.retain_books(&mut books);
        return Ok(LibrarySubjectView { children: Vec::new(), books });
    }
    let mut raw_places = Vec::<(String, String)>::new();
    conn.get_external_places(|row| {
        raw_places.push((row.get(0)?, row.get(1)?));
        Ok(())
    })?;
    let mut children = Vec::new();
    for (wikidata_id, name) in raw_places {
        let (mut book_count, downloaded_book_count) = if scope.is_all() { external_entity_counts(conn, &wikidata_id, false)? } else { scope.counts(get_external_entity_content_hashes(conn, &wikidata_id)?) };
        if downloaded_only {
            book_count = downloaded_book_count;
        }
        if book_count > 0 {
            children.push(LibrarySubjectEntry { id: SubjectPath::new(format!("{PLACES_SUBJECT_PATH}/{wikidata_id}")), name, book_count, downloaded_book_count });
        }
    }
    Ok(LibrarySubjectView { children, books: Vec::new() })
}

pub fn get_external_entity_content_hashes(conn: &Connection, wikidata_id: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/browse/get_external_entity_content_hashes_select.sql"))?;
    let hashes = statement.query_map([wikidata_id], |row| row.get::<_, String>(0))?.map(|value| value.map(|value| ContentHash::new(&value))).collect();
    hashes
}

pub fn external_kind_hashes(conn: &Connection, kind: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/browse/external_kind_hashes_select.sql"))?;
    let hashes = statement.query_map([kind], |row| row.get::<_, String>(0))?.map(|value| value.map(|value| ContentHash::new(&value))).collect();
    hashes
}
pub fn get_folder_download_counts(conn: &Connection, dir_id: &str) -> Result<(usize, usize), rusqlite::Error> {
    let mut counts = (0, 0);
    conn.get_folder_download_counts(dir_id, |row| {
        let total = row.get::<_, i64>(0)?;
        let downloaded = row.get::<_, i64>(1)?;
        counts = (usize::try_from(downloaded).map_err(|_| invalid_integer_column(1, "downloaded book count must not be negative"))?, usize::try_from(total).map_err(|_| invalid_integer_column(0, "folder book count must not be negative"))?);
        Ok(())
    })?;
    Ok(counts)
}
pub fn get_book_ancestor_dir_ids(conn: &Connection, content_hash: ContentHash) -> Result<Vec<DirId>, rusqlite::Error> {
    let mut dir_ids = Vec::new();
    conn.get_book_ancestor_dir_ids(content_hash.as_str(), |row| {
        let raw: String = row.get(0)?;
        dir_ids.push(parse_uuid_column(raw, 0)?);
        Ok(())
    })?;
    Ok(dir_ids)
}
/// The most recently read books: up to `limit` still in progress, then up to
/// `limit` finished. The two groups arrive in one statement and stay in
/// reading order within each, so a caller splits them by the same threshold
/// the query ranked them with.
pub fn get_recently_read_cards(conn: &Connection, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_recently_read_cards(limit, i32::from(downloaded_only), finished_threshold, |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}
pub fn get_most_progress_cards(conn: &Connection, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_most_progress_cards(limit, i32::from(downloaded_only), finished_threshold, |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}
pub fn get_recently_added_cards(conn: &Connection, limit: i32, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_recently_added_cards(limit, i32::from(downloaded_only), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}
pub fn search_book_cards_scoped(conn: &Connection, query: &str, downloaded_only: bool) -> Result<LibrarySearchResult, rusqlite::Error> {
    let normalized_query = catalog_model::normalize_search_text(query);
    if normalized_query.is_empty() {
        return Ok(LibrarySearchResult::default());
    }
    let mut title_matches = Vec::new();
    conn.search_book_cards_by_title(&normalized_query, i32::from(downloaded_only), |row| {
        title_matches.push(read_book_card_row(row)?);
        Ok(())
    })?;
    let mut author_matches = Vec::new();
    conn.search_book_cards_by_author(&normalized_query, i32::from(downloaded_only), |row| {
        author_matches.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(LibrarySearchResult { title_matches, author_matches })
}

pub fn retain_search_results(result: &mut LibrarySearchResult, allowed: &HashSet<ContentHash>) {
    result.title_matches.retain(|book| allowed.contains(&book.content_hash));
    result.author_matches.retain(|book| allowed.contains(&book.content_hash));
}

pub fn get_authors_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<(AuthorId, String, Option<String>, usize)>, rusqlite::Error> {
    let mut authors = Vec::new();
    conn.get_authors(i32::from(downloaded_only), |row| {
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
pub fn get_author_book_cards_scoped(conn: &Connection, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<BookCardRow>>, rusqlite::Error> {
    let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
    conn.get_author_book_cards(i32::from(downloaded_only), per_author, |row| {
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
pub fn get_author_subjects_scoped(conn: &Connection, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<library_model::AuthorSubject>>, rusqlite::Error> {
    let mut subjects: HashMap<AuthorId, Vec<library_model::AuthorSubject>> = HashMap::new();
    conn.get_author_subjects(i32::from(downloaded_only), per_author, |row| {
        let raw_id: String = row.get(0)?;
        let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        subjects.entry(id).or_default().push(library_model::AuthorSubject::new(row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?));
        Ok(())
    })?;
    Ok(subjects)
}

/// Shared application projection for a library home screen. Platform hosts
/// supply only the user's local-download preference.
pub fn library_home(conn: &Connection, limit: i32, downloaded_only: bool) -> Result<LibraryHomeView, rusqlite::Error> {
    Ok(LibraryHomeView {
        most_progress: get_most_progress_cards(conn, limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD))?,
        recently_read: get_recently_read_cards(conn, limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD))?,
        recently_added: get_recently_added_cards(conn, limit, downloaded_only)?,
    })
}

pub fn library_folder(conn: &Connection, directory_id: &DirId, downloaded_only: bool, file_type: LibraryFileTypeFilter) -> Result<LibraryFolderView, rusqlite::Error> {
    let scope = FileTypeScope::load(conn, file_type)?;
    let directory_id = directory_id.to_string();
    let directories = get_folder_list_rows_filtered_sorted(conn, &directory_id, "", &[file_type], BrowseChipSort::Alphabetical, &[])?;
    let mut books = get_book_cards_in_dir_scoped(conn, &directory_id, downloaded_only, "", LibraryFileTypeFilter::All)?;
    scope.retain_books(&mut books);
    let mut filtered = Vec::with_capacity(directories.len());
    for directory in directories {
        if !downloaded_only || directory.downloaded_book_count > 0 {
            filtered.push(LibraryFolderEntry { id: parse_uuid_column(directory.id, 0)?, name: directory.name, book_count: directory.book_count, downloaded_book_count: directory.downloaded_book_count });
        }
    }
    Ok(LibraryFolderView { children: filtered, books })
}

pub fn library_authors(conn: &Connection, selected: Option<AuthorId>, sort: AuthorSort, downloaded_only: bool) -> Result<LibraryAuthorsView, rusqlite::Error> {
    let mut shelves = get_author_book_cards_scoped(conn, downloaded_only, AUTHOR_SHELF_BOOK_COUNT)?;
    let mut subjects = get_author_subjects_scoped(conn, downloaded_only, AUTHOR_SUBJECT_COUNT)?;
    let mut authors = get_authors_scoped(conn, downloaded_only)?
        .into_iter()
        .map(|(id, name, sort_name, book_count)| {
            let books = shelves.remove(&id).unwrap_or_default();
            let subjects = subjects.remove(&id).unwrap_or_default();
            AuthorSummary::try_new(id, name, sort_name, book_count, books, subjects).map_err(|error| invalid_text_column(0, error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    sort.sort(&mut authors);
    let books = selected.map(|author_id| get_book_cards_by_author_scoped(conn, author_id, downloaded_only)).transpose()?;
    Ok(LibraryAuthorsView { authors, books })
}

pub fn library_search(conn: &Connection, query: &str, downloaded_only: bool) -> Result<LibrarySearchResult, rusqlite::Error> {
    search_book_cards_scoped(conn, query, downloaded_only)
}

pub fn library_search_folder(conn: &Connection, directory_id: &DirId, query: &str, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySearchResult, rusqlite::Error> {
    let mut result = search_book_cards_scoped(conn, query, downloaded_only)?;
    let scope = FileTypeScope::load(conn, file_type)?;
    let allowed = get_descendant_content_hashes(conn, &directory_id.to_string())?.into_iter().filter(|hash| scope.contains(hash)).collect::<HashSet<_>>();
    retain_search_results(&mut result, &allowed);
    Ok(result)
}

pub fn library_search_subject(conn: &Connection, subject_path: Option<&str>, query: &str, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySearchResult, rusqlite::Error> {
    let mut result = search_book_cards_scoped(conn, query, downloaded_only)?;
    let scope = FileTypeScope::load(conn, file_type)?;
    let Some(subject_path) = subject_path else {
        result.title_matches.retain(|book| scope.contains(&book.content_hash));
        result.author_matches.retain(|book| scope.contains(&book.content_hash));
        return Ok(result);
    };
    let mut allowed: HashSet<ContentHash> = if subject_path == PLACES_SUBJECT_PATH {
        external_kind_hashes(conn, "place")?.into_iter().collect()
    } else if let Some(wikidata_id) = external_path_id(subject_path, PLACES_SUBJECT_PATH) {
        get_external_entity_content_hashes(conn, wikidata_id)?.into_iter().collect()
    } else {
        get_descendant_subject_content_hashes(conn, subject_path)?.into_iter().collect()
    };
    allowed.retain(|hash| scope.contains(hash));
    retain_search_results(&mut result, &allowed);
    Ok(result)
}

pub fn library_subjects(conn: &Connection, subject_path: Option<&str>, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySubjectView, rusqlite::Error> {
    let scope = FileTypeScope::load(conn, file_type)?;
    if subject_path.is_some_and(|path| path == PLACES_SUBJECT_PATH || path.starts_with(&format!("{PLACES_SUBJECT_PATH}/"))) {
        return external_places(conn, subject_path.expect("checked above"), downloaded_only, &scope);
    }
    let parent_path = subject_path.unwrap_or(ROOT_SUBJECT_PATH);
    let mut books = subject_path.map(|path| get_book_cards_in_subject_scoped(conn, path, downloaded_only, "", LibraryFileTypeFilter::All)).transpose()?.unwrap_or_default();
    scope.retain_books(&mut books);
    let (mut subjects, hoisted_books) = compact_subject_children(conn, parent_path, downloaded_only, &scope)?;
    let mut seen_books = books.iter().map(|book| book.content_hash.clone()).collect::<HashSet<_>>();
    books.extend(hoisted_books.into_iter().filter(|book| seen_books.insert(book.content_hash.clone())));
    if subject_path.is_none() {
        let (mut book_count, downloaded_book_count) = if scope.is_all() { external_kind_counts(conn, "place")? } else { scope.counts(external_kind_hashes(conn, "place")?) };
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

/// Storage-backed portion of a book detail, including the book's navigation:
/// a document's is whatever import stored, an audiobook's is its chapters
/// projected into the same hierarchical form. A document with no stored row has
/// not been inspected for one yet, and comes back empty rather than wrong — the
/// caller may parse the file and store the result.
pub fn book_detail_summary(conn: &Connection, content_hash: ContentHash) -> Result<BookDetail, rusqlite::Error> {
    let book = conn.query_row(include_str!("sql/browse/book_detail_summary_select.sql"), [content_hash.as_str()], read_book_card_row)?;
    let source_directory_id = conn.query_row(include_str!("sql/browse/book_detail_summary_select_2.sql"), [content_hash.as_str()], |row| row.get::<_, String>(0)).optional()?;

    let path: Vec<library_model::BrowsePathSegment> = source_directory_id
        .as_deref()
        .map(|directory| super::assets::get_dir_path_components(conn, directory))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|component| library_model::BrowsePathSegment { id: component.id.to_string(), name: component.name.as_str().to_owned() })
        .collect();

    let mut authors = Vec::new();
    let mut statement = conn.prepare(include_str!("sql/browse/book_detail_summary_select_3.sql"))?;
    for row in statement.query_map([content_hash.as_str()], |row| {
        let id: String = row.get(0)?;
        Ok(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? })
    })? {
        authors.push(row?);
    }

    // The same table the `author_credit` view reads, with the roles that view
    // filters out. Only narrator and translator: a reader picks an audiobook by
    // its narrator and an edition by its translator, and nothing else in the
    // contributor list answers a question a reader is asking.
    let mut credits = Vec::new();
    let mut statement = conn.prepare(include_str!("sql/browse/book_detail_summary_select_4.sql"))?;
    let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
    let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
    for row in statement.query_map(rusqlite::params![content_hash.as_str(), narrator, translator], |row| {
        let id: String = row.get(0)?;
        let role: Vec<u8> = row.get(2)?;
        Ok(library_model::BookDetailCredit {
            id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
            name: row.get(1)?,
            role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
        })
    })? {
        credits.push(row?);
    }

    let mut subject_paths = Vec::new();
    let mut subjects = conn.prepare(include_str!("sql/browse/book_detail_summary_select_5.sql"))?;
    for row in subjects.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))? {
        subject_paths.push(row?);
    }

    let author_ids = authors.iter().map(|author| author.id.to_string()).collect::<Vec<_>>();
    let mut related = Vec::new();
    if !author_ids.is_empty() {
        let author_ids = serde_json::to_string(&author_ids).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let mut statement = conn.prepare(include_str!("sql/related_books.sql"))?;
        for row in statement.query_map(rusqlite::params![author_ids, content_hash.as_str()], read_book_card_row)? {
            related.push(row?);
        }
    }
    let toc = book_navigation(conn, &content_hash)?;
    let (current_entry, current_target) = current_entry(conn, &content_hash, book.format_category, &toc)?;
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
fn current_entry(conn: &Connection, content_hash: &ContentHash, format: library_model::LibraryFileTypeFilter, toc: &[library_model::BookTocEntry]) -> Result<(Option<String>, Option<String>), rusqlite::Error> {
    if format == library_model::LibraryFileTypeFilter::Audiobook {
        let Some(position) = super::reading::get_audiobook_reading_position(conn, content_hash.clone())? else { return Ok((None, None)) };
        let chapter = super::audiobooks::audiobook_chapters(conn, content_hash)?.into_iter().find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
        return Ok(match chapter {
            Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
            None => (None, None),
        });
    }
    let Some(target) = super::toc::reading_entry(conn, content_hash)? else { return Ok((None, None)) };
    Ok((super::toc::entry_title(toc, &target), Some(target)))
}

/// One shape for every medium, read from one place. An audiobook's chapters are
/// a flat ordered list, which is a tree one level deep, so nothing about a
/// format survives into how its navigation is stored or fetched.
fn book_navigation(conn: &Connection, content_hash: &ContentHash) -> Result<Vec<library_model::BookTocEntry>, rusqlite::Error> {
    Ok(super::toc::book_toc(conn, content_hash)?.unwrap_or_default())
}

/// Flatten one explicitly supplied level without inspecting promoted children.
/// Keep ungrouped chips together before headed groups, retaining input order
/// within both sections and within each group's children.
pub fn expand_chip_groups(children: &mut Vec<BrowseRow>, mut replacements: std::collections::HashMap<String, Vec<BrowseRow>>) -> Vec<library_model::BrowseChipGroup> {
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

/// Read the immediate children of all eligible folders in one snapshot/query.
/// Eligibility ignores filters: hiding a direct book must not turn its folder into a heading.
pub fn group_folder_children(conn: &Connection, children: &mut Vec<BrowseRow>, file_types: &[LibraryFileTypeFilter], languages: &[&str], sort: BrowseChipSort, downloaded_only: bool) -> rusqlite::Result<Vec<library_model::BrowseChipGroup>> {
    let ids = serde_json::to_string(&children.iter().map(|row| &row.id).collect::<Vec<_>>()).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let types = file_types_query_value(file_types);
    let languages = languages_query_value(languages);
    let mut statement = conn.prepare(include_str!("sql/browse/group_folder_children_with.sql"))?;
    let mut replacements = std::collections::HashMap::<String, Vec<BrowseRow>>::new();
    let mut rows = statement.query(rusqlite::params![ids, types, languages, chip_sort_query_value(sort), downloaded_only])?;
    while let Some(row) = rows.next()? {
        let total = row.get::<_, i64>(3)? as usize;
        let downloaded = row.get::<_, i64>(4)? as usize;
        if (downloaded_only && downloaded == 0) || ((types != 0 || !languages.is_empty()) && total == 0) {
            continue;
        }
        replacements.entry(row.get(0)?).or_default().push(BrowseRow { id: row.get(1)?, name: row.get(2)?, path: None, book_count: if downloaded_only { downloaded } else { total }, downloaded_book_count: downloaded });
    }
    Ok(expand_chip_groups(children, replacements))
}

include_sql!("src/browse/sql/browsing.sql");
include_sql!("src/browse/sql/rebuilds.sql");
include_sql!("src/browse/sql/home.sql");

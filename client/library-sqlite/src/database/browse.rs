use super::books::{get_book_cards_by_author_scoped, get_book_cards_in_dir_scoped};
use super::*;
use app_contract::library::{AuthorSort, AuthorSummary, BookDetail, BookDetailAuthor, BrowseBookSort, BrowseChipSort, BrowseRow, FolderSort, LibraryFileTypeFilter, LibraryHomeView, LibrarySearchResult, SubjectPath};
use library_model::ROOT_SUBJECT_PATH;
use rusqlite::OptionalExtension;
use std::collections::HashSet;

pub const UNCATEGORIZED_SUBJECT_PATH: &str = "__uncategorized__";
pub const OTHER_SUBJECT_SYSTEMS_PATH: &str = "__other_subject_systems__";
pub const PLACES_SUBJECT_PATH: &str = "__places__";
pub const PEOPLE_SUBJECT_PATH: &str = "__people__";

/// Subject labels that have not yet been projected into the unified taxonomy.
///
/// Keeping this query with the rest of the browse read model lets every client
/// discover taxonomy work without duplicating SQLite schema knowledge.
pub fn unmapped_subject_texts(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT MIN(TRIM(subject.name))
         FROM book_subject subject
         JOIN book ON book.row_id=subject.book_row_id
         WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
           AND TRIM(subject.name) != ''
           AND LOWER(COALESCE(TRIM(subject.authority), '')) NOT IN ('wikidata-place', 'wikidata-person')
           AND EXISTS(
               SELECT 1 FROM book_dir placement
               WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
           )
           AND NOT EXISTS(
               SELECT 1 FROM book_subject_assignment_evidence evidence
               WHERE evidence.book_row_id=subject.book_row_id
                 AND evidence.subject_position=subject.position
                 AND evidence.system_id='unified'
           )
         GROUP BY LOWER(TRIM(subject.name))
         ORDER BY LOWER(TRIM(subject.name)), TRIM(subject.name)",
    )?;
    statement.query_map([], |row| row.get(0))?.collect()
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
        let mut statement = conn.prepare(
            "SELECT book.content_hash,book.format,MAX(placement.is_downloaded)
             FROM book JOIN book_dir placement ON placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
             WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
             GROUP BY book.row_id,book.content_hash,book.format",
        )?;
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

    pub fn card_counts(&self, mut books: Vec<BookCardRow>) -> (usize, usize) {
        self.retain_books(&mut books);
        (books.len(), books.iter().filter(|book| book.downloaded).count())
    }
}

/// Produces the useful category choices immediately below a subject. Empty
/// taxonomy branches disappear. A populated route is compacted only when the
/// user has exactly one visible child at this level; siblings are never
/// bypassed or replaced by their books. Root categories are never compacted.
pub fn compact_subject_children(conn: &Connection, parent_path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<(Vec<LibrarySubjectEntry>, Vec<BookCardRow>), rusqlite::Error> {
    let mut children = populated_subject_children(conn, parent_path, downloaded_only, scope)?;
    if parent_path == ROOT_SUBJECT_PATH || children.len() != 1 {
        return Ok((children, Vec::new()));
    }

    let mut display = children.remove(0);
    loop {
        let mut exact_books = get_book_cards_in_subject_scoped(conn, display.id.as_str(), downloaded_only, "", LibraryFileTypeFilter::All)?;
        scope.retain_books(&mut exact_books);
        if !exact_books.is_empty() {
            return if display.book_count == 1 && exact_books.len() == 1 { Ok((Vec::new(), exact_books)) } else { Ok((vec![display], Vec::new())) };
        }
        let mut descendants = populated_subject_children(conn, display.id.as_str(), downloaded_only, scope)?;
        if descendants.len() != 1 {
            return Ok((vec![display], Vec::new()));
        }
        display = descendants.remove(0);
    }
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

#[cfg(all(feature = "scanner", not(target_os = "android")))]
#[derive(Clone)]
pub enum KnownDirectoryState {
    /// `None` means the library root, which has no relative directory path of its own.
    Live(Option<RelativeDirPath>),
    Deleted,
}
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub struct KnownDirectory {
    pub parent_id: DirId,
    pub name: FileName,
    pub state: KnownDirectoryState,
}
#[cfg(all(feature = "scanner", not(target_os = "android")))]
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
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub fn get_known_directory_scan_state(conn: &Connection, dir_id: &DirId) -> Result<Option<KnownDirectoryState>, rusqlite::Error> {
    let mut state = None;
    conn.get_known_directory_scan_state(&dir_id.to_string(), |row| {
        state = Some(read_known_directory_state(row, 0, 1)?);
        Ok(())
    })?;
    Ok(state)
}
#[cfg(all(feature = "scanner", not(target_os = "android")))]
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
#[cfg(all(feature = "scanner", not(target_os = "android")))]
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
    let mut cards = Vec::new();
    let languages = languages_query_value(languages);
    conn.get_book_cards_in_subject(subject_path, i32::from(downloaded_only), normalized_query, file_types_query_value(file_types), book_sort_query_value(book_sort), &languages, |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

pub fn get_uncategorized_book_cards_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_uncategorized_book_cards(i32::from(downloaded_only), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

pub fn get_other_subject_system_book_cards_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_other_subject_system_book_cards(i32::from(downloaded_only), |row| {
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
    conn.query_row(
        "SELECT COUNT(DISTINCT reference.book_row_id),COUNT(DISTINCT CASE WHEN card.downloaded THEN reference.book_row_id END)
         FROM book_external_entity reference JOIN book_cards card ON card.book_row_id=reference.book_row_id WHERE reference.kind=?1",
        [kind],
        |row| {
            let total = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "external entity book count must not be negative"))?;
            let downloaded = usize::try_from(row.get::<_, i64>(1)?).map_err(|_| invalid_integer_column(1, "external entity downloaded count must not be negative"))?;
            Ok((total, downloaded))
        },
    )
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

pub fn external_people(conn: &Connection, path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<LibrarySubjectView, rusqlite::Error> {
    if let Some(wikidata_id) = external_path_id(path, PEOPLE_SUBJECT_PATH) {
        let mut books = external_entity_books(conn, wikidata_id, downloaded_only)?;
        scope.retain_books(&mut books);
        return Ok(LibrarySubjectView { children: Vec::new(), books });
    }
    let mut raw_people = Vec::<(String, String)>::new();
    conn.get_external_people(|row| {
        raw_people.push((row.get(0)?, row.get(1)?));
        Ok(())
    })?;
    let mut children = Vec::new();
    for (wikidata_id, name) in raw_people {
        let (mut book_count, downloaded_book_count) = if scope.is_all() { external_entity_counts(conn, &wikidata_id, false)? } else { scope.counts(get_external_entity_content_hashes(conn, &wikidata_id)?) };
        if downloaded_only {
            book_count = downloaded_book_count;
        }
        if book_count > 0 {
            children.push(LibrarySubjectEntry { id: SubjectPath::new(format!("{PEOPLE_SUBJECT_PATH}/{wikidata_id}")), name, book_count, downloaded_book_count });
        }
    }
    Ok(LibrarySubjectView { children, books: Vec::new() })
}

pub fn get_external_entity_content_hashes(conn: &Connection, wikidata_id: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT book.content_hash FROM book_external_entity reference JOIN book ON book.row_id=reference.book_row_id
         WHERE reference.wikidata_id=?1 AND book.deleted_at IS NULL AND book.hidden_at IS NULL ORDER BY book.content_hash",
    )?;
    let hashes = statement.query_map([wikidata_id], |row| row.get::<_, String>(0))?.map(|value| value.map(|value| ContentHash::new(&value))).collect();
    hashes
}

pub fn external_kind_hashes(conn: &Connection, kind: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT book.content_hash FROM book_external_entity reference JOIN book ON book.row_id=reference.book_row_id
         WHERE reference.kind=?1 AND book.deleted_at IS NULL AND book.hidden_at IS NULL ORDER BY book.content_hash",
    )?;
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
    let normalized_query = library_model::normalize_search_text(query);
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

/// The covers each author's row shows, keyed by author, in the order the row
/// draws them.
pub fn get_author_cover_hashes_scoped(conn: &Connection, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<(ContentHash, String)>>, rusqlite::Error> {
    let mut covers: HashMap<AuthorId, Vec<(ContentHash, String)>> = HashMap::new();
    conn.get_author_cover_hashes(i32::from(downloaded_only), per_author, |row| {
        let raw_id: String = row.get(0)?;
        let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_hash: String = row.get(1)?;
        let title: String = row.get(2)?;
        covers.entry(id).or_default().push((ContentHash::new(&raw_hash), title));
        Ok(())
    })?;
    Ok(covers)
}

/// Shared application projection for a library home screen. Platform hosts
/// supply only the user's local-download preference.
pub fn library_home(conn: &Connection, limit: i32, downloaded_only: bool) -> Result<LibraryHomeView, rusqlite::Error> {
    let finished_threshold = f64::from(app_contract::BrowsingContentContract::default().finished_progress_threshold);
    Ok(LibraryHomeView {
        most_progress: get_most_progress_cards(conn, limit, downloaded_only, finished_threshold)?,
        recently_read: get_recently_read_cards(conn, limit, downloaded_only, finished_threshold)?,
        recently_added: get_recently_added_cards(conn, limit, downloaded_only)?,
    })
}

pub fn library_folder(conn: &Connection, directory_id: &DirId, downloaded_only: bool, file_type: LibraryFileTypeFilter) -> Result<LibraryFolderView, rusqlite::Error> {
    let scope = FileTypeScope::load(conn, file_type)?;
    let directory_id = directory_id.to_string();
    let mut directories = get_child_directories(conn, &directory_id)?;
    FolderSort::Name.sort(&mut directories);
    let mut books = get_book_cards_in_dir_scoped(conn, &directory_id, downloaded_only, "", LibraryFileTypeFilter::All)?;
    scope.retain_books(&mut books);
    let mut filtered = Vec::with_capacity(directories.len());
    for directory in directories {
        let (book_count, downloaded_book_count) = if scope.is_all() { (directory.book_count(), directory.downloaded_book_count()) } else { scope.counts(get_descendant_content_hashes(conn, &directory.id().to_string())?) };
        if scope.is_all() || book_count > 0 {
            filtered.push(LibraryFolderEntry { id: *directory.id(), name: directory.name().as_str().to_owned(), book_count, downloaded_book_count });
        }
    }
    Ok(LibraryFolderView { children: filtered, books })
}

pub fn library_authors(conn: &Connection, selected: Option<AuthorId>, sort: AuthorSort, downloaded_only: bool) -> Result<LibraryAuthorsView, rusqlite::Error> {
    let mut covers = get_author_cover_hashes_scoped(conn, downloaded_only, i32::from(app_contract::frontend::AUTHOR_ROW_COVER_COUNT))?;
    let mut authors = get_authors_scoped(conn, downloaded_only)?
        .into_iter()
        .map(|(id, name, sort_name, book_count)| {
            let (cover_hashes, cover_titles) = covers.remove(&id).unwrap_or_default().into_iter().unzip();
            AuthorSummary::try_new(id, name, sort_name, book_count, cover_hashes, cover_titles).map_err(|error| invalid_text_column(0, error.to_string()))
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
    } else if subject_path == PEOPLE_SUBJECT_PATH {
        external_kind_hashes(conn, "person")?.into_iter().collect()
    } else if let Some(wikidata_id) = external_path_id(subject_path, PLACES_SUBJECT_PATH) {
        get_external_entity_content_hashes(conn, wikidata_id)?.into_iter().collect()
    } else if let Some(wikidata_id) = external_path_id(subject_path, PEOPLE_SUBJECT_PATH) {
        get_external_entity_content_hashes(conn, wikidata_id)?.into_iter().collect()
    } else if subject_path == UNCATEGORIZED_SUBJECT_PATH {
        get_uncategorized_book_cards_scoped(conn, false)?.into_iter().map(|book| book.content_hash).collect()
    } else if subject_path == OTHER_SUBJECT_SYSTEMS_PATH {
        get_other_subject_system_book_cards_scoped(conn, false)?.into_iter().map(|book| book.content_hash).collect()
    } else {
        get_descendant_subject_content_hashes(conn, subject_path)?.into_iter().collect()
    };
    allowed.retain(|hash| scope.contains(hash));
    retain_search_results(&mut result, &allowed);
    Ok(result)
}

pub fn library_subjects(conn: &Connection, subject_path: Option<&str>, file_type: LibraryFileTypeFilter, downloaded_only: bool) -> Result<LibrarySubjectView, rusqlite::Error> {
    let scope = FileTypeScope::load(conn, file_type)?;
    if subject_path == Some(UNCATEGORIZED_SUBJECT_PATH) {
        let mut books = get_uncategorized_book_cards_scoped(conn, downloaded_only)?;
        scope.retain_books(&mut books);
        return Ok(LibrarySubjectView { children: Vec::new(), books });
    }
    if subject_path == Some(OTHER_SUBJECT_SYSTEMS_PATH) {
        let mut books = get_other_subject_system_book_cards_scoped(conn, downloaded_only)?;
        scope.retain_books(&mut books);
        return Ok(LibrarySubjectView { children: Vec::new(), books });
    }
    if subject_path.is_some_and(|path| path == PLACES_SUBJECT_PATH || path.starts_with(&format!("{PLACES_SUBJECT_PATH}/"))) {
        return external_places(conn, subject_path.expect("checked above"), downloaded_only, &scope);
    }
    if subject_path.is_some_and(|path| path == PEOPLE_SUBJECT_PATH || path.starts_with(&format!("{PEOPLE_SUBJECT_PATH}/"))) {
        return external_people(conn, subject_path.expect("checked above"), downloaded_only, &scope);
    }
    let parent_path = subject_path.unwrap_or(ROOT_SUBJECT_PATH);
    let mut books = subject_path.map(|path| get_book_cards_in_subject_scoped(conn, path, downloaded_only, "", LibraryFileTypeFilter::All)).transpose()?.unwrap_or_default();
    scope.retain_books(&mut books);
    let (mut subjects, hoisted_books) = compact_subject_children(conn, parent_path, downloaded_only, &scope)?;
    let mut seen_books = books.iter().map(|book| book.content_hash.clone()).collect::<HashSet<_>>();
    books.extend(hoisted_books.into_iter().filter(|book| seen_books.insert(book.content_hash.clone())));
    if subject_path.is_none() {
        let special = [(PLACES_SUBJECT_PATH, "Places", external_kind_hashes(conn, "place")?), (PEOPLE_SUBJECT_PATH, "Persons", external_kind_hashes(conn, "person")?)];
        for (id, name, hashes) in special {
            let (mut book_count, downloaded_book_count) = if scope.is_all() { external_kind_counts(conn, if id == PLACES_SUBJECT_PATH { "place" } else { "person" })? } else { scope.counts(hashes) };
            if downloaded_only {
                book_count = downloaded_book_count;
            }
            if book_count > 0 {
                subjects.push(LibrarySubjectEntry { id: SubjectPath::new(id), name: name.to_owned(), book_count, downloaded_book_count });
            }
        }
        // Raw assignments from subject systems that do not map into the
        // unified taxonomy stay queryable through this legacy hidden path.
        let specials = [(UNCATEGORIZED_SUBJECT_PATH, "Uncategorized", scope.card_counts(get_uncategorized_book_cards_scoped(conn, false)?))];
        for (id, name, (mut book_count, downloaded_book_count)) in specials {
            if downloaded_only {
                book_count = downloaded_book_count;
            }
            if book_count > 0 {
                subjects.push(LibrarySubjectEntry { id: SubjectPath::new(id), name: name.to_owned(), book_count, downloaded_book_count });
            }
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

/// Storage-backed portion of a book detail. TOC extraction remains with the
/// platform's publication reader because browsers and native hosts obtain the
/// bytes differently.
pub fn book_detail_summary(conn: &Connection, content_hash: ContentHash) -> Result<BookDetail, rusqlite::Error> {
    let book = conn.query_row(
        "SELECT content_hash,title,author,description,progress,downloaded,download_requested,audiobook,audiobook_duration_ms,audiobook_chapter_count,added_at,NULL FROM book_cards WHERE content_hash=?1 AND EXISTS (SELECT 1 FROM book_dir WHERE book_row_id=book_cards.book_row_id AND deleted_at IS NULL)",
        [content_hash.as_str()],
        read_book_card_row,
    )?;
    let source_directory_id =
        conn.query_row("SELECT dir_id FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND deleted_at IS NULL ORDER BY dir_id LIMIT 1", [content_hash.as_str()], |row| row.get::<_, String>(0)).optional()?;

    let mut authors = Vec::new();
    let mut statement = conn.prepare(
        "SELECT identity.stable_id,credit.credited_name FROM author_credit credit JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE credit.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY credit.position",
    )?;
    for row in statement.query_map([content_hash.as_str()], |row| {
        let id: String = row.get(0)?;
        Ok(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? })
    })? {
        authors.push(row?);
    }

    let mut subject_paths = Vec::new();
    let mut subjects = conn.prepare(
        "SELECT DISTINCT route.path FROM book_unified_concept assignment JOIN unified_concept_route route ON route.concept_id=assignment.concept_id WHERE assignment.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY route.path COLLATE NOCASE,route.path",
    )?;
    for row in subjects.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))? {
        subject_paths.push(row?);
    }

    let author_ids = authors.iter().map(|author| author.id.to_string()).collect::<Vec<_>>();
    let mut related = Vec::new();
    if !author_ids.is_empty() {
        let placeholders = std::iter::repeat_n("?", author_ids.len()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT DISTINCT card.content_hash,card.title,card.author,card.description,card.progress,card.downloaded,card.download_requested,card.audiobook,card.audiobook_duration_ms,card.audiobook_chapter_count,card.added_at,NULL FROM book_cards card JOIN author_credit credit ON credit.book_row_id=card.book_row_id JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE identity.stable_id IN ({placeholders}) AND card.content_hash<>? AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=card.book_row_id AND placement.deleted_at IS NULL) ORDER BY LOWER(card.title),card.title,card.content_hash LIMIT 12"
        );
        let mut values = author_ids.iter().map(String::as_str).collect::<Vec<_>>();
        values.push(content_hash.as_str());
        let mut statement = conn.prepare(&sql)?;
        for row in statement.query_map(rusqlite::params_from_iter(values), read_book_card_row)? {
            related.push(row?);
        }
    }
    Ok(BookDetail { book, source_directory_id, authors, subject_paths, toc: Vec::new(), related })
}

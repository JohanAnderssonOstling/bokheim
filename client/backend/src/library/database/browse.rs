use super::assets::get_dir_path_components;
use super::books::{get_book_cards_by_author_scoped, get_book_cards_in_dir_scoped};
use super::*;
use crate::app::BackendError;
use crate::library::{AuthorSort, AuthorSummary, LibraryAuthorsView, LibraryBackend, LibraryFolderEntry, LibraryFolderView, LibraryHomeView, LibrarySearchResult, LibrarySubjectEntry, LibrarySubjectView, SubjectPath};
use app_contract::catalog::{CatalogAuthorDiscoveryRequest, CatalogPublicationIdentifier, CatalogPublicationIdentifierScheme};
use app_contract::library::FolderSort;
use std::collections::HashSet;

const MAX_AUTHOR_DISCOVERY_IDENTIFIERS: usize = 2;
pub const UNCATEGORIZED_SUBJECT_PATH: &str = "__uncategorized__";
pub const OTHER_SUBJECT_SYSTEMS_PATH: &str = "__other_subject_systems__";

fn catalog_search_identifier(scheme: &str, value: String) -> Option<CatalogPublicationIdentifier> {
    let scheme = match scheme {
        "isbn" => CatalogPublicationIdentifierScheme::Isbn,
        "asin" => CatalogPublicationIdentifierScheme::Asin,
        "doi" => CatalogPublicationIdentifierScheme::Doi,
        "oclc" => CatalogPublicationIdentifierScheme::Oclc,
        "lccn" => CatalogPublicationIdentifierScheme::Lccn,
        "google" => CatalogPublicationIdentifierScheme::Google,
        "goodreads" => CatalogPublicationIdentifierScheme::Goodreads,
        "kobo" => CatalogPublicationIdentifierScheme::Kobo,
        "unspecified" if CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: value.clone() }.is_valid_search_identifier() => CatalogPublicationIdentifierScheme::Isbn,
        _ => return None,
    };
    Some(CatalogPublicationIdentifier { scheme, value })
}

impl LibraryBackend {
    pub fn home(&self, limit: i32) -> Result<LibraryHomeView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        // The same threshold both frontends split the returned list with, so
        // the limit is spent on exactly the groups they go on to render.
        let finished_threshold = f64::from(app_contract::BrowsingContentContract::default().finished_progress_threshold);
        let conn = self.db.get().map_err(BackendError::operation)?;
        let recently_read = get_recently_read_cards(&conn, limit, downloaded_only, finished_threshold).map_err(BackendError::operation)?;
        let recently_added = get_recently_added_cards(&conn, limit, downloaded_only).map_err(BackendError::operation)?;
        Ok(LibraryHomeView { recently_read, recently_added })
    }

    pub fn child_directories(&self, parent_id: &DirId) -> Result<Vec<Dir>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut directories = get_child_directories(&conn, &parent_id.to_string()).map_err(BackendError::operation)?;
        FolderSort::Name.sort(&mut directories);
        Ok(directories)
    }

    pub fn directory_path(&self, directory_id: &DirId) -> Result<Vec<Dir>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_dir_path_components(&conn, &directory_id.to_string()).map(|components| components.into_iter().map(|component| Dir::new(component.id, component.name, 0, 0)).collect()).map_err(BackendError::operation)
    }

    pub fn search(&self, query: &str) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        search_book_cards_scoped(&conn, query, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    pub fn search_folder(&self, directory_id: &DirId, query: &str) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut result = search_book_cards_scoped(&conn, query, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        let allowed = get_descendant_content_hashes(&conn, &directory_id.to_string()).map_err(BackendError::operation)?.into_iter().collect::<HashSet<_>>();
        retain_search_results(&mut result, &allowed);
        Ok(result)
    }

    pub fn search_subject(&self, subject_path: Option<&str>, query: &str) -> Result<LibrarySearchResult, BackendError> {
        let Some(subject_path) = subject_path else { return self.search(query) };
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut result = search_book_cards_scoped(&conn, query, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        let allowed = if subject_path == UNCATEGORIZED_SUBJECT_PATH {
            get_uncategorized_book_cards_scoped(&conn, false).map_err(BackendError::operation)?.into_iter().map(|book| book.content_hash).collect()
        } else if subject_path == OTHER_SUBJECT_SYSTEMS_PATH {
            get_other_subject_system_book_cards_scoped(&conn, false).map_err(BackendError::operation)?.into_iter().map(|book| book.content_hash).collect()
        } else {
            get_descendant_subject_content_hashes(&conn, subject_path).map_err(BackendError::operation)?.into_iter().collect()
        };
        retain_search_results(&mut result, &allowed);
        Ok(result)
    }

    /// The author index in the reader's chosen order. The sort lives here
    /// rather than in each frontend so that both agree on the tie-breaks —
    /// two authors with the same book count, or the same filing name.
    pub fn authors(&self, selected: Option<AuthorId>, sort: AuthorSort) -> Result<LibraryAuthorsView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut covers = get_author_cover_hashes_scoped(&conn, downloaded_only, i32::from(app_contract::frontend::AUTHOR_ROW_COVER_COUNT)).map_err(BackendError::operation)?;
        let mut authors = get_authors_scoped(&conn, downloaded_only)
            .map_err(BackendError::operation)?
            .into_iter()
            .map(|(id, name, sort_name, book_count)| AuthorSummary::try_new(id, name, sort_name, book_count, covers.remove(&id).unwrap_or_default()).map_err(BackendError::operation))
            .collect::<Result<Vec<_>, _>>()?;
        sort.sort(&mut authors);
        let books = selected.map(|author_id| get_book_cards_by_author_scoped(&conn, author_id, downloaded_only).map_err(BackendError::operation)).transpose()?;
        Ok(LibraryAuthorsView { authors, books })
    }

    pub fn author_books(&self, author_id: AuthorId) -> Result<Vec<BookCardRow>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_book_cards_by_author_scoped(&conn, author_id, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    pub(crate) fn catalog_author_discovery_request(&self, author_id: AuthorId) -> Result<CatalogAuthorDiscoveryRequest, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let (author_name, stored_identifiers) = get_author_discovery_seed(&conn, author_id).map_err(BackendError::operation)?.ok_or_else(|| BackendError::operation("author was not found"))?;
        let mut identifiers = stored_identifiers.into_iter().filter_map(|(scheme, value)| catalog_search_identifier(&scheme, value)).filter(CatalogPublicationIdentifier::is_valid_search_identifier).collect::<Vec<_>>();
        identifiers.sort_by_key(|identifier| match identifier.scheme {
            CatalogPublicationIdentifierScheme::Isbn => 0_u8,
            CatalogPublicationIdentifierScheme::Doi => 1,
            CatalogPublicationIdentifierScheme::Oclc | CatalogPublicationIdentifierScheme::Lccn => 2,
            _ => 3,
        });
        let mut seen = std::collections::HashSet::new();
        identifiers.retain(|identifier| seen.insert((identifier.scheme, identifier.normalized_value())));
        identifiers.truncate(MAX_AUTHOR_DISCOVERY_IDENTIFIERS);
        Ok(CatalogAuthorDiscoveryRequest { author_name, book_identifiers: identifiers })
    }

    pub fn folder(&self, directory_id: &DirId) -> Result<LibraryFolderView, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let directory_id = directory_id.to_string();
        let mut directories = get_child_directories(&conn, &directory_id).map_err(BackendError::operation)?;
        FolderSort::Name.sort(&mut directories);
        let books = get_book_cards_in_dir_scoped(&conn, &directory_id, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        let directories = directories
            .into_iter()
            .map(|directory| LibraryFolderEntry { id: *directory.id(), name: directory.name().as_str().to_owned(), book_count: directory.book_count(), downloaded_book_count: directory.downloaded_book_count() })
            .collect();
        Ok(LibraryFolderView { children: directories, books })
    }

    pub fn subjects(&self, subject_path: Option<&str>) -> Result<LibrarySubjectView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        if subject_path == Some(UNCATEGORIZED_SUBJECT_PATH) {
            let books = get_uncategorized_book_cards_scoped(&conn, downloaded_only).map_err(BackendError::operation)?;
            return Ok(LibrarySubjectView { children: Vec::new(), books });
        }
        if subject_path == Some(OTHER_SUBJECT_SYSTEMS_PATH) {
            let books = get_other_subject_system_book_cards_scoped(&conn, downloaded_only).map_err(BackendError::operation)?;
            return Ok(LibrarySubjectView { children: Vec::new(), books });
        }
        let parent_path = subject_path.unwrap_or("");
        let mut subjects = get_subject_children(&conn, parent_path).map_err(BackendError::operation)?;
        if subject_path.is_none() {
            let (book_count, downloaded_book_count) = get_uncategorized_counts(&conn).map_err(BackendError::operation)?;
            if book_count > 0 {
                subjects.push(LibrarySubjectEntry { id: SubjectPath::new(UNCATEGORIZED_SUBJECT_PATH), name: "Uncategorized".to_owned(), book_count, downloaded_book_count });
            }
            let (book_count, downloaded_book_count) = get_other_subject_system_counts(&conn).map_err(BackendError::operation)?;
            if book_count > 0 {
                subjects.push(LibrarySubjectEntry { id: SubjectPath::new(OTHER_SUBJECT_SYSTEMS_PATH), name: "Other subject systems".to_owned(), book_count, downloaded_book_count });
            }
        }
        subjects.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()).then_with(|| left.id.as_str().cmp(right.id.as_str())));
        let books = subject_path.map(|path| get_book_cards_in_subject_scoped(&conn, path, downloaded_only).map_err(BackendError::operation)).transpose()?.unwrap_or_default();
        Ok(LibrarySubjectView { children: subjects, books })
    }
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
fn read_known_directory_state(row: &rusqlite::Row<'_>, live_column: usize, path_column: usize) -> Result<KnownDirectoryState, rusqlite::Error> {
    let live: bool = row.get(live_column)?;
    if !live {
        return Ok(KnownDirectoryState::Deleted);
    }
    let raw_path: String = row.get(path_column)?;
    let path = if raw_path.is_empty() { None } else { Some(RelativeDirPath::parse(&raw_path).map_err(|error| invalid_text_column(path_column, error.to_string()))?) };
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
pub(super) fn get_child_directories(conn: &Connection, dir_path: &str) -> Result<Vec<Dir>, rusqlite::Error> {
    let mut sub_dirs: Vec<Dir> = Vec::new();
    conn.get_sub_dirs(dir_path, |row| {
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

pub(super) fn get_subject_children(conn: &Connection, parent_path: &str) -> Result<Vec<LibrarySubjectEntry>, rusqlite::Error> {
    let mut subjects = Vec::new();
    conn.get_subject_children(parent_path, |row| {
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

pub(super) fn get_uncategorized_counts(conn: &Connection) -> Result<(usize, usize), rusqlite::Error> {
    let mut counts = (0, 0);
    conn.get_uncategorized_subject_counts(|row| {
        let total = row.get::<_, i64>(0)?;
        let downloaded = row.get::<_, i64>(1)?;
        counts = (
            usize::try_from(total).map_err(|_| invalid_integer_column(0, "uncategorized book count must not be negative"))?,
            usize::try_from(downloaded).map_err(|_| invalid_integer_column(1, "downloaded uncategorized book count must not be negative"))?,
        );
        Ok(())
    })?;
    Ok(counts)
}

pub(super) fn get_other_subject_system_counts(conn: &Connection) -> Result<(usize, usize), rusqlite::Error> {
    let mut counts = (0, 0);
    conn.get_other_subject_system_counts(library_model::BISAC_SYSTEM_ID, |row| {
        let total = row.get::<_, i64>(0)?;
        let downloaded = row.get::<_, i64>(1)?;
        counts = (
            usize::try_from(total).map_err(|_| invalid_integer_column(0, "other-system book count must not be negative"))?,
            usize::try_from(downloaded).map_err(|_| invalid_integer_column(1, "downloaded other-system book count must not be negative"))?,
        );
        Ok(())
    })?;
    Ok(counts)
}

pub(super) fn get_book_cards_in_subject_scoped(conn: &Connection, subject_path: &str, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_book_cards_in_subject(subject_path, i32::from(downloaded_only), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

pub(super) fn get_uncategorized_book_cards_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_uncategorized_book_cards(i32::from(downloaded_only), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

pub(super) fn get_other_subject_system_book_cards_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_other_subject_system_book_cards(library_model::BISAC_SYSTEM_ID, i32::from(downloaded_only), |row| {
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
pub(super) fn get_descendant_subject_content_hashes(conn: &Connection, subject_path: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut content_hashes = Vec::new();
    conn.get_descendant_subject_content_hashes(subject_path, |row| {
        let raw: String = row.get(0)?;
        content_hashes.push(ContentHash::new(&raw));
        Ok(())
    })?;
    Ok(content_hashes)
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
pub(super) fn get_recently_read_cards(conn: &Connection, limit: i32, downloaded_only: bool, finished_threshold: f64) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_recently_read_cards(limit, i32::from(downloaded_only), finished_threshold, |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}
pub(super) fn get_recently_added_cards(conn: &Connection, limit: i32, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_recently_added_cards(limit, i32::from(downloaded_only), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}
pub(super) fn search_book_cards_scoped(conn: &Connection, query: &str, downloaded_only: bool) -> Result<LibrarySearchResult, rusqlite::Error> {
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

pub(super) fn retain_search_results(result: &mut LibrarySearchResult, allowed: &HashSet<ContentHash>) {
    result.title_matches.retain(|book| allowed.contains(&book.content_hash));
    result.author_matches.retain(|book| allowed.contains(&book.content_hash));
}
pub(super) fn get_authors_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<(AuthorId, String, Option<String>, usize)>, rusqlite::Error> {
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
pub(super) fn get_author_cover_hashes_scoped(conn: &Connection, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<ContentHash>>, rusqlite::Error> {
    let mut covers: HashMap<AuthorId, Vec<ContentHash>> = HashMap::new();
    conn.get_author_cover_hashes(i32::from(downloaded_only), per_author, |row| {
        let raw_id: String = row.get(0)?;
        let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let raw_hash: String = row.get(1)?;
        covers.entry(id).or_default().push(ContentHash::new(&raw_hash));
        Ok(())
    })?;
    Ok(covers)
}
pub(super) fn get_author_discovery_seed(conn: &Connection, author_id: AuthorId) -> Result<Option<(String, Vec<(String, String)>)>, rusqlite::Error> {
    let mut author_name = None;
    let mut identifiers = Vec::new();
    conn.get_author_discovery_seed(&author_id.to_string(), |row| {
        let name: String = row.get(0)?;
        author_name.get_or_insert(name);
        let scheme: Option<String> = row.get(1)?;
        let value: Option<String> = row.get(2)?;
        if let (Some(scheme), Some(value)) = (scheme, value) {
            identifiers.push((scheme, value));
        }
        Ok(())
    })?;
    Ok(author_name.map(|name| (name, identifiers)))
}

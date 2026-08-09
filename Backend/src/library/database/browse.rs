use super::books::{get_book_cards_by_author_scoped, get_book_cards_in_dir_scoped};
use super::*;
use crate::app::BackendError;
use crate::library::{AuthorSort, AuthorSummary, LibraryAuthorsView, LibraryBackend, LibraryFolderEntry, LibraryFolderView, LibraryHomeView};
use bokheim_app_contract::catalog::{CatalogAuthorDiscoveryRequest, CatalogPublicationIdentifier, CatalogPublicationIdentifierScheme};
use bokheim_app_contract::library::FolderSort;

const MAX_AUTHOR_DISCOVERY_IDENTIFIERS: usize = 2;

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
        let conn = self.db.get().map_err(BackendError::operation)?;
        let recently_read = get_recently_read_cards(&conn, limit, downloaded_only).map_err(BackendError::operation)?;
        let recently_added = get_recently_added_cards(&conn, limit, downloaded_only).map_err(BackendError::operation)?;
        Ok(LibraryHomeView { recently_read, recently_added })
    }

    pub fn child_directories(&self, parent_id: &str) -> Result<Vec<Dir>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut directories = get_child_directories(&conn, parent_id).map_err(BackendError::operation)?;
        FolderSort::Name.sort(&mut directories);
        Ok(directories)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<BookCardRow>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        search_book_cards_scoped(&conn, query.trim(), limit, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)
    }

    pub fn authors(&self, selected: Option<AuthorId>) -> Result<LibraryAuthorsView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut authors = get_authors_scoped(&conn, downloaded_only)
            .map_err(BackendError::operation)?
            .into_iter()
            .map(|(id, name, book_count)| AuthorSummary::try_new(id, name, book_count).map_err(BackendError::operation))
            .collect::<Result<Vec<_>, _>>()?;
        AuthorSort::Name.sort(&mut authors);
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

    pub fn folder(&self, directory_id: &str) -> Result<LibraryFolderView, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut directories = get_child_directories(&conn, directory_id).map_err(BackendError::operation)?;
        FolderSort::Name.sort(&mut directories);
        let books = get_book_cards_in_dir_scoped(&conn, directory_id, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        let directories = directories
            .into_iter()
            .map(|directory| LibraryFolderEntry { id: directory.id().as_str().to_owned(), name: directory.name().as_str().to_owned(), book_count: directory.book_count(), downloaded_book_count: directory.downloaded_book_count() })
            .collect();
        Ok(LibraryFolderView { directories, books })
    }
}
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub enum KnownDirectoryState {
    /// `None` means the library root, which has no relative directory path of its own.
    Live(Option<RelativeDirPath>),
    Deleted,
}
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub fn get_known_directory_scan_state(conn: &Connection, dir_id: &DirId) -> Result<Option<KnownDirectoryState>, rusqlite::Error> {
    let mut state = None;
    conn.get_known_directory_scan_state(dir_id.as_str(), |row| {
        let live: bool = row.get(0)?;
        state = Some(if live {
            let raw_path: String = row.get(1)?;
            let path = if raw_path.is_empty() { None } else { Some(RelativeDirPath::parse(&raw_path).map_err(|error| invalid_text_column(1, error.to_string()))?) };
            KnownDirectoryState::Live(path)
        } else {
            KnownDirectoryState::Deleted
        });
        Ok(())
    })?;
    Ok(state)
}
#[cfg(all(feature = "scanner", not(target_os = "android")))]
pub fn is_book_file_tombstoned(conn: &Connection, dir_id: &DirId, file_name: &FileName) -> Result<bool, rusqlite::Error> {
    let mut tombstoned = false;
    conn.is_book_file_tombstoned(dir_id.as_str(), file_name.as_str(), |row| {
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
        let id = DirId::parse(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let name = FileName::parse(&raw_name).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let book_count = usize::try_from(raw_book_count).map_err(|_| invalid_integer_column(2, "folder book count must not be negative"))?;
        let downloaded_book_count = usize::try_from(raw_downloaded_book_count).map_err(|_| invalid_integer_column(3, "downloaded folder book count must not be negative"))?;
        sub_dirs.push(Dir::new(id, name, book_count, downloaded_book_count));
        Ok(())
    })?;
    Ok(sub_dirs)
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
        dir_ids.push(DirId::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?);
        Ok(())
    })?;
    Ok(dir_ids)
}
pub(super) fn get_recently_read_cards(conn: &Connection, limit: i32, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_recently_read_cards(limit, i32::from(downloaded_only), |row| {
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
pub(super) fn search_book_cards_scoped(conn: &Connection, query: &str, limit: usize, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = storage_i64(u64::try_from(limit).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?, "search limit")?;
    let mut cards = Vec::new();
    conn.search_book_cards(&normalized_query, i32::from(downloaded_only), limit, |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}
pub(super) fn get_authors_scoped(conn: &Connection, downloaded_only: bool) -> Result<Vec<(AuthorId, String, usize)>, rusqlite::Error> {
    let mut authors = Vec::new();
    conn.get_authors(i32::from(downloaded_only), |row| {
        let raw_id: String = row.get(0)?;
        let id = AuthorId::parse(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
        let name: String = row.get(1)?;
        let count: i64 = row.get(2)?;
        let count = usize::try_from(count).map_err(|_| invalid_integer_column(2, "author book count must not be negative"))?;
        authors.push((id, name, count));
        Ok(())
    })?;
    Ok(authors)
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

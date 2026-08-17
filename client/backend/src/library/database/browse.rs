use super::assets::get_dir_path_components;
use super::books::{get_book_cards_by_author_scoped, get_book_cards_in_dir_scoped};
use super::*;
use crate::app::BackendError;
use crate::library::{AuthorSort, AuthorSummary, LibraryAuthorsView, LibraryBackend, LibraryFolderEntry, LibraryFolderView, LibraryHomeView, LibrarySearchResult, LibrarySubjectEntry, LibrarySubjectView, SubjectPath};
use app_contract::catalog::{CatalogAuthorDiscoveryRequest, CatalogPublicationIdentifier, CatalogPublicationIdentifierScheme};
use app_contract::library::FolderSort;
use app_contract::library::{BookDetail, BookDetailAuthor, BookTocEntry, LibraryFileTypeFilter};
use rusqlite::OptionalExtension;
use std::collections::HashSet;

const MAX_AUTHOR_DISCOVERY_IDENTIFIERS: usize = 2;
pub const UNCATEGORIZED_SUBJECT_PATH: &str = "__uncategorized__";
pub const OTHER_SUBJECT_SYSTEMS_PATH: &str = "__other_subject_systems__";
pub const PLACES_SUBJECT_PATH: &str = "__places__";
pub const PEOPLE_SUBJECT_PATH: &str = "__people__";

struct FileTypeScope {
    filter: LibraryFileTypeFilter,
    matching: HashSet<ContentHash>,
    downloaded: HashSet<ContentHash>,
}

impl FileTypeScope {
    fn load(conn: &Connection, filter: LibraryFileTypeFilter) -> Result<Self, rusqlite::Error> {
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

    fn is_all(&self) -> bool {
        self.filter == LibraryFileTypeFilter::All
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

    fn card_counts(&self, mut books: Vec<BookCardRow>) -> (usize, usize) {
        self.retain_books(&mut books);
        (books.len(), books.iter().filter(|book| book.downloaded).count())
    }
}

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
    pub fn book_detail(&self, content_hash: ContentHash) -> Result<BookDetail, BackendError> {
        let mut detail = self.book_detail_summary(content_hash.clone())?;
        detail.toc = self.book_toc(content_hash)?;
        Ok(detail)
    }

    pub fn book_detail_summary(&self, content_hash: ContentHash) -> Result<BookDetail, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut book = None;
        conn.query_row(
            "SELECT content_hash,title,author,description,progress,downloaded,download_requested,audiobook,audiobook_duration_ms,audiobook_chapter_count FROM book_cards WHERE content_hash=?1 AND EXISTS (SELECT 1 FROM book_dir WHERE book_row_id=book_cards.book_row_id AND deleted_at IS NULL)",
            [content_hash.as_str()],
            |row| super::books::read_book_card_row(row),
        ).map(|value| book = Some(value)).map_err(BackendError::operation)?;
        let book = book.ok_or_else(|| BackendError::operation("The book does not exist"))?;
        let source_directory_id = conn
            .query_row("SELECT dir_id FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND deleted_at IS NULL ORDER BY dir_id LIMIT 1", [content_hash.as_str()], |row| row.get::<_, String>(0))
            .optional()
            .map_err(BackendError::operation)?;

        let mut authors = Vec::new();
        let mut statement = conn.prepare(
            "SELECT identity.stable_id,credit.credited_name FROM author_credit credit JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE credit.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY credit.position"
        ).map_err(BackendError::operation)?;
        let rows = statement
            .query_map([content_hash.as_str()], |row| {
                let id: String = row.get(0)?;
                Ok(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? })
            })
            .map_err(BackendError::operation)?;
        for row in rows {
            authors.push(row.map_err(BackendError::operation)?);
        }

        let mut subject_paths = Vec::new();
        let mut subjects = conn
            .prepare("SELECT DISTINCT route.path FROM book_unified_concept assignment JOIN unified_concept_route route ON route.concept_id=assignment.concept_id WHERE assignment.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY route.path COLLATE NOCASE,route.path")
            .map_err(BackendError::operation)?;
        let rows = subjects.query_map([content_hash.as_str()], |row| row.get::<_, String>(0)).map_err(BackendError::operation)?;
        for row in rows {
            subject_paths.push(row.map_err(BackendError::operation)?);
        }

        let author_ids = authors.iter().map(|author| author.id.to_string()).collect::<Vec<_>>();
        let mut related = Vec::new();
        if !author_ids.is_empty() {
            let placeholders = std::iter::repeat_n("?", author_ids.len()).collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT DISTINCT card.content_hash,card.title,card.author,card.description,card.progress,card.downloaded,card.download_requested,card.audiobook,card.audiobook_duration_ms,card.audiobook_chapter_count FROM book_cards card JOIN author_credit credit ON credit.book_row_id=card.book_row_id JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE identity.stable_id IN ({placeholders}) AND card.content_hash<>? AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=card.book_row_id AND placement.deleted_at IS NULL) ORDER BY LOWER(card.title),card.title,card.content_hash LIMIT 12"
            );
            let mut values = author_ids.iter().map(String::as_str).collect::<Vec<_>>();
            values.push(content_hash.as_str());
            let mut statement = conn.prepare(&sql).map_err(BackendError::operation)?;
            let rows = statement.query_map(rusqlite::params_from_iter(values), super::books::read_book_card_row).map_err(BackendError::operation)?;
            for row in rows {
                related.push(row.map_err(BackendError::operation)?);
            }
        }
        drop(subjects);
        drop(statement);
        drop(conn);

        Ok(BookDetail { book, source_directory_id, authors, subject_paths, toc: Vec::new(), related })
    }

    pub fn book_toc(&self, content_hash: ContentHash) -> Result<Vec<BookTocEntry>, BackendError> {
        let toc = if self.is_epub(content_hash.clone())? {
            let resolved = self.resolve_book_for_reader(content_hash)?;
            epub_provider::EpubProvider::try_from_reader(resolved.reader).and_then(|provider| provider.toc()).ok().flatten().unwrap_or_default().into_iter().map(convert_toc_entry).collect()
        } else {
            Vec::new()
        };
        Ok(toc)
    }

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
        self.search_folder_filtered(directory_id, query, LibraryFileTypeFilter::All)
    }

    pub fn search_folder_filtered(&self, directory_id: &DirId, query: &str, file_type: LibraryFileTypeFilter) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut result = search_book_cards_scoped(&conn, query, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        let scope = FileTypeScope::load(&conn, file_type).map_err(BackendError::operation)?;
        let allowed = get_descendant_content_hashes(&conn, &directory_id.to_string()).map_err(BackendError::operation)?.into_iter().filter(|hash| scope.matching.contains(hash)).collect::<HashSet<_>>();
        retain_search_results(&mut result, &allowed);
        Ok(result)
    }

    pub fn search_subject(&self, subject_path: Option<&str>, query: &str) -> Result<LibrarySearchResult, BackendError> {
        self.search_subject_filtered(subject_path, query, LibraryFileTypeFilter::All)
    }

    pub fn search_subject_filtered(&self, subject_path: Option<&str>, query: &str, file_type: LibraryFileTypeFilter) -> Result<LibrarySearchResult, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut result = search_book_cards_scoped(&conn, query, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        let scope = FileTypeScope::load(&conn, file_type).map_err(BackendError::operation)?;
        let Some(subject_path) = subject_path else {
            retain_search_results(&mut result, &scope.matching);
            return Ok(result);
        };
        let mut allowed: HashSet<ContentHash> = if subject_path == PLACES_SUBJECT_PATH {
            external_kind_hashes(&conn, "place").map_err(BackendError::operation)?.into_iter().collect()
        } else if subject_path == PEOPLE_SUBJECT_PATH {
            external_kind_hashes(&conn, "person").map_err(BackendError::operation)?.into_iter().collect()
        } else if let Some(wikidata_id) = external_path_id(subject_path, PLACES_SUBJECT_PATH) {
            get_external_entity_content_hashes(&conn, wikidata_id).map_err(BackendError::operation)?.into_iter().collect()
        } else if let Some(wikidata_id) = external_path_id(subject_path, PEOPLE_SUBJECT_PATH) {
            get_external_entity_content_hashes(&conn, wikidata_id).map_err(BackendError::operation)?.into_iter().collect()
        } else if subject_path == UNCATEGORIZED_SUBJECT_PATH {
            get_uncategorized_book_cards_scoped(&conn, false).map_err(BackendError::operation)?.into_iter().map(|book| book.content_hash).collect()
        } else if subject_path == OTHER_SUBJECT_SYSTEMS_PATH {
            get_other_subject_system_book_cards_scoped(&conn, false).map_err(BackendError::operation)?.into_iter().map(|book| book.content_hash).collect()
        } else {
            get_descendant_subject_content_hashes(&conn, subject_path).map_err(BackendError::operation)?.into_iter().collect()
        };
        allowed.retain(|hash| scope.matching.contains(hash));
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
            .map(|(id, name, sort_name, book_count)| {
                let (cover_hashes, cover_titles) = covers.remove(&id).unwrap_or_default().into_iter().unzip();
                AuthorSummary::try_new(id, name, sort_name, book_count, cover_hashes, cover_titles).map_err(BackendError::operation)
            })
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
        self.folder_filtered(directory_id, LibraryFileTypeFilter::All)
    }

    pub fn folder_filtered(&self, directory_id: &DirId, file_type: LibraryFileTypeFilter) -> Result<LibraryFolderView, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let scope = FileTypeScope::load(&conn, file_type).map_err(BackendError::operation)?;
        let directory_id = directory_id.to_string();
        let mut directories = get_child_directories(&conn, &directory_id).map_err(BackendError::operation)?;
        FolderSort::Name.sort(&mut directories);
        let mut books = get_book_cards_in_dir_scoped(&conn, &directory_id, self.app()?.downloaded_books_only()?).map_err(BackendError::operation)?;
        scope.retain_books(&mut books);
        let mut filtered = Vec::with_capacity(directories.len());
        for directory in directories {
            let (book_count, downloaded_book_count) =
                if scope.is_all() { (directory.book_count(), directory.downloaded_book_count()) } else { scope.counts(get_descendant_content_hashes(&conn, &directory.id().to_string()).map_err(BackendError::operation)?) };
            if scope.is_all() || book_count > 0 {
                filtered.push(LibraryFolderEntry { id: *directory.id(), name: directory.name().as_str().to_owned(), book_count, downloaded_book_count });
            }
        }
        let directories = filtered;
        Ok(LibraryFolderView { children: directories, books })
    }

    pub fn subjects(&self, subject_path: Option<&str>) -> Result<LibrarySubjectView, BackendError> {
        self.subjects_filtered(subject_path, LibraryFileTypeFilter::All)
    }

    pub fn subjects_filtered(&self, subject_path: Option<&str>, file_type: LibraryFileTypeFilter) -> Result<LibrarySubjectView, BackendError> {
        let downloaded_only = self.app()?.downloaded_books_only()?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        let scope = FileTypeScope::load(&conn, file_type).map_err(BackendError::operation)?;
        if subject_path == Some(UNCATEGORIZED_SUBJECT_PATH) {
            let mut books = get_uncategorized_book_cards_scoped(&conn, downloaded_only).map_err(BackendError::operation)?;
            scope.retain_books(&mut books);
            return Ok(LibrarySubjectView { children: Vec::new(), books });
        }
        if subject_path == Some(OTHER_SUBJECT_SYSTEMS_PATH) {
            let mut books = get_other_subject_system_book_cards_scoped(&conn, downloaded_only).map_err(BackendError::operation)?;
            scope.retain_books(&mut books);
            return Ok(LibrarySubjectView { children: Vec::new(), books });
        }
        if subject_path.is_some_and(|path| path == PLACES_SUBJECT_PATH || path.starts_with(&format!("{PLACES_SUBJECT_PATH}/"))) {
            return external_places(&conn, subject_path.expect("checked above"), downloaded_only, &scope).map_err(BackendError::operation);
        }
        if subject_path.is_some_and(|path| path == PEOPLE_SUBJECT_PATH || path.starts_with(&format!("{PEOPLE_SUBJECT_PATH}/"))) {
            return external_people(&conn, subject_path.expect("checked above"), downloaded_only, &scope).map_err(BackendError::operation);
        }
        let parent_path = subject_path.unwrap_or("");
        let mut subjects = get_subject_children(&conn, parent_path).map_err(BackendError::operation)?;
        if !scope.is_all() {
            let mut filtered = Vec::with_capacity(subjects.len());
            for mut subject in subjects {
                let (book_count, downloaded_book_count) = scope.counts(get_descendant_subject_content_hashes(&conn, subject.id.as_str()).map_err(BackendError::operation)?);
                if book_count > 0 {
                    subject.book_count = book_count;
                    subject.downloaded_book_count = downloaded_book_count;
                    filtered.push(subject);
                }
            }
            subjects = filtered;
        }
        if subject_path.is_none() {
            let special = [(PLACES_SUBJECT_PATH, "Places", external_kind_hashes(&conn, "place").map_err(BackendError::operation)?), (PEOPLE_SUBJECT_PATH, "Persons", external_kind_hashes(&conn, "person").map_err(BackendError::operation)?)];
            for (id, name, hashes) in special {
                let (book_count, downloaded_book_count) = if scope.is_all() { external_kind_counts(&conn, if id == PLACES_SUBJECT_PATH { "place" } else { "person" }).map_err(BackendError::operation)? } else { scope.counts(hashes) };
                if scope.is_all() || book_count > 0 {
                    subjects.push(LibrarySubjectEntry { id: SubjectPath::new(id), name: name.to_owned(), book_count, downloaded_book_count });
                }
            }
            let specials = [
                (UNCATEGORIZED_SUBJECT_PATH, "Uncategorized", scope.card_counts(get_uncategorized_book_cards_scoped(&conn, false).map_err(BackendError::operation)?)),
                (OTHER_SUBJECT_SYSTEMS_PATH, "Other subject systems", scope.card_counts(get_other_subject_system_book_cards_scoped(&conn, false).map_err(BackendError::operation)?)),
            ];
            for (id, name, (book_count, downloaded_book_count)) in specials {
                if scope.is_all() || book_count > 0 {
                    subjects.push(LibrarySubjectEntry { id: SubjectPath::new(id), name: name.to_owned(), book_count, downloaded_book_count });
                }
            }
        }
        subjects.sort_by(|left, right| {
            library_model::unified_subject_sort_order(left.id.as_str())
                .cmp(&library_model::unified_subject_sort_order(right.id.as_str()))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        let mut books = subject_path.map(|path| get_book_cards_in_subject_scoped(&conn, path, downloaded_only).map_err(BackendError::operation)).transpose()?.unwrap_or_default();
        scope.retain_books(&mut books);
        Ok(LibrarySubjectView { children: subjects, books })
    }
}

fn convert_toc_entry(entry: epub_provider::TocEntry) -> BookTocEntry {
    BookTocEntry { title: entry.title, target: entry.link, children: entry.children.into_iter().map(convert_toc_entry).collect() }
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
    conn.get_other_subject_system_counts(|row| {
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
pub(super) fn get_descendant_subject_content_hashes(conn: &Connection, subject_path: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut content_hashes = Vec::new();
    conn.get_descendant_subject_content_hashes(subject_path, |row| {
        let raw: String = row.get(0)?;
        content_hashes.push(ContentHash::new(&raw));
        Ok(())
    })?;
    Ok(content_hashes)
}

fn external_path_id<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    path.strip_prefix(root)?.strip_prefix('/')?.rsplit('/').next().filter(|id| {
        (id.starts_with('Q') && id[1..].bytes().all(|byte| byte.is_ascii_digit())) || (["OLP-", "OLR-"].iter().any(|prefix| id.starts_with(prefix)) && id[4..].len() % 2 == 0 && id[4..].bytes().all(|byte| byte.is_ascii_hexdigit()))
    })
}

fn external_kind_counts(conn: &Connection, kind: &str) -> Result<(usize, usize), rusqlite::Error> {
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

fn external_entity_counts(conn: &Connection, wikidata_id: &str, recursive_places: bool) -> Result<(usize, usize), rusqlite::Error> {
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

fn external_entity_books(conn: &Connection, wikidata_id: &str, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut books = Vec::new();
    conn.get_book_cards_for_external_entity(wikidata_id, i32::from(downloaded_only), |row| {
        books.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(books)
}

fn external_places(conn: &Connection, path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<LibrarySubjectView, rusqlite::Error> {
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
        let (book_count, downloaded_book_count) = if scope.is_all() { external_entity_counts(conn, &wikidata_id, false)? } else { scope.counts(get_external_entity_content_hashes(conn, &wikidata_id)?) };
        if scope.is_all() || book_count > 0 {
            children.push(LibrarySubjectEntry { id: SubjectPath::new(format!("{PLACES_SUBJECT_PATH}/{wikidata_id}")), name, book_count, downloaded_book_count });
        }
    }
    Ok(LibrarySubjectView { children, books: Vec::new() })
}

fn external_people(conn: &Connection, path: &str, downloaded_only: bool, scope: &FileTypeScope) -> Result<LibrarySubjectView, rusqlite::Error> {
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
        let (book_count, downloaded_book_count) = if scope.is_all() { external_entity_counts(conn, &wikidata_id, false)? } else { scope.counts(get_external_entity_content_hashes(conn, &wikidata_id)?) };
        if scope.is_all() || book_count > 0 {
            children.push(LibrarySubjectEntry { id: SubjectPath::new(format!("{PEOPLE_SUBJECT_PATH}/{wikidata_id}")), name, book_count, downloaded_book_count });
        }
    }
    Ok(LibrarySubjectView { children, books: Vec::new() })
}

fn get_external_entity_content_hashes(conn: &Connection, wikidata_id: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT book.content_hash FROM book_external_entity reference JOIN book ON book.row_id=reference.book_row_id
         WHERE reference.wikidata_id=?1 AND book.deleted_at IS NULL AND book.hidden_at IS NULL ORDER BY book.content_hash",
    )?;
    let hashes = statement.query_map([wikidata_id], |row| row.get::<_, String>(0))?.map(|value| value.map(|value| ContentHash::new(&value))).collect();
    hashes
}

fn external_kind_hashes(conn: &Connection, kind: &str) -> Result<Vec<ContentHash>, rusqlite::Error> {
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
pub(super) fn get_author_cover_hashes_scoped(conn: &Connection, downloaded_only: bool, per_author: i32) -> Result<HashMap<AuthorId, Vec<(ContentHash, String)>>, rusqlite::Error> {
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

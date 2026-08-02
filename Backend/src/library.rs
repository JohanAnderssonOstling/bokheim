// `include_sql!` generates query methods with one argument per SQL parameter.
#![allow(clippy::too_many_arguments)]

use crate::db;
use crate::epub::get_epub_id_title_author;
use crate::home::BackendContext;
use crate::path_types::LibraryRoot;
use crate::scanner::{LibraryEvent, LibraryScanner};
use crate::sync::SyncManager;
use crate::{BookDownloadStatus, BookId, ContentHash, DownloadState, EpubCfi, FolderDownloadStatus, ReadProgress, ServerUrl, ServerUrlError};
use crossbeam_channel::{unbounded, Receiver, Sender};
use epub_provider::{EpubProvider, ResourceProvider};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::io::Reader as ImageReader;
use image::{ColorType, ImageEncoder, ImageFormat};
use include_sqlite_sql::{impl_sql, include_sql};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize, Serializer};
use specta::Type;
use std::cmp::Reverse;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use sync_common::{DirId, LibraryId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BookTitle(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AuthorName(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct NonNegativeTimestamp(i64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderPath {
    Root,
    Relative(crate::path_types::RelativeDirPath),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LibraryValueError(&'static str);

impl std::fmt::Display for LibraryValueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for LibraryValueError {}

fn validated_display_text(value: &str, kind: &'static str) -> Result<String, LibraryValueError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        Err(LibraryValueError(kind))
    } else {
        Ok(value.to_owned())
    }
}

impl BookTitle {
    pub fn parse(value: &str) -> Result<Self, LibraryValueError> {
        validated_display_text(value, "book title must be non-empty, bounded, and contain no control characters").map(Self)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AuthorName {
    pub fn parse(value: &str) -> Result<Self, LibraryValueError> {
        validated_display_text(value, "author name must be non-empty, bounded, and contain no control characters").map(Self)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl NonNegativeTimestamp {
    pub fn new(value: i64) -> Result<Self, LibraryValueError> {
        if value >= 0 {
            Ok(Self(value))
        } else {
            Err(LibraryValueError("timestamp must not be negative"))
        }
    }
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl FolderPath {
    pub fn parse(value: &str) -> Result<Self, LibraryValueError> {
        if value == "(Root)" || value.is_empty() {
            return Ok(Self::Root);
        }
        crate::path_types::RelativeDirPath::parse(value).map(Self::Relative).map_err(|_| LibraryValueError("folder path must be root or a valid relative directory path"))
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Root => "(Root)",
            Self::Relative(path) => path.as_str(),
        }
    }
}

impl Serialize for FolderPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Clone, Serialize, Type)]
pub struct Book {
    #[specta(type = String)]
    id: BookId,
    #[specta(type = String)]
    title: BookTitle,
    #[serde(serialize_with = "serialize_author")]
    #[specta(type = String)]
    author: Option<AuthorName>,
    cover: Option<Vec<u8>>,
    #[specta(type = specta_typescript::Number)]
    read_progress: ReadProgress,
    #[specta(type = specta_typescript::Number)]
    last_read: NonNegativeTimestamp,
    #[specta(type = specta_typescript::Number)]
    added_at: NonNegativeTimestamp,
}

/// Lightweight book metadata for IPC and list views.
///
/// Covers deliberately do not live in this type: embedding image bytes in a
/// JSON list response multiplies the payload size and forces the webview to
/// allocate the same image several times. Frontends fetch visible thumbnails
/// separately through their native transport.
#[derive(Clone, Serialize, Type)]
pub struct BookSummary {
    #[specta(type = String)]
    id: BookId,
    #[specta(type = String)]
    title: BookTitle,
    #[serde(serialize_with = "serialize_author")]
    #[specta(type = String)]
    author: Option<AuthorName>,
    #[specta(type = specta_typescript::Number)]
    read_progress: ReadProgress,
    #[specta(type = specta_typescript::Number)]
    last_read: NonNegativeTimestamp,
    #[specta(type = specta_typescript::Number)]
    added_at: NonNegativeTimestamp,
    download_state: DownloadState,
}

fn serialize_author<S: Serializer>(author: &Option<AuthorName>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(author.as_ref().map(AuthorName::as_str).unwrap_or(""))
}
#[derive(Clone, Debug, Serialize, Type)]
pub struct Dir {
    #[specta(type = String)]
    id: DirId,
    #[specta(type = String)]
    name: sync_common::FileName,
    #[specta(type = specta_typescript::Number)]
    book_count: usize,
    #[specta(type = specta_typescript::Number)]
    downloaded_book_count: usize,
}

#[derive(Clone, Serialize, Type)]
pub struct DirContent {
    pub books: Vec<Book>,
    pub dirs: Vec<Dir>,
}

#[derive(Clone, Serialize, Type)]
pub struct DirSummaryContent {
    pub books: Vec<BookSummary>,
    pub dirs: Vec<Dir>,
}

#[derive(Clone, Serialize, Type)]
pub struct AuthorSummary {
    #[specta(type = String)]
    name: AuthorName,
    #[specta(type = specta_typescript::Number)]
    book_count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthorSort {
    /// Case-insensitive ascending name, with the original spelling as a stable tie-breaker.
    #[default]
    Name,
    /// Descending book count, then ascending name.
    BookCount,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort {
    /// Case-insensitive ascending name, with the original spelling as a stable tie-breaker.
    #[default]
    Name,
    /// Descending recursive book count, then ascending name.
    BookCount,
}

#[derive(Clone, Serialize, Type)]
pub struct AuthorFolderBooks {
    #[specta(type = String)]
    folder_id: DirId,
    #[specta(type = String)]
    folder_path: FolderPath,
    books: Vec<Book>,
}

#[derive(Clone, Serialize, Type)]
pub struct AuthorFolderBookSummaries {
    #[specta(type = String)]
    folder_id: DirId,
    #[specta(type = String)]
    folder_path: FolderPath,
    books: Vec<BookSummary>,
}

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

fn log_query_timing(operation: &str, started: Instant, item_count: usize) {
    #[cfg(debug_assertions)]
    log::info!(target: "performance", "backend.query operation={operation} duration_ms={:.2} items={item_count}", started.elapsed().as_secs_f64() * 1000.0);
}

impl Book {
    pub(crate) fn try_new(id: BookId, title: String, author: String) -> Result<Self, LibraryValueError> {
        let title = BookTitle::parse(&title)?;
        let author = if author.trim().is_empty() { None } else { Some(AuthorName::parse(&author)?) };
        Ok(Self { id, title, author, cover: None, read_progress: ReadProgress::ZERO, last_read: NonNegativeTimestamp(0), added_at: NonNegativeTimestamp(0) })
    }

    pub fn id(&self) -> BookId {
        self.id
    }
    pub fn title(&self) -> &BookTitle {
        &self.title
    }
    pub fn author(&self) -> Option<&AuthorName> {
        self.author.as_ref()
    }
    pub fn cover(&self) -> Option<&[u8]> {
        self.cover.as_deref()
    }
    pub fn read_progress(&self) -> ReadProgress {
        self.read_progress
    }
    pub fn last_read(&self) -> NonNegativeTimestamp {
        self.last_read
    }
    pub fn added_at(&self) -> NonNegativeTimestamp {
        self.added_at
    }
    pub(crate) fn set_cover(&mut self, cover: Option<Vec<u8>>) {
        self.cover = cover;
    }
    pub(crate) fn set_read_progress(&mut self, value: ReadProgress) {
        self.read_progress = value;
    }
    pub(crate) fn set_last_read(&mut self, value: NonNegativeTimestamp) {
        self.last_read = value;
    }
    pub(crate) fn set_added_at(&mut self, value: NonNegativeTimestamp) {
        self.added_at = value;
    }
}

impl From<Book> for BookSummary {
    fn from(book: Book) -> Self {
        Self { id: book.id, title: book.title, author: book.author, read_progress: book.read_progress, last_read: book.last_read, added_at: book.added_at, download_state: DownloadState::NotDownloaded }
    }
}

impl BookSummary {
    pub fn id(&self) -> BookId {
        self.id
    }
    pub fn download_state(&self) -> &DownloadState {
        &self.download_state
    }
    fn set_download_state(&mut self, state: DownloadState) {
        self.download_state = state;
    }
}

impl Dir {
    pub(crate) fn new(id: DirId, name: sync_common::FileName, book_count: usize, downloaded_book_count: usize) -> Self {
        Self { id, name, book_count, downloaded_book_count }
    }
    pub fn id(&self) -> &DirId {
        &self.id
    }
    pub fn name(&self) -> &sync_common::FileName {
        &self.name
    }
    pub fn book_count(&self) -> usize {
        self.book_count
    }
    pub fn downloaded_book_count(&self) -> usize {
        self.downloaded_book_count
    }
}

impl FolderSort {
    fn sort(self, directories: &mut [Dir]) {
        match self {
            Self::Name => directories.sort_by_cached_key(|directory| (directory.name.as_str().to_lowercase(), directory.name.as_str().to_owned())),
            Self::BookCount => directories.sort_by_cached_key(|directory| (Reverse(directory.book_count), directory.name.as_str().to_lowercase(), directory.name.as_str().to_owned())),
        }
    }
}

impl AuthorSummary {
    fn try_new(name: String, book_count: usize) -> Result<Self, LibraryValueError> {
        Ok(Self { name: AuthorName::parse(&name)?, book_count })
    }
    pub fn name(&self) -> &AuthorName {
        &self.name
    }
    pub fn book_count(&self) -> usize {
        self.book_count
    }
}

impl AuthorSort {
    fn sort(self, authors: &mut [AuthorSummary]) {
        match self {
            Self::Name => authors.sort_by_cached_key(|author| (author.name.as_str().to_lowercase(), author.name.as_str().to_owned())),
            Self::BookCount => authors.sort_by_cached_key(|author| (Reverse(author.book_count), author.name.as_str().to_lowercase(), author.name.as_str().to_owned())),
        }
    }
}

impl AuthorFolderBooks {
    fn try_new(folder_id: DirId, folder_path: String, books: Vec<Book>) -> Result<Self, LibraryValueError> {
        Ok(Self { folder_id, folder_path: FolderPath::parse(&folder_path)?, books })
    }
    pub fn folder_id(&self) -> &DirId {
        &self.folder_id
    }
    pub fn folder_path(&self) -> &FolderPath {
        &self.folder_path
    }
    pub fn books(&self) -> &[Book] {
        &self.books
    }
}

impl From<AuthorFolderBooks> for AuthorFolderBookSummaries {
    fn from(group: AuthorFolderBooks) -> Self {
        Self { folder_id: group.folder_id, folder_path: group.folder_path, books: group.books.into_iter().map(BookSummary::from).collect() }
    }
}
include_sql!("src/database/create_library.sql");
include_sql!("src/database/library.sql");

pub struct Library {
    context: BackendContext,
    id: LibraryId,
    library_root: LibraryRoot,
    db_pool: Pool<SqliteConnectionManager>,
    sync_manager: Arc<SyncManager>,
    event_tx: Sender<LibraryEvent>,
    // Kept alive so its Drop implementation can stop and join scanner threads.
    _scanner: LibraryScanner,
}

pub fn sync_server_url() -> Result<ServerUrl, ServerUrlError> {
    Ok(crate::api_server_url())
}

impl Library {
    pub fn try_new(context: &BackendContext, library_path: &str, library_id: &str) -> Result<(Self, Receiver<LibraryEvent>), Box<dyn std::error::Error>> {
        Self::try_with_options(context, library_path, library_id, None, None)
    }

    pub(crate) fn try_new_with_sync_binding(context: &BackendContext, library_path: &str, library_id: &str, server_url: &str, account_id: &str) -> Result<(Self, Receiver<LibraryEvent>), Box<dyn std::error::Error>> {
        crate::auth::validate_user_id(account_id)?;
        Self::try_with_options(context, library_path, library_id, Some(ServerUrl::parse(server_url)?), Some(account_id.to_owned()))
    }

    fn try_with_options(context: &BackendContext, library_path: &str, library_id: &str, forced_server_url: Option<ServerUrl>, expected_account_id: Option<String>) -> Result<(Self, Receiver<LibraryEvent>), Box<dyn std::error::Error>> {
        let sync_enabled = forced_server_url.is_some();
        let library_root = LibraryRoot::open(Path::new(library_path))?;
        let library_id = LibraryId::parse(library_id)?;
        log::info!("Opening library {} at {}", library_id, library_root.as_path().display());
        let metadata_dir = library_root.as_path().join(crate::APP_HIDDEN_DIR);
        fs::create_dir_all(metadata_dir.join("thumbnails"))?;
        let db_path = metadata_dir.join("library.db");

        let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
            // SQLite enforces foreign keys per connection, and the pull path
            // relies on FK violations failing the transaction instead of being
            // silently accepted.
            conn.pragma_update(None, "foreign_keys", "ON")?;
            // Enable WAL mode for better concurrency
            conn.pragma_update(None, "journal_mode", "WAL")?;
            // Set busy timeout to 10 seconds (increased for large scans)
            conn.pragma_update(None, "busy_timeout", "10000")?;
            Ok(())
        });

        // Configure pool with larger size to avoid connection starvation
        let db_pool = Pool::builder()
            .max_size(20) // Increased from default 10
            .connection_timeout(std::time::Duration::from_secs(30))
            .build(manager)?;

        let client_id = {
            let conn = db_pool.get()?;
            db::init_db(&conn)?
        }; // Connection explicitly dropped here before scanner starts

        // Create event channel
        let (event_tx, event_rx) = unbounded();
        // Create scanner with event channel
        let scanner = LibraryScanner::from_root(context.clone(), library_root.clone(), db_pool.clone(), event_tx.clone())?;

        let library = Self {
            context: context.clone(),
            id: library_id.clone(),
            library_root: library_root.clone(),
            db_pool: db_pool.clone(),
            sync_manager: Arc::new(SyncManager::new(
                db_pool.clone(),
                client_id,
                match forced_server_url {
                    Some(url) => url,
                    None => sync_server_url()?,
                },
                library_id,
                library_root,
                context.app_data_dir().join("session.json"),
                sync_enabled,
                expected_account_id,
                Some(event_tx.clone()),
                context.transfer_runtime(),
            )?),
            event_tx: event_tx.clone(),
            _scanner: scanner,
        };

        Ok((library, event_rx))
    }

    pub fn id(&self) -> &LibraryId {
        &self.id
    }

    pub fn try_get_dir_contents(&self, dir_id: &str) -> Result<DirContent, Box<dyn std::error::Error>> {
        self.try_get_dir_contents_sorted(dir_id, FolderSort::Name)
    }

    pub fn try_get_dir_contents_sorted(&self, dir_id: &str, sort: FolderSort) -> Result<DirContent, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let (mut dirs, mut books) = db::get_dir_contents(&conn, dir_id)?;
        sort.sort(&mut dirs);
        for book in &mut books {
            book.set_cover(self.try_get_thumbnail(book.id())?);
        }
        Ok(DirContent { books, dirs })
    }

    pub fn try_get_dir_contents_raw(&self, dir_id: &str) -> Result<DirContent, Box<dyn std::error::Error>> {
        self.try_get_dir_contents_raw_sorted(dir_id, FolderSort::Name)
    }

    pub fn try_get_dir_contents_raw_sorted(&self, dir_id: &str, sort: FolderSort) -> Result<DirContent, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let conn = self.db_pool.get()?;
        let (mut dirs, books) = db::get_dir_contents(&conn, dir_id)?;
        sort.sort(&mut dirs);
        log_query_timing("directory", started, dirs.len() + books.len());
        Ok(DirContent { books, dirs })
    }

    pub fn try_get_dir_summary(&self, dir_id: &str) -> Result<DirSummaryContent, Box<dyn std::error::Error>> {
        self.try_get_dir_summary_sorted(dir_id, FolderSort::Name)
    }

    pub fn try_get_dir_summary_sorted(&self, dir_id: &str, sort: FolderSort) -> Result<DirSummaryContent, Box<dyn std::error::Error>> {
        let content = self.try_get_dir_contents_raw_sorted(dir_id, sort)?;
        Ok(DirSummaryContent { books: self.summarize_books(content.books)?, dirs: content.dirs })
    }

    pub fn try_get_recently_read(&self, limit: i32) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let mut books = db::get_recently_read(&conn, limit)?;
        for book in &mut books {
            book.set_cover(self.try_get_thumbnail(book.id())?);
        }
        Ok(books)
    }

    pub fn try_get_recently_read_raw(&self, limit: i32) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let conn = self.db_pool.get()?;
        let books = db::get_recently_read(&conn, limit)?;
        log_query_timing("recently_read", started, books.len());
        Ok(books)
    }

    pub fn try_get_recently_read_summaries(&self, limit: i32) -> Result<Vec<BookSummary>, Box<dyn std::error::Error>> {
        self.summarize_books(self.try_get_recently_read_raw(limit)?)
    }

    pub fn try_get_recently_added(&self, limit: i32) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let mut books = db::get_recently_added(&conn, limit)?;
        for book in &mut books {
            book.set_cover(self.try_get_thumbnail(book.id())?);
        }
        Ok(books)
    }

    pub fn try_get_recently_added_raw(&self, limit: i32) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let conn = self.db_pool.get()?;
        let books = db::get_recently_added(&conn, limit)?;
        log_query_timing("recently_added", started, books.len());
        Ok(books)
    }

    pub fn try_get_recently_added_summaries(&self, limit: i32) -> Result<Vec<BookSummary>, Box<dyn std::error::Error>> {
        self.summarize_books(self.try_get_recently_added_raw(limit)?)
    }

    pub fn try_search_books(&self, query: &str, limit: usize) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let mut books = db::search_books(&conn, query, limit)?;
        for book in &mut books {
            book.set_cover(self.try_get_thumbnail(book.id())?);
        }
        Ok(books)
    }

    pub fn try_search_books_raw(&self, query: &str, limit: usize) -> Result<Vec<Book>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let conn = self.db_pool.get()?;
        let books = db::search_books(&conn, query, limit)?;
        log_query_timing("search", started, books.len());
        Ok(books)
    }

    pub fn try_search_book_summaries(&self, query: &str, limit: usize) -> Result<Vec<BookSummary>, Box<dyn std::error::Error>> {
        self.summarize_books(self.try_search_books_raw(query, limit)?)
    }

    pub fn try_get_authors(&self) -> Result<Vec<AuthorSummary>, Box<dyn std::error::Error>> {
        self.try_get_authors_sorted(AuthorSort::Name)
    }

    pub fn try_get_authors_sorted(&self, sort: AuthorSort) -> Result<Vec<AuthorSummary>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let conn = self.db_pool.get()?;
        let mut authors = db::get_authors(&conn)?.into_iter().map(|(name, book_count)| AuthorSummary::try_new(name, book_count).map_err(Into::into)).collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        sort.sort(&mut authors);
        log_query_timing("authors", started, authors.len());
        Ok(authors)
    }

    pub fn try_get_author_folder_books(&self, author_name: &str) -> Result<Vec<AuthorFolderBooks>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let mut groups = db::get_author_books_grouped(&conn, author_name)?;
        for (_, _, books) in &mut groups {
            for book in books {
                book.set_cover(self.try_get_thumbnail(book.id())?);
            }
        }
        groups.into_iter().map(|(folder_id, folder_path, books)| AuthorFolderBooks::try_new(folder_id, folder_path, books).map_err(Into::into)).collect()
    }

    pub fn try_get_author_folder_books_raw(&self, author_name: &str) -> Result<Vec<AuthorFolderBooks>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let conn = self.db_pool.get()?;
        let groups = db::get_author_books_grouped(&conn, author_name)?
            .into_iter()
            .map(|(folder_id, folder_path, books)| AuthorFolderBooks::try_new(folder_id, folder_path, books).map_err(Into::into))
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        log_query_timing("author_books", started, groups.iter().map(|group| group.books.len()).sum());
        Ok(groups)
    }

    pub fn try_get_author_folder_book_summaries(&self, author_name: &str) -> Result<Vec<AuthorFolderBookSummaries>, Box<dyn std::error::Error>> {
        self.try_get_author_folder_books_raw(author_name)?.into_iter().map(|group| Ok(AuthorFolderBookSummaries { folder_id: group.folder_id, folder_path: group.folder_path, books: self.summarize_books(group.books)? })).collect()
    }

    fn summarize_books(&self, books: Vec<Book>) -> Result<Vec<BookSummary>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let states = db::get_book_download_states(&conn, &books.iter().map(Book::id).collect::<Vec<_>>())?;
        books
            .into_iter()
            .map(|book| {
                let book_id = book.id();
                let mut summary = BookSummary::from(book);
                let state = match states.get(&book_id).copied() {
                    Some((true, _)) => DownloadState::Downloaded,
                    Some((false, true)) => DownloadState::Queued,
                    _ => DownloadState::NotDownloaded,
                };
                summary.set_download_state(state);
                Ok(summary)
            })
            .collect()
    }

    fn persistent_download_state(conn: &rusqlite::Connection, book_id: BookId) -> Result<DownloadState, rusqlite::Error> {
        if db::is_book_downloaded(conn, book_id.clone())? {
            Ok(DownloadState::Downloaded)
        } else if db::is_book_download_requested(conn, book_id)? {
            Ok(DownloadState::Queued)
        } else {
            Ok(DownloadState::NotDownloaded)
        }
    }

    pub fn try_get_thumbnail(&self, book_id: BookId) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let mut bytes = self.context.try_get_thumbnail(book_id)?;
        if bytes.is_none() {
            let conn = self.db_pool.get()?;
            for content_hash in db::get_book_hashes(&conn, book_id)? {
                let path = crate::transfer::thumbnail_path_from_content_hash(&self.library_root, &content_hash);
                match fs::read(path) {
                    Ok(found) => {
                        bytes = Some(found);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        #[cfg(debug_assertions)]
        log::info!(target: "performance", "backend.thumbnail duration_ms={:.2} bytes={}", started.elapsed().as_secs_f64() * 1000.0, bytes.as_ref().map_or(0, Vec::len));
        Ok(bytes)
    }

    pub fn try_book_path(&self, book_id: BookId) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let Some(relative_path) = db::get_book_file_path(&conn, book_id)? else {
            return Ok(None);
        };
        Ok(Some(self.library_root.resolve_book(&relative_path)))
    }

    pub fn open_book_reader(&self, book_id: BookId) -> Result<Box<dyn ReadSeek + Send>, Box<dyn std::error::Error>> {
        let path = self.try_book_path(book_id)?.ok_or_else(|| format!("book not found: {book_id}"))?;
        let file = File::open(path)?;
        Ok(Box::new(file))
    }

    pub fn read_book_bytes(&self, book_id: BookId) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut reader = self.open_book_reader(book_id)?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub fn get_book_reading_position(&self, book_id: BookId) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(db::get_book_reading_position(&conn, book_id)?.map(EpubCfi::into_string))
    }

    pub fn get_pdf_reading_page(&self, book_id: BookId) -> Result<Option<u32>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(db::get_pdf_reading_page(&conn, book_id)?)
    }

    pub fn is_book_downloaded(&self, book_id: BookId) -> Result<bool, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(db::is_book_downloaded(&conn, book_id)?)
    }

    pub fn book_download_statuses(&self, book_ids: &[BookId]) -> Result<Vec<BookDownloadStatus>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let states = db::get_book_download_states(&conn, book_ids)?;
        book_ids
            .iter()
            .map(|book_id| {
                let state = match states.get(book_id).copied() {
                    Some((true, _)) => DownloadState::Downloaded,
                    Some((false, true)) => DownloadState::Queued,
                    _ => DownloadState::NotDownloaded,
                };
                Ok(BookDownloadStatus::new(book_id.canonical(), state))
            })
            .collect()
    }

    pub fn book_download_state(&self, book_id: BookId) -> Result<DownloadState, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(Self::persistent_download_state(&conn, book_id)?)
    }

    pub fn folder_download_statuses(&self, dir_ids: &[DirId]) -> Result<Vec<FolderDownloadStatus>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        dir_ids
            .iter()
            .map(|dir_id| {
                let (downloaded, total) = db::get_folder_download_counts(&conn, dir_id.as_str())?;
                Ok(FolderDownloadStatus::new(dir_id.as_str(), downloaded, total))
            })
            .collect()
    }

    pub fn folder_download_status(&self, dir_id: &DirId) -> Result<FolderDownloadStatus, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let (downloaded, total) = db::get_folder_download_counts(&conn, dir_id.as_str())?;
        Ok(FolderDownloadStatus::new(dir_id.as_str(), downloaded, total))
    }

    pub fn request_folder_download(&self, dir_id: &DirId) -> Result<usize, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let book_ids = db::get_descendant_book_ids(&conn, dir_id.as_str())?
            .into_iter()
            .filter_map(|book_id| match db::is_book_downloaded(&conn, book_id.clone()) {
                Ok(false) => Some(Ok(book_id)),
                Ok(true) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        drop(conn);
        self.sync_manager.request_downloads(&book_ids)?;
        Ok(book_ids.len())
    }

    pub fn update_reading_position(&self, book_id: BookId, cfi: &str) -> Result<(), Box<dyn std::error::Error>> {
        let cfi = EpubCfi::parse(cfi)?;
        let conn = self.db_pool.get()?;
        db::update_reading_position(&conn, book_id, &cfi)?;
        Ok(())
    }

    pub fn update_pdf_reading_page(&self, book_id: BookId, page_index: u32) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        db::update_pdf_reading_page(&conn, book_id, page_index)?;
        Ok(())
    }

    pub fn update_last_read(&self, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        db::update_last_read(&conn, book_id)?;
        Ok(())
    }

    pub fn mark_book_opened(&self, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
        self.update_last_read(book_id)?;
        self.event_tx.send(LibraryEvent::BookOpened { book_id })?;
        Ok(())
    }

    pub async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.sync_once().await
    }

    pub async fn sync_user_initiated(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.sync_once_with_origin(crate::TransferOrigin::UserInitiated).await
    }

    pub async fn resume_transfers(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.resume_transfers().await
    }

    pub fn request_book_download(&self, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.request_download(book_id)
    }

    pub fn user_id(&self) -> Option<String> {
        self.sync_manager.user_id()
    }

    pub fn server_url(&self) -> &ServerUrl {
        self.sync_manager.server_url()
    }

    pub fn bound_user_id(&self) -> Result<Option<String>, String> {
        self.sync_manager.bound_user_id()
    }

    pub fn is_signed_in(&self) -> bool {
        self.sync_manager.is_signed_in()
    }

    pub fn current_session(&self) -> Option<crate::auth::Session> {
        self.sync_manager.current_session()
    }

    /// Adopt a session obtained out-of-band (e.g. propagated from another live
    /// library after login). Enforces the local account binding.
    pub fn adopt_session(&self, session: crate::auth::Session) -> Result<(), String> {
        self.sync_manager.adopt_session(session)
    }

    pub fn clear_session(&self) -> Result<(), String> {
        self.sync_manager.clear_session()
    }

    pub async fn suspend_sync(&self) -> Result<(), String> {
        self.sync_manager.suspend().await
    }

    pub fn transfer_statuses(&self) -> Result<Vec<crate::TransferStatus>, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(db::get_transfer_statuses(&conn)?)
    }

    pub fn login_blocking(&self, username: &str, password: &str) -> Result<crate::auth::Session, crate::auth::AuthError> {
        let session = crate::auth::login_blocking(username, password)?;
        self.sync_manager.adopt_session(session.clone()).map_err(crate::auth::AuthError::Binding)?;
        Ok(session)
    }

    pub fn signup_blocking(&self, invite: &str, username: &str, password: &str) -> Result<crate::auth::Session, crate::auth::AuthError> {
        let session = crate::auth::signup_blocking(invite, username, password)?;
        self.sync_manager.adopt_session(session.clone()).map_err(crate::auth::AuthError::Binding)?;
        Ok(session)
    }

    pub fn logout_blocking(&self) -> Result<(), crate::auth::AuthError> {
        if let Some(s) = self.sync_manager.current_session() {
            crate::auth::logout_blocking(&s)?;
        }
        self.sync_manager.clear_session().map_err(crate::auth::AuthError::Binding)
    }

    pub fn delete_directory(&self, dir_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let dir_id = sync_common::DirId::parse(dir_id)?;
        let mut conn = self.db_pool.get()?;
        let deleted = db::delete_dir(&mut conn, &dir_id)?;
        drop(conn);
        db::remove_deleted_dir_artifacts(self.library_root.as_path(), &deleted)?;
        Ok(())
    }

    pub fn create_directory(&self, parent_id: &str, name: &str) -> Result<Dir, Box<dyn std::error::Error>> {
        let parent_id = DirId::parse(parent_id)?;
        let name = sync_common::FileName::parse(name.trim())?;
        let mut conn = self.db_pool.get()?;
        let created = db::create_local_directory(&mut conn, &self.library_root, &parent_id, &name)?;
        let _ = self.event_tx.send(LibraryEvent::DirectoryAdded { path: created.relative_path.as_str().to_owned() });
        Ok(created.directory)
    }

    /// Copies and indexes a supported book in one operation. Mobile builds do not run a
    /// filesystem watcher, so importing must not depend on a later scan.
    pub fn import_book(&self, parent_id: &str, source_path: &Path, file_name: &str) -> Result<Book, Box<dyn std::error::Error>> {
        let parent_id = DirId::parse(parent_id)?;
        let file_name = sync_common::FileName::parse(file_name.trim())?;
        let extension = Path::new(file_name.as_str()).extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        let expected_format = if extension.eq_ignore_ascii_case("epub") {
            book_format::BookFormat::Epub
        } else if extension.eq_ignore_ascii_case("pdf") {
            book_format::BookFormat::Pdf
        } else {
            return Err("only EPUB and PDF files can be added to a library".into());
        };

        let source_path = source_path.canonicalize()?;
        if !source_path.is_file() {
            return Err(format!("selected book is not a file: {}", source_path.display()).into());
        }
        let validated = book_format::validate_path(&source_path)?;
        if validated.format != expected_format {
            return Err("the selected book's extension does not match its contents".into());
        }
        let fallback_title = || Path::new(file_name.as_str()).file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
        let (title, author) = match validated.format {
            book_format::BookFormat::Epub => {
                let (_, raw_title, author) = get_epub_id_title_author(&source_path)?;
                (if raw_title.trim().is_empty() { fallback_title() } else { raw_title }, author)
            }
            book_format::BookFormat::Pdf => (fallback_title(), String::new()),
        };

        let mut conn = self.db_pool.get()?;
        let parent_is_live: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NULL)", [parent_id.as_str()], |row| row.get(0))?;
        if !parent_is_live {
            return Err(format!("library folder does not exist: {parent_id}").into());
        }
        let destination_parent = if parent_id.as_str() == "__root__" {
            self.library_root.as_path().to_path_buf()
        } else {
            db::materialize_dir_with_markers(&conn, self.library_root.as_path(), parent_id.as_str())?.ok_or_else(|| format!("could not resolve library folder: {parent_id}"))?
        };
        let destination = destination_parent.join(file_name.as_str());
        if destination.exists() {
            return Err(format!("a book named '{}' already exists in this folder", file_name.as_str()).into());
        }

        let temporary = destination_parent.join(format!("{}-import-{}.tmp", crate::APP_HIDDEN_DIR, uuid::Uuid::new_v4()));
        let mut destination_created = false;
        let copy_result = (|| -> std::io::Result<()> {
            let mut source = File::open(&source_path)?;
            let mut target = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            std::io::copy(&mut source, &mut target)?;
            target.sync_all()?;
            fs::hard_link(&temporary, &destination)?;
            destination_created = true;
            fs::remove_file(&temporary)
        })();
        if let Err(error) = copy_result {
            let _ = fs::remove_file(&temporary);
            if destination_created {
                let _ = fs::remove_file(&destination);
            }
            return Err(error.into());
        }

        let indexed = (|| -> Result<(BookId, ContentHash), Box<dyn std::error::Error>> {
            let mut hash_buffer = vec![0_u8; 1024 * 1024];
            let (book_id, content_hash) = generate_content_hash_blake3(&destination, &mut hash_buffer)?;
            let local_hash = db::local_file_fingerprint(&destination)?;
            let scan_id = db::next_scan_id(&conn)?;
            let transaction = conn.transaction()?;
            db::insert_book(&transaction, book_id)?;
            db::insert_local_book_dir(&transaction, &parent_id, book_id, &file_name, local_hash, &content_hash, scan_id)?;
            db::update_book_metadata(&transaction, book_id, &title, &author)?;
            transaction.commit()?;
            Ok((book_id, content_hash))
        })();
        let (book_id, content_hash) = match indexed {
            Ok(identity) => identity,
            Err(error) => {
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
        };
        drop(conn);

        if let Err(error) = generate_thumbnail_for_sync(&self.context, &self.library_root, &destination, book_id, &content_hash) {
            log::warn!("Could not generate cover for imported book {}: {error}", destination.display());
        }
        let book = self.try_get_dir_contents(parent_id.as_str())?.books.into_iter().find(|book| book.id() == book_id).ok_or_else(|| format!("imported book was not indexed: {book_id}"))?;
        let relative_path = destination.strip_prefix(self.library_root.as_path()).unwrap_or(&destination).to_string_lossy().into_owned();
        let _ = self.event_tx.send(LibraryEvent::BookAdded { book_id, path: relative_path });
        Ok(book)
    }

    pub fn import_epub(&self, parent_id: &str, source_path: &Path, file_name: &str) -> Result<Book, Box<dyn std::error::Error>> {
        self.import_book(parent_id, source_path, file_name)
    }
}
pub fn generate_file_hash(path: &Path) -> std::io::Result<u64> {
    const CHUNK: usize = 16 * 1024;

    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();

    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buf = vec![0u8; CHUNK];

    let n = file.read(&mut buf)?;
    hasher.update(&buf[..n]);

    if file_size > CHUNK as u64 {
        file.seek(SeekFrom::End(-(CHUNK as i64)))?;
        let n = file.read(&mut buf)?;
        hasher.update(&buf[..n]);
    }
    hasher.update(&file_size.to_le_bytes());
    Ok(hasher.digest())
}

/// Full-file blake3 of `path`, returning both the 64-bit book id (first 8 bytes
/// of the digest) and the full 64-char hex digest. The hex digest is the
/// content-addressed blob key the sync server stores and verifies against, so it
/// must be the blake3 of the whole file — see `book_dir.content_hash`.
pub fn generate_content_hash_blake3(path: &Path, buf: &mut [u8]) -> std::io::Result<(BookId, ContentHash)> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();

    loop {
        let n = file.read(buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let content_hash = ContentHash::parse(hasher.finalize().to_hex().as_ref()).expect("blake3 always produces a valid content hash");
    let book_id = BookId::from_content_hash(&content_hash);
    Ok((book_id, content_hash))
}

pub fn get_image_type(image_path: &str) -> Option<ImageFormat> {
    let file_extension = image_path.rsplit('.').next()?.to_ascii_lowercase();
    match file_extension.as_str() {
        "jpeg" => Some(ImageFormat::Jpeg),
        "jpg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "gif" => Some(ImageFormat::Gif),
        _ => None,
    }
}

pub fn generate_thumbnail_to_app_data(context: &BackendContext, epub_path: &Path, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
    generate_thumbnail(context, None, epub_path, book_id)
}

pub(crate) fn generate_thumbnail_for_sync(context: &BackendContext, library_root: &LibraryRoot, book_path: &Path, book_id: BookId, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
    let sync_path = crate::transfer::thumbnail_path_from_content_hash(library_root, content_hash);
    if book_path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")) {
        return generate_pdf_thumbnail(context, Some(sync_path.as_path()), book_path, book_id);
    }
    generate_thumbnail(context, Some(sync_path.as_path()), book_path, book_id)
}

#[cfg(feature = "desktop")]
fn generate_pdf_thumbnail(context: &BackendContext, sync_path: Option<&Path>, pdf_path: &Path, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
    let thumbnails_dir = context.thumbnails_dir()?;
    let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", book_id));
    if thumbnail_path.exists() && sync_path.is_none_or(Path::exists) {
        return Ok(());
    }
    let session = pdf_reader_core::PdfDocumentSession::open(pdf_path)?;
    let rendered = session.render_page(0, Some(300))?;
    let width = rendered.pixel_width();
    let height = rendered.pixel_height();
    let rgb = rendered.rgba().chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect::<Vec<_>>();
    publish_thumbnail_images(&thumbnail_path, sync_path, &rgb, width, height)
}

#[cfg(not(feature = "desktop"))]
fn generate_pdf_thumbnail(_: &BackendContext, _: Option<&Path>, _: &Path, _: BookId) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn generate_thumbnail(context: &BackendContext, sync_path: Option<&Path>, epub_path: &Path, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
    let thumbnails_dir = context.thumbnails_dir()?;
    let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", book_id));

    if thumbnail_path.exists() && sync_path.is_none_or(Path::exists) {
        return Ok(());
    }

    let epub = File::open(epub_path).and_then(EpubProvider::try_from_reader)?;
    let image_path = match epub.cover_image_path()? {
        Some(path) => path,
        None => {
            log::warn!("No cover image path in EPUB while generating thumbnail for book {} ({})", book_id, epub_path.display());
            return Ok(());
        }
    };

    let image_type = get_image_type(&image_path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported EPUB cover image format for book {book_id}: {image_path}")))?;
    let image_bytes = epub.read_bytes(&image_path)?;
    let decoded = ImageReader::with_format(Cursor::new(&image_bytes), image_type).decode()?.to_rgb8();

    let (src_width, src_height) = decoded.dimensions();
    let dst_width = 300;
    let scale = dst_width as f32 / src_width as f32;
    let dst_height = (src_height as f32 * scale).round() as u32;

    let src_image = Image::from_vec_u8(src_width, src_height, decoded.into_raw(), PixelType::U8x3)?;
    let mut dst_image = Image::new(dst_width, dst_height, PixelType::U8x3);

    let mut resizer = Resizer::new();
    resizer.resize(&src_image, &mut dst_image, None)?;

    publish_thumbnail_images(&thumbnail_path, sync_path, dst_image.buffer(), dst_width, dst_height)
}

fn publish_thumbnail_images(thumbnail_path: &Path, sync_path: Option<&Path>, rgb: &[u8], width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    if !thumbnail_path.exists() {
        let mut result_buf = BufWriter::new(Vec::new());
        JpegEncoder::new(&mut result_buf).write_image(rgb, width, height, ColorType::Rgb8)?;
        atomic_publish_bytes(thumbnail_path, &result_buf.into_inner().map_err(std::io::Error::other)?)?;
    }
    if let Some(sync_path) = sync_path.filter(|path| !path.exists()) {
        let mut result_buf = BufWriter::new(Vec::new());
        PngEncoder::new(&mut result_buf).write_image(rgb, width, height, ColorType::Rgb8)?;
        atomic_publish_bytes(sync_path, &result_buf.into_inner().map_err(std::io::Error::other)?)?;
    }
    Ok(())
}

fn atomic_publish_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "thumbnail target has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = PathBuf::from(format!("{}.tmp-{}", path.display(), uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        fs::remove_file(&temporary)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{generate_content_hash_blake3, generate_thumbnail_for_sync, AuthorSort, AuthorSummary, Book, BookSummary, Dir, FolderSort, Library};
    use crate::home::{AppDataLocation, BackendContext};
    use crate::path_types::LibraryRoot;
    use crate::{BookId, ContentHash};
    use image::{DynamicImage, ImageOutputFormat};
    use lopdf::{Document, Object, Stream, dictionary};
    use std::io::{Cursor, Write};
    use std::path::Path;
    use zip::write::SimpleFileOptions;

    fn write_test_pdf(path: &Path) {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let contents_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 500.into()],
            "Resources" => dictionary! {},
            "Contents" => contents_id,
        });
        document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("ID", Object::Array(vec![Object::string_literal(vec![1_u8; 16]), Object::string_literal(vec![2_u8; 16])]));
        document.save(path).unwrap();
    }

    #[test]
    fn generated_content_hash_round_trips_to_book_id() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("book.epub");
        std::fs::write(&path, b"book bytes").unwrap();
        let mut buffer = [0_u8; 16];

        let (book_id, content_hash) = generate_content_hash_blake3(&path, &mut buffer).unwrap();

        assert_eq!(BookId::from_content_hash(&content_hash), book_id);
    }

    #[test]
    fn generated_content_hash_reports_missing_files() {
        let mut buffer = [0_u8; 16];
        assert!(generate_content_hash_blake3(Path::new("definitely-missing.epub"), &mut buffer).is_err());
    }

    #[test]
    fn pdf_import_indexes_thumbnail_and_resume_page() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        let source = temp.path().join("paper.pdf");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        write_test_pdf(&source);
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        let (library, _) = Library::try_new(&context, library_dir.to_str().unwrap(), "pdf-test").unwrap();

        let book = library.import_book("__root__", &source, "paper.pdf").unwrap();
        assert_eq!(book.title().as_str(), "paper");
        assert!(library.try_book_path(book.id()).unwrap().unwrap().ends_with("paper.pdf"));
        library.update_pdf_reading_page(book.id(), 0).unwrap();
        assert_eq!(library.get_pdf_reading_page(book.id()).unwrap(), Some(0));
        assert!(app_data.join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{}.jpg", book.id())).exists());
    }

    #[test]
    fn book_summary_serialization_never_embeds_cover_bytes() {
        let mut book = Book::try_new(BookId::new(42), "A title".to_string(), "An author".to_string()).unwrap();
        book.set_cover(Some(vec![0xff, 0xd8, 0xff, 0xd9]));

        let serialized = serde_json::to_value(BookSummary::from(book)).unwrap();

        assert_eq!(serialized["id"], "42");
        assert!(serialized.get("cover").is_none());
    }

    #[test]
    fn author_name_sort_is_case_insensitive_and_deterministic() {
        let mut authors = vec![AuthorSummary::try_new("zelda".to_string(), 3).unwrap(), AuthorSummary::try_new("Alice".to_string(), 1).unwrap(), AuthorSummary::try_new("alice".to_string(), 2).unwrap()];

        AuthorSort::Name.sort(&mut authors);

        assert_eq!(authors.iter().map(|author| author.name().as_str()).collect::<Vec<_>>(), vec!["Alice", "alice", "zelda"]);
    }

    #[test]
    fn author_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut authors = vec![AuthorSummary::try_new("Zelda".to_string(), 2).unwrap(), AuthorSummary::try_new("Alice".to_string(), 7).unwrap(), AuthorSummary::try_new("Bob".to_string(), 2).unwrap()];

        AuthorSort::BookCount.sort(&mut authors);

        assert_eq!(authors.iter().map(|author| (author.name().as_str(), author.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }

    #[test]
    fn folder_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut directories = vec![
            Dir::new(sync_common::DirId::parse("z").unwrap(), sync_common::FileName::parse("Zelda").unwrap(), 2, 0),
            Dir::new(sync_common::DirId::parse("a").unwrap(), sync_common::FileName::parse("Alice").unwrap(), 7, 0),
            Dir::new(sync_common::DirId::parse("b").unwrap(), sync_common::FileName::parse("Bob").unwrap(), 2, 0),
        ];

        FolderSort::BookCount.sort(&mut directories);

        assert_eq!(directories.iter().map(|directory| (directory.name().as_str(), directory.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }

    #[test]
    fn thumbnail_generation_populates_ui_and_content_addressed_sync_caches_deterministically() {
        let temp = tempfile::tempdir().unwrap();
        let app_data = temp.path().join("app");
        let library_dir = temp.path().join("library");
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&library_dir).unwrap();
        let epub_path = library_dir.join("book.epub");

        let mut cover = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 3).write_to(&mut cover, ImageOutputFormat::Png).unwrap();
        let file = std::fs::File::create(&epub_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("mimetype", options.compression_method(zip::CompressionMethod::Stored)).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", options).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("OEBPS/content.opf", options).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#)
            .unwrap();
        archive.start_file("OEBPS/cover.png", options).unwrap();
        archive.write_all(cover.get_ref()).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", options).unwrap();
        archive.write_all(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>book</body></html>").unwrap();
        archive.finish().unwrap();

        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.clone())).unwrap();
        let library_root = LibraryRoot::open(&library_dir).unwrap();
        let content_hash = ContentHash::parse(blake3::hash(&std::fs::read(&epub_path).unwrap()).to_hex().as_str()).unwrap();
        let book_id = BookId::from_content_hash(&content_hash);
        generate_thumbnail_for_sync(&context, &library_root, &epub_path, book_id, &content_hash).unwrap();

        let ui_bytes = std::fs::read(app_data.join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{book_id}.jpg"))).unwrap();
        let sync_path = crate::transfer::thumbnail_path_from_content_hash(&library_root, &content_hash);
        let sync_bytes = std::fs::read(&sync_path).unwrap();
        assert_eq!(&ui_bytes[..2], &[0xff, 0xd8]);
        assert_eq!(&sync_bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let dimensions = image::load_from_memory_with_format(&sync_bytes, image::ImageFormat::Png).unwrap().to_rgb8().dimensions();
        assert_eq!(dimensions, (300, 450));

        generate_thumbnail_for_sync(&context, &library_root, &epub_path, book_id, &content_hash).unwrap();
        assert_eq!(std::fs::read(sync_path).unwrap(), sync_bytes);
    }
}

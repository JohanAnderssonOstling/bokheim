use crate::download::DownloadState;
use domain::{AuthorName, BookTitle, FolderPath, LibraryValueError, NonNegativeTimestamp, ReadProgress};
use serde::{Deserialize, Serialize, Serializer};
use specta::Type;
use std::cmp::Reverse;
use sync_common::{AuthorId, ContentHash, DirId, LibraryId};

/// The complete, stable destination for a book import.
/// Frontends must choose both identities before starting an asynchronous
/// download. The backend still verifies their relationship against the live
/// library database when the import is committed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct BookImportDestination {
    #[specta(type = String)]
    library_id: LibraryId,
    #[specta(type = String)]
    directory_id: DirId,
}

impl BookImportDestination {
    pub fn new(library_id: LibraryId, directory_id: DirId) -> Self {
        Self { library_id, directory_id }
    }

    pub fn parse(library_id: &str, directory_id: &str) -> Result<Self, sync_common::DomainTextError> {
        Ok(Self::new(LibraryId::parse(library_id)?, DirId::parse(directory_id)?))
    }

    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn directory_id(&self) -> &DirId {
        &self.directory_id
    }
}

#[derive(Clone, Serialize, Type)]
pub struct Book {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    #[specta(type = String)]
    pub title: BookTitle,
    #[serde(serialize_with = "serialize_author")]
    #[specta(type = String)]
    pub author: Option<AuthorName>,
    pub description: String,
    pub cover: Option<Vec<u8>>,
    #[specta(type = specta_typescript::Number)]
    pub read_progress: ReadProgress,
    #[specta(type = specta_typescript::Number)]
    pub added_at: NonNegativeTimestamp,
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
    pub content_hash: ContentHash,
    #[specta(type = String)]
    pub title: BookTitle,
    #[serde(serialize_with = "serialize_author")]
    #[specta(type = String)]
    pub author: Option<AuthorName>,
    #[specta(type = specta_typescript::Number)]
    pub read_progress: ReadProgress,
    #[specta(type = specta_typescript::Number)]
    pub added_at: NonNegativeTimestamp,
    pub download_state: DownloadState,
}

fn serialize_author<S: Serializer>(author: &Option<AuthorName>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(author.as_ref().map(AuthorName::as_str).unwrap_or(""))
}
#[derive(Clone, Debug, Serialize, Type)]
pub struct Dir {
    #[specta(type = String)]
    pub id: DirId,
    #[specta(type = String)]
    pub name: sync_common::FileName,
    #[specta(type = specta_typescript::Number)]
    pub book_count: usize,
    #[specta(type = specta_typescript::Number)]
    pub downloaded_book_count: usize,
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
    pub id: AuthorId,
    #[specta(type = String)]
    pub name: AuthorName,
    #[specta(type = specta_typescript::Number)]
    pub book_count: usize,
}

/// Everything a browsing card renders for one book.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct BookCardRow {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub title: String,
    pub author: String,
    pub description: String,
    pub progress: f32,
    pub downloaded: bool,
    pub download_requested: bool,
    pub audiobook: bool,
    #[specta(type = Option<specta_typescript::Number>)]
    pub audiobook_duration_ms: Option<u64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub audiobook_chapter_count: Option<usize>,
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
    pub folder_id: DirId,
    #[specta(type = String)]
    pub folder_path: FolderPath,
    pub books: Vec<Book>,
}

#[derive(Clone, Serialize, Type)]
pub struct AuthorFolderBookSummaries {
    #[specta(type = String)]
    pub folder_id: DirId,
    #[specta(type = String)]
    pub folder_path: FolderPath,
    pub books: Vec<BookSummary>,
}

impl Book {
    pub fn try_new(content_hash: ContentHash, title: String, author: String) -> Result<Self, LibraryValueError> {
        let title = BookTitle::parse(&title)?;
        let author = if author.trim().is_empty() { None } else { Some(AuthorName::parse(&author)?) };
        Ok(Self { content_hash, title, author, description: String::new(), cover: None, read_progress: ReadProgress::ZERO, added_at: NonNegativeTimestamp::ZERO })
    }

    pub fn content_hash(&self) -> ContentHash {
        self.content_hash.clone()
    }
    pub fn title(&self) -> &BookTitle {
        &self.title
    }
    pub fn author(&self) -> Option<&AuthorName> {
        self.author.as_ref()
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn set_description(&mut self, value: String) {
        self.description = value;
    }
    pub fn cover(&self) -> Option<&[u8]> {
        self.cover.as_deref()
    }
    pub fn read_progress(&self) -> ReadProgress {
        self.read_progress
    }
    pub fn added_at(&self) -> NonNegativeTimestamp {
        self.added_at
    }
    pub fn set_cover(&mut self, cover: Option<Vec<u8>>) {
        self.cover = cover;
    }
    pub fn set_read_progress(&mut self, value: ReadProgress) {
        self.read_progress = value;
    }
    pub fn set_added_at(&mut self, value: NonNegativeTimestamp) {
        self.added_at = value;
    }
}

impl From<Book> for BookSummary {
    fn from(book: Book) -> Self {
        Self { content_hash: book.content_hash, title: book.title, author: book.author, read_progress: book.read_progress, added_at: book.added_at, download_state: DownloadState::NotDownloaded }
    }
}

impl BookSummary {
    pub fn content_hash(&self) -> ContentHash {
        self.content_hash.clone()
    }
    pub fn download_state(&self) -> &DownloadState {
        &self.download_state
    }
    pub fn set_download_state(&mut self, state: DownloadState) {
        self.download_state = state;
    }
}

impl Dir {
    pub fn new(id: DirId, name: sync_common::FileName, book_count: usize, downloaded_book_count: usize) -> Self {
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
    pub fn sort(self, directories: &mut [Dir]) {
        match self {
            Self::Name => directories.sort_by_cached_key(|directory| (directory.name.as_str().to_lowercase(), directory.name.as_str().to_owned())),
            Self::BookCount => directories.sort_by_cached_key(|directory| (Reverse(directory.book_count), directory.name.as_str().to_lowercase(), directory.name.as_str().to_owned())),
        }
    }
}

impl AuthorSummary {
    pub fn try_new(id: AuthorId, name: String, book_count: usize) -> Result<Self, LibraryValueError> {
        Ok(Self { id, name: AuthorName::parse(&name)?, book_count })
    }
    pub fn id(&self) -> AuthorId {
        self.id
    }
    pub fn name(&self) -> &AuthorName {
        &self.name
    }
    pub fn book_count(&self) -> usize {
        self.book_count
    }
}

impl AuthorSort {
    pub fn sort(self, authors: &mut [AuthorSummary]) {
        match self {
            Self::Name => authors.sort_by_cached_key(|author| (author.name.as_str().to_lowercase(), author.name.as_str().to_owned(), author.id.to_string())),
            Self::BookCount => authors.sort_by_cached_key(|author| (Reverse(author.book_count), author.name.as_str().to_lowercase(), author.name.as_str().to_owned(), author.id.to_string())),
        }
    }
}

impl AuthorFolderBooks {
    pub fn try_new(folder_id: DirId, folder_path: String, books: Vec<Book>) -> Result<Self, LibraryValueError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_destination_round_trips_validated_identities() {
        let destination = BookImportDestination::parse("library-1", "folder-1").unwrap();
        let json = serde_json::to_string(&destination).unwrap();
        let restored: BookImportDestination = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, destination);
        assert_eq!(restored.library_id().as_str(), "library-1");
        assert_eq!(restored.directory_id().as_str(), "folder-1");
    }

    #[test]
    fn import_destination_rejects_invalid_boundary_values() {
        assert!(BookImportDestination::parse("", "folder-1").is_err());
        assert!(BookImportDestination::parse("library-1", "").is_err());
        assert!(serde_json::from_str::<BookImportDestination>(r#"{"library_id":"library-1","directory_id":"bad\nfolder"}"#).is_err());
        assert!(serde_json::from_str::<BookImportDestination>(r#"{"library_id":"library-1","directory_id":"folder-1","extra":true}"#).is_err());
    }

    #[test]
    fn author_summary_exposes_identity_independently_from_display_name() {
        let first_id = AuthorId::from_migration_seed(b"first Alex Smith");
        let second_id = AuthorId::from_migration_seed(b"second Alex Smith");
        let first = AuthorSummary::try_new(first_id, "Alex Smith".to_owned(), 1).unwrap();
        let second = AuthorSummary::try_new(second_id, "Alex Smith".to_owned(), 2).unwrap();

        assert_eq!(first.name(), second.name());
        assert_ne!(first.id(), second.id());
        assert_eq!(serde_json::to_value(first).unwrap()["id"], first_id.to_string());
        assert_eq!(serde_json::to_value(second).unwrap()["id"], second_id.to_string());
    }
}

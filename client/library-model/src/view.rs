use crate::{AuthorId, AuthorName, AuthorNameError, ContentHash};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

type DirId = uuid::Uuid;
type LibraryId = uuid::Uuid;

/// Percentage at which a reading position is presented as finished.
pub const FINISHED_PROGRESS_THRESHOLD: f32 = 98.0;

/// Stable runtime identity for a book within one configured library.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BookLocator {
    library_id: LibraryId,
    content_hash: ContentHash,
}

impl BookLocator {
    pub fn new(library_id: LibraryId, content_hash: ContentHash) -> Self {
        Self { library_id, content_hash }
    }

    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn content_hash(&self) -> ContentHash {
        self.content_hash.clone()
    }
}

/// A change relevant to a currently displayed library page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryUpdate {
    Contents,
    Covers(Vec<ContentHash>),
}

/// Stable identity for a location in Bokheim's subject hierarchy.
/// This is a taxonomy path, not a filesystem path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubjectPath(String);

impl SubjectPath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SubjectPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Limits hierarchical library browsing to one broad publication family.
/// `Book` intentionally means every supported reading format except PDF and
/// M4B, which have dedicated filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryFileTypeFilter {
    #[default]
    All,
    Book,
    Pdf,
    Audiobook,
}

impl LibraryFileTypeFilter {
    pub const ALL: [Self; 4] = [Self::All, Self::Book, Self::Pdf, Self::Audiobook];

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Book => "Books",
            Self::Pdf => "PDF",
            Self::Audiobook => "Audiobooks",
        }
    }

    pub fn matches_extension(self, extension: &str) -> bool {
        match self {
            Self::All => true,
            Self::Pdf => extension.eq_ignore_ascii_case("pdf"),
            Self::Audiobook => extension.eq_ignore_ascii_case("m4b"),
            Self::Book => matches!(extension.to_ascii_lowercase().as_str(), "epub" | "mobi" | "azw" | "azw3"),
        }
    }
}

/// Independent ordering for hierarchy chips on the shared browse page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowseChipSort {
    #[default]
    Alphabetical,
    BookCount,
}

impl BrowseChipSort {
    pub const ALL: [Self; 2] = [Self::Alphabetical, Self::BookCount];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "Alphabetical",
            Self::BookCount => "Book count",
        }
    }
}

/// Independent ordering for book cards on the shared browse page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowseBookSort {
    #[default]
    Alphabetical,
    RecentlyRead,
}

impl BrowseBookSort {
    pub const ALL: [Self; 2] = [Self::Alphabetical, Self::RecentlyRead];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => "Alphabetical",
            Self::RecentlyRead => "Recently read",
        }
    }
}

/// One directly browsable child returned by a library hierarchy query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseRow {
    pub id: String,
    pub name: String,
    /// Parent hierarchy for scoped search results; absent for direct children.
    pub path: Option<String>,
    pub book_count: usize,
    pub downloaded_book_count: usize,
}

/// Direct contents for one hierarchy location.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowseContents {
    pub children: Vec<BrowseRow>,
    pub books: Vec<BookCardRow>,
}

/// Typed input shared by folder and subject browsing implementations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBrowseQuery {
    pub location: String,
    pub search: String,
    pub file_types: Vec<LibraryFileTypeFilter>,
    pub chip_sort: BrowseChipSort,
    pub book_sort: BrowseBookSort,
    pub languages: Vec<String>,
}

#[cfg(test)]
mod file_type_filter_tests {
    use super::LibraryFileTypeFilter;

    #[test]
    fn broad_book_filter_excludes_dedicated_pdf_and_audiobook_formats() {
        assert!(LibraryFileTypeFilter::Book.matches_extension("epub"));
        assert!(!LibraryFileTypeFilter::Book.matches_extension("cbz"));
        assert!(!LibraryFileTypeFilter::Book.matches_extension("pdf"));
        assert!(!LibraryFileTypeFilter::Book.matches_extension("m4b"));
        assert!(LibraryFileTypeFilter::Pdf.matches_extension("PDF"));
        assert!(LibraryFileTypeFilter::Audiobook.matches_extension("M4B"));
    }
}

/// The complete, stable destination for a book import.
/// Frontends must choose both identities before starting an asynchronous
/// download. The backend still verifies their relationship against the live
/// library database when the import is committed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookImportDestination {
    library_id: LibraryId,
    directory_id: DirId,
}

impl BookImportDestination {
    pub fn new(library_id: LibraryId, directory_id: DirId) -> Self {
        Self { library_id, directory_id }
    }

    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn directory_id(&self) -> &DirId {
        &self.directory_id
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Dir {
    pub id: DirId,
    pub name: String,
    pub book_count: usize,
    pub downloaded_book_count: usize,
}

#[derive(Clone, Serialize)]
pub struct AuthorSummary {
    pub id: AuthorId,
    pub name: AuthorName,
    /// The filing form — "Ekman, Kerstin" — that the index sorts and groups by.
    /// Falls back to the display name for books that carry no `file-as`, so
    /// this is never empty and callers never have to decide again.
    pub sort_name: String,
    pub book_count: usize,
    /// Books displayed on the author's card, most recently read first. Each
    /// content hash identifies both its cover thumbnail and its detail page,
    /// capped at
    /// The presentation may choose how many of these covers to display.
    pub cover_hashes: Vec<ContentHash>,
    /// Titles parallel to `cover_hashes`, used when a displayed book has no
    /// cover artwork. Both vectors are produced by the same ordered query.
    pub cover_titles: Vec<String>,
}

/// Everything a browsing card renders for one book.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BookCardRow {
    pub content_hash: ContentHash,
    pub title: String,
    pub author: String,
    pub description: String,
    pub progress: f32,
    pub downloaded: bool,
    pub download_requested: bool,
    pub audiobook: bool,
    pub audiobook_duration_ms: Option<u64>,
    pub audiobook_chapter_count: Option<usize>,
    /// Milliseconds since the Unix epoch when this book entered the library.
    /// Legacy rows without a timestamp use zero and sort after dated books.
    pub added_at: i64,
    /// Concrete placement represented by a folder-browse result. Cards from
    /// placement-neutral views such as Home and Subjects leave this absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_directory: Option<DirId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryHomeView {
    /// Unfinished books ordered by progress, highest first.
    #[serde(default)]
    pub most_progress: Vec<BookCardRow>,
    pub recently_read: Vec<BookCardRow>,
    pub recently_added: Vec<BookCardRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashedBook {
    pub content_hash: ContentHash,
    pub title: String,
    pub format: String,
    pub deleted_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashedFolder {
    pub id: DirId,
    pub name: String,
    pub deleted_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryTrashView {
    pub books: Vec<TrashedBook>,
    pub folders: Vec<TrashedFolder>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryFolderDestination {
    pub id: DirId,
    pub parent_id: Option<DirId>,
    pub label: String,
    /// Human-readable hierarchy for flat selectors. This is not a filesystem
    /// location; hierarchical selectors must follow `parent_id`.
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionIdentityReviewStatus {
    Ambiguous,
    NoMatch,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditionIdentityReviewCandidate {
    pub isbn13: String,
    pub edition_id: String,
    pub work_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub matched_publisher: bool,
    pub matched_publication_year: bool,
    /// External classifications resolved to Bokheim taxonomy paths.
    pub classifications: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditionIdentityReview {
    pub content_hash: ContentHash,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub inferred_title: Option<String>,
    pub inferred_author: Option<String>,
    pub inferred_publication_year: Option<i32>,
    pub status: EditionIdentityReviewStatus,
    pub detail: Option<String>,
    pub candidates: Vec<EditionIdentityReviewCandidate>,
    pub candidates_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudiobookChapter {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// One credited author on a book. Only `aut` contributor roles are projected
/// here; editors, translators and narrators remain contributor metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BookDetailAuthor {
    pub id: AuthorId,
    pub name: String,
}

/// A reader target in the publication's own hierarchical navigation model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookTocEntry {
    pub title: String,
    pub target: String,
    pub children: Vec<BookTocEntry>,
}

pub const AUDIOBOOK_TOC_TARGET_PREFIX: &str = "audiobook-position-ms:";

pub fn audiobook_toc_target(position_ms: u64) -> String {
    format!("{AUDIOBOOK_TOC_TARGET_PREFIX}{position_ms}")
}

pub fn audiobook_toc_position(target: &str) -> Option<u64> {
    target.strip_prefix(AUDIOBOOK_TOC_TARGET_PREFIX)?.parse().ok()
}

/// Complete initial payload for a book detail screen. Related books are kept
/// in the same response for native callers; browser workers may still resolve
/// the command independently from route rendering.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BookDetail {
    pub book: BookCardRow,
    /// One live placement that can act as the source for copy/move actions.
    /// A detail screen is not itself scoped to a folder.
    pub source_directory_id: Option<String>,
    pub authors: Vec<BookDetailAuthor>,
    /// Canonical full paths. Frontends display their leaf and use the complete
    /// value for navigation and accessible names.
    pub subject_paths: Vec<String>,
    pub toc: Vec<BookTocEntry>,
    pub related: Vec<BookCardRow>,
}

/// One library search with title matches kept ahead of distinct author-only
/// matches. A book that matches both is returned only in `title_matches`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LibrarySearchResult {
    pub title_matches: Vec<BookCardRow>,
    pub author_matches: Vec<BookCardRow>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorSort {
    /// Case-insensitive ascending name, with the original spelling as a stable tie-breaker.
    #[default]
    Name,
    /// Descending book count, then ascending name.
    BookCount,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort {
    /// Case-insensitive ascending name, with the original spelling as a stable tie-breaker.
    #[default]
    Name,
    /// Descending recursive book count, then ascending name.
    BookCount,
}

impl Dir {
    pub fn new(id: DirId, name: String, book_count: usize, downloaded_book_count: usize) -> Self {
        Self { id, name, book_count, downloaded_book_count }
    }
    pub fn id(&self) -> &DirId {
        &self.id
    }
    pub fn name(&self) -> &String {
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
    /// `sort_name` is the stored filing form, if the source had one. An empty
    /// or absent one resolves to the display name here, once, so that every
    /// caller filing, grouping or sorting by it gets the same answer.
    pub fn try_new(id: AuthorId, name: String, sort_name: Option<String>, book_count: usize, cover_hashes: Vec<ContentHash>, cover_titles: Vec<String>) -> Result<Self, AuthorNameError> {
        let name = AuthorName::parse(&name)?;
        let sort_name = sort_name.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()).unwrap_or_else(|| name.as_str().to_owned());
        let cover_titles = cover_hashes.iter().enumerate().map(|(index, _)| cover_titles.get(index).map(|title| title.trim()).filter(|title| !title.is_empty()).unwrap_or("Untitled").to_owned()).collect();
        Ok(Self { id, name, sort_name, book_count, cover_hashes, cover_titles })
    }
    pub fn id(&self) -> AuthorId {
        self.id
    }
    pub fn name(&self) -> &AuthorName {
        &self.name
    }
    pub fn sort_name(&self) -> &str {
        &self.sort_name
    }
    pub fn book_count(&self) -> usize {
        self.book_count
    }
    pub fn cover_hashes(&self) -> &[ContentHash] {
        &self.cover_hashes
    }
    pub fn cover_titles(&self) -> &[String] {
        &self.cover_titles
    }
}

impl AuthorSort {
    pub fn sort(self, authors: &mut [AuthorSummary]) {
        match self {
            Self::Name => authors.sort_by_cached_key(|author| (author.sort_name.to_lowercase(), author.sort_name.clone(), author.id.to_string())),
            Self::BookCount => authors.sort_by_cached_key(|author| (Reverse(author.book_count), author.sort_name.to_lowercase(), author.sort_name.clone(), author.id.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_id_from_migration_seed;

    fn fixture_id(label: &str) -> uuid::Uuid {
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, label.as_bytes())
    }

    #[test]
    fn import_destination_round_trips_identities() {
        let destination = BookImportDestination::new(fixture_id("library-1"), fixture_id("folder-1"));
        let json = serde_json::to_string(&destination).unwrap();
        let restored: BookImportDestination = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, destination);
        assert_eq!(restored.library_id(), destination.library_id());
        assert_eq!(restored.directory_id(), destination.directory_id());
    }

    #[test]
    fn import_destination_rejects_unknown_fields() {
        assert!(serde_json::from_str::<BookImportDestination>(r#"{"library_id":"library-1","directory_id":"folder-1","extra":true}"#).is_err());
    }

    #[test]
    fn audiobook_toc_targets_round_trip_chapter_positions() {
        let target = audiobook_toc_target(3_726_500);
        assert_eq!(target, "audiobook-position-ms:3726500");
        assert_eq!(audiobook_toc_position(&target), Some(3_726_500));
        assert_eq!(audiobook_toc_position("chapter.xhtml"), None);
        assert_eq!(audiobook_toc_position("audiobook-position-ms:not-a-number"), None);
    }

    #[test]
    fn authors_file_under_their_sort_name_not_their_display_name() {
        let id = agent_id_from_migration_seed(b"Kerstin Ekman");
        let author = AuthorSummary::try_new(id, "Kerstin Ekman".to_owned(), Some("Ekman, Kerstin".to_owned()), 9, Vec::new(), Vec::new()).unwrap();

        assert_eq!(author.name().as_str(), "Kerstin Ekman", "the display name is what the reader sees");
        assert_eq!(author.sort_name(), "Ekman, Kerstin");
    }

    #[test]
    fn a_missing_sort_name_falls_back_to_the_display_name() {
        let id = agent_id_from_migration_seed(b"Susanna Clarke");
        let blank = AuthorSummary::try_new(id, "Susanna Clarke".to_owned(), Some("   ".to_owned()), 2, Vec::new(), Vec::new()).unwrap();
        let absent = AuthorSummary::try_new(id, "Susanna Clarke".to_owned(), None, 2, Vec::new(), Vec::new()).unwrap();

        assert_eq!(blank.sort_name(), "Susanna Clarke");
        assert_eq!(absent.sort_name(), "Susanna Clarke");
    }

    #[test]
    fn sorting_by_name_orders_by_the_filing_form() {
        let author = |display: &str, sort: &str| AuthorSummary::try_new(agent_id_from_migration_seed(display.as_bytes()), display.to_owned(), Some(sort.to_owned()), 1, Vec::new(), Vec::new()).unwrap();
        let mut authors = vec![author("Kerstin Ekman", "Ekman, Kerstin"), author("Susanna Clarke", "Clarke, Susanna"), author("Peter Englund", "Englund, Peter")];

        AuthorSort::Name.sort(&mut authors);

        assert_eq!(authors.iter().map(AuthorSummary::sort_name).collect::<Vec<_>>(), vec!["Clarke, Susanna", "Ekman, Kerstin", "Englund, Peter"]);
    }

    #[test]
    fn author_summary_exposes_identity_independently_from_display_name() {
        let first_id = agent_id_from_migration_seed(b"first Alex Smith");
        let second_id = agent_id_from_migration_seed(b"second Alex Smith");
        let first = AuthorSummary::try_new(first_id, "Alex Smith".to_owned(), None, 1, Vec::new(), Vec::new()).unwrap();
        let second = AuthorSummary::try_new(second_id, "Alex Smith".to_owned(), None, 2, Vec::new(), Vec::new()).unwrap();

        assert_eq!(first.name(), second.name());
        assert_ne!(first.id(), second.id());
        assert_eq!(serde_json::to_value(first).unwrap()["id"], first_id.to_string());
        assert_eq!(serde_json::to_value(second).unwrap()["id"], second_id.to_string());
    }
}

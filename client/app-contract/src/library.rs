use library_model::AuthorId;
use library_model::{AuthorName, AuthorNameError};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::cmp::Reverse;
use sync_common::{ContentHash, DirId, LibraryId};

/// Limits hierarchical library browsing to one broad publication family.
/// `Book` intentionally means every supported reading format except PDF and
/// M4B, which have dedicated filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
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
            Self::Book => !extension.eq_ignore_ascii_case("pdf") && !extension.eq_ignore_ascii_case("m4b"),
        }
    }
}

#[cfg(test)]
mod file_type_filter_tests {
    use super::LibraryFileTypeFilter;

    #[test]
    fn broad_book_filter_excludes_dedicated_pdf_and_audiobook_formats() {
        assert!(LibraryFileTypeFilter::Book.matches_extension("epub"));
        assert!(LibraryFileTypeFilter::Book.matches_extension("cbz"));
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

    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn directory_id(&self) -> &DirId {
        &self.directory_id
    }
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
pub struct AuthorSummary {
    #[specta(type = String)]
    pub id: AuthorId,
    #[specta(type = String)]
    pub name: AuthorName,
    /// The filing form — "Ekman, Kerstin" — that the index sorts and groups by.
    /// Falls back to the display name for books that carry no `file-as`, so
    /// this is never empty and callers never have to decide again.
    pub sort_name: String,
    #[specta(type = specta_typescript::Number)]
    pub book_count: usize,
    /// Books displayed on the author's card, most recently read first. Each
    /// content hash identifies both its cover thumbnail and its detail page,
    /// capped at
    /// [`crate::frontend::AUTHOR_ROW_COVER_COUNT`].
    #[specta(type = Vec<String>)]
    pub cover_hashes: Vec<ContentHash>,
    /// Titles parallel to `cover_hashes`, used when a displayed book has no
    /// cover artwork. Both vectors are produced by the same ordered query.
    pub cover_titles: Vec<String>,
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
    /// Milliseconds since the Unix epoch when this book entered the library.
    /// Legacy rows without a timestamp use zero and sort after dated books.
    #[specta(type = specta_typescript::Number)]
    pub added_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AudiobookChapter {
    pub title: String,
    #[specta(type = specta_typescript::Number)]
    pub start_ms: u64,
    #[specta(type = specta_typescript::Number)]
    pub end_ms: u64,
}

/// One credited author on a book. Only `aut` contributor roles are projected
/// here; editors, translators and narrators remain contributor metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct BookDetailAuthor {
    #[specta(type = String)]
    pub id: AuthorId,
    pub name: String,
}

/// A reader target in the publication's own hierarchical navigation model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BookTocEntry {
    pub title: String,
    pub target: String,
    pub children: Vec<BookTocEntry>,
}

/// Complete initial payload for a book detail screen. Related books are kept
/// in the same response for native callers; browser workers may still resolve
/// the command independently from route rendering.
#[derive(Clone, Debug, PartialEq, Serialize, Type)]
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Type)]
pub struct LibrarySearchResult {
    pub title_matches: Vec<BookCardRow>,
    pub author_matches: Vec<BookCardRow>,
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
    use library_model::agent_id_from_migration_seed;
    use test_support::{fixture_dir_id, fixture_library_id};

    #[test]
    fn import_destination_round_trips_identities() {
        let destination = BookImportDestination::new(fixture_library_id("library-1"), fixture_dir_id("folder-1"));
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

use ui_text::text;

use crate::BookTocEntry;
use book_model::AuthorId;
use content_address::BookId;
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
    #[serde(rename = "content_hash")]
    book_id: BookId,
}

impl BookLocator {
    pub fn new(library_id: LibraryId, book_id: BookId) -> Self {
        Self { library_id, book_id }
    }

    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub fn book_id(&self) -> BookId {
        self.book_id
    }
}

/// An ongoing library operation reported to the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryBusyReason {
    Importing,
    DownloadingBooks,
    SavingBooks,
    MovingLibrary,
    RecoveringLibrary,
}

impl LibraryBusyReason {
    pub const fn blocks_ui(self) -> bool {
        matches!(self, Self::MovingLibrary | Self::RecoveringLibrary)
    }
}

/// Current library activity for UI presentation. The backend library state lock
/// provides admission independently of these snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookFileActivity {
    #[default]
    Idle,
    Downloading,
    Busy {
        reason: LibraryBusyReason,
        /// Includes imports hidden by the displayed reason.
        importing: bool,
        /// Combines restrictions from every active foreground operation.
        downloads_blocked: bool,
    },
}

impl BookFileActivity {
    pub fn from_reasons(active: &[LibraryBusyReason], downloading: bool) -> Self {
        let reason = active.iter().copied().find(|reason| reason.blocks_ui()).or_else(|| active.first().copied());
        match reason {
            Some(reason) => Self::Busy {
                reason,
                importing: active.contains(&LibraryBusyReason::Importing),
                downloads_blocked: active.iter().any(|reason| reason.blocks_ui() || matches!(reason, LibraryBusyReason::DownloadingBooks | LibraryBusyReason::SavingBooks)),
            },
            None if downloading => Self::Downloading,
            None => Self::Idle,
        }
    }

    /// One representative reason for display; action restrictions are aggregated.
    pub const fn reason(self) -> Option<LibraryBusyReason> {
        match self {
            Self::Busy { reason, .. } => Some(reason),
            _ => None,
        }
    }

    pub const fn importing(self) -> bool {
        matches!(self, Self::Busy { importing: true, .. })
    }

    /// Network transfers do not reserve the library's files. Publication
    /// revalidates its destination under the backend state lock.
    pub const fn blocks_file_changes(self) -> bool {
        self.busy_reason().is_some()
    }

    pub const fn busy_reason(self) -> Option<LibraryBusyReason> {
        match self {
            Self::Busy { reason, .. } if reason.blocks_ui() => Some(reason),
            _ => None,
        }
    }

    pub const fn blocks_download_requests(self) -> bool {
        matches!(self, Self::Busy { downloads_blocked: true, .. })
    }
}

/// A change relevant to a currently displayed library page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LibraryUpdate {
    Contents,
    Covers(Vec<BookId>),
    /// Independent cover work failed; pending work will be retried.
    ThumbnailFailed {
        #[serde(rename = "content_hash")]
        book_id: BookId,
        failure: Option<operation_failure::Failure>,
    },
    Download(crate::BookDownloadStatus),
    /// A queued user action failed after its initial command was accepted.
    DownloadFailed {
        #[serde(rename = "content_hash")]
        book_id: BookId,
        /// Why it failed, when known; the diagnostic stays in the backend log.
        failure: Option<operation_failure::Failure>,
    },
    CatalogImport {
        entry: CardKey,
        progress: crate::CatalogImportProgress,
    },
    Scanning(bool),
    /// Foreground restrictions or background download activity changed.
    BookFileActivity(BookFileActivity),
    /// Durable sync progress changed after a completed exchange.
    SyncStatusChanged,
    /// Annotation text, anchors or deletion state changed locally.
    Annotations,
    /// Reading position, progress, or chapter read status changed. Shelf cards
    /// may need refreshing, while annotations and bookmarks remain unchanged.
    ReadingProgress(BookId),
    TextSearchRemoved(crate::TextSearchRemoval),
}

/// Limits hierarchical library browsing to one broad book family.
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
            Self::All => text!(browse::all),
            Self::Book => text!(browse::books),
            Self::Pdf => text!(browse::pdf),
            Self::Audiobook => text!(browse::audiobooks),
        }
    }

    pub fn matches_extension(self, extension: &str) -> bool {
        match self {
            Self::All => true,
            Self::Pdf => extension.eq_ignore_ascii_case("pdf"),
            Self::Audiobook => extension.eq_ignore_ascii_case("m4b") || extension.eq_ignore_ascii_case("mp3folder"),
            Self::Book => matches!(extension.to_ascii_lowercase().as_str(), "epub" | "mobi" | "azw" | "azw3" | "fb2" | "fbz" | "fb2.zip" | "cbz"),
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
    Popularity,
    /// Home's existing shelf order, offered only by Home.
    DefaultOrder,
}

impl BrowseChipSort {
    pub const ALL: [Self; 3] = [Self::Alphabetical, Self::BookCount, Self::Popularity];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => text!(browse::alphabetical),
            Self::BookCount => text!(browse::book_count),
            Self::Popularity => text!(browse::popularity),
            Self::DefaultOrder => text!(browse::default_order),
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
    Popularity,
    /// Home's existing activity order, offered only by Home.
    DefaultOrder,
}

impl BrowseBookSort {
    pub const ALL: [Self; 3] = [Self::Alphabetical, Self::RecentlyRead, Self::Popularity];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Alphabetical => text!(browse::alphabetical),
            Self::RecentlyRead => text!(browse::recently_read),
            Self::Popularity => text!(browse::popularity),
            Self::DefaultOrder => text!(browse::default_order),
        }
    }
}

/// A browsable node: all books, a folder, a route in the subject hierarchy, an author,
/// a shelf of Home, a route in the public-domain catalog's subject hierarchy, an
/// or an author of the catalog's books.
/// `Author(None)` is the author index, every credited name, and
/// `CatalogAuthor(None)` the catalog's; `Home(None)` is Home itself, its
/// shelves. An author or a shelf holds books and nothing below them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BrowseLocation {
    Folder(DirId),
    Subject(i64),
    Author(Option<AuthorId>),
    Home(Option<HomeShelf>),
    Books,
    Notebook(crate::NotebookSort),
    Catalog(i64),
    CatalogAuthor(Option<AuthorId>),
    TextSearch(crate::NotebookSort),
}

impl BrowseLocation {
    /// The directory this location is, when it is a folder.
    pub fn folder(self) -> Option<DirId> {
        match self {
            Self::Folder(id) => Some(id),
            _ => None,
        }
    }
}

/// Home's shelves, in the order Home shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HomeShelf {
    RecentlyRead,
    Finished,
    RecentlyAdded,
}

impl HomeShelf {
    pub fn label(self) -> &'static str {
        match self {
            Self::RecentlyRead => text!(browse::recently_read),
            Self::Finished => text!(reader::finished),
            Self::RecentlyAdded => text!(browse::recently_added),
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::RecentlyRead => "recently-read",
            Self::Finished => "finished",
            Self::RecentlyAdded => "recently-added",
        }
    }
}

impl std::fmt::Display for BrowseLocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Books => write!(formatter, "books:"),
            Self::Notebook(sort) => write!(formatter, "notebook:{sort:?}"),
            Self::TextSearch(sort) => write!(formatter, "text-search:{sort:?}"),
            Self::Folder(id) => write!(formatter, "folder:{id}"),
            Self::Subject(route) => write!(formatter, "subject:{route}"),
            Self::Author(None) => write!(formatter, "author:"),
            Self::Author(Some(id)) => write!(formatter, "author:{id}"),
            Self::Home(None) => write!(formatter, "home:"),
            Self::Home(Some(shelf)) => write!(formatter, "home:{}", shelf.slug()),
            Self::Catalog(route) => write!(formatter, "catalog:{route}"),
            Self::CatalogAuthor(None) => write!(formatter, "catalog-author:"),
            Self::CatalogAuthor(Some(id)) => write!(formatter, "catalog-author:{id}"),
        }
    }
}

/// One directly browsable child returned by a library hierarchy query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseRow {
    /// Labels and paths are display data.
    pub id: BrowseLocation,
    pub name: String,
    /// Parent hierarchy for scoped search results; absent for direct children.
    pub path: Option<String>,
    pub book_count: usize,
    pub downloaded_book_count: usize,
}

/// One folder or subject in the displayed breadcrumb trail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowsePathSegment {
    pub id: BrowseLocation,
    pub name: String,
}

/// Direct contents for one hierarchy location.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowseContents {
    pub children: Vec<BrowseRow>,
    pub books: Vec<BookCardRow>,
}

/// One panel in the shared folder and subject browse presentation.
///
/// `parent` is the folder or subject represented by the panel; its immediate
/// children and directly assigned books are rendered inside it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrowseSection {
    pub parent: BrowseRow,
    pub children: Vec<BrowseRow>,
    pub books: Vec<BookCardRow>,
    /// Total directly displayed books when only the first page is included.
    /// Absent for complete sections, including older browse responses.
    #[serde(default)]
    pub book_count: Option<usize>,
}

/// A bounded range of a section's books, in the active browse order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrowseBookPage {
    pub books: Vec<BookCardRow>,
    pub offset: usize,
    pub total: usize,
}

/// Initial section covers include a small buffer for responsive layouts.
pub const BROWSE_SECTION_BOOK_LIMIT: usize = 8;
pub const MAX_BROWSE_SECTION_BOOK_LIMIT: usize = 128;

/// Independent windows for the outer section list and the direct book grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowsePageRequest {
    pub section_offset: usize,
    pub section_limit: usize,
    pub book_offset: usize,
    pub book_limit: usize,
}

impl Default for BrowsePageRequest {
    fn default() -> Self {
        Self { section_offset: 0, section_limit: 24, book_offset: 0, book_limit: 48 }
    }
}

/// Tree navigation stays complete; flat author headings use a bounded window.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowsePageState {
    pub section_ids: Vec<BrowseLocation>,
    pub section_offset: usize,
    pub book_offset: usize,
    pub book_total: usize,
    #[serde(default)]
    pub headings: Option<BrowseHeadingState>,
    /// Selection, ordering and state revision shared by every continuation.
    #[serde(default)]
    pub result_revision: Option<String>,
}

impl BrowsePageState {
    pub fn heading_count(&self) -> usize {
        self.headings.as_ref().map_or(self.section_ids.len(), |headings| headings.total)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseHeadingState {
    pub total: usize,
    /// Detect reordered or changed headings before merging another window.
    pub revision: String,
    pub cards: bool,
}

/// Number of visible library books tagged with a base language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryLanguageCount {
    pub language: String,
    pub book_count: u32,
}

/// Number of visible library books in one broad format category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryFormatCount {
    pub format: LibraryFileTypeFilter,
    pub book_count: u32,
}

/// Synthetic root of the subject browse hierarchy.
pub const ROOT_SUBJECT_ROUTE_ID: i64 = 0;

/// Typed input shared by folder and subject browsing implementations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBrowseQuery {
    pub location: BrowseLocation,
    pub search: String,
    pub file_types: Vec<LibraryFileTypeFilter>,
    pub chip_sort: BrowseChipSort,
    pub book_sort: BrowseBookSort,
    pub languages: Vec<String>,
    /// Drops books at or past [`FINISHED_PROGRESS_THRESHOLD`].
    ///
    /// Applied after subject compaction so hiding finished books does not
    /// change which branches or single-child chains are collapsed.
    /// Facet counts continue to describe the location including finished books.
    #[serde(default)]
    pub hide_finished: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Dir {
    pub id: DirId,
    pub name: String,
    pub book_count: usize,
    pub downloaded_book_count: usize,
}

/// Where a card's book is, which decides what it offers: a library book is
/// opened, read and downloaded; hosted catalog books are added to the
/// library, while external catalog entries open their source page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookSource {
    Library {
        #[serde(rename = "content_hash")]
        book_id: BookId,
    },
    /// A catalog book the library does not hold; catalog browsing leaves out
    /// the ones it does.
    Catalog {
        #[serde(rename = "content_hash")]
        book_id: BookId,
    },
    /// A catalog entry whose book stays on the source's website.
    /// Its opaque entry key identifies metadata, never a downloaded file.
    /// `downloadable` when the source offers the audiobook as an archive,
    /// which adding the entry downloads into the library.
    ExternalCatalog { entry: CardKey, downloadable: bool },
}

/// One book in a browse result, including catalog details when available.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BookCardRow {
    /// Which book this is, and so what the card offers.
    pub source: BookSource,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    pub author: String,
    pub progress: f32,
    pub downloaded: bool,
    pub download_requested: bool,
    /// Canonical broad format used by filters, facets, and cover chips.
    pub format_category: LibraryFileTypeFilter,
    pub audiobook_duration_ms: Option<u64>,
    /// Milliseconds since the Unix epoch when this book entered the library.
    /// Legacy rows without a timestamp use zero and sort after dated books.
    pub added_at: i64,
    /// Concrete placement represented by a folder-browse result. Cards from
    /// placement-neutral views such as Home and Subjects leave this absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_directory: Option<DirId>,
}

/// What identifies a card in a listing. A book with a file is keyed by its
/// content hash; an external catalog book is keyed by an id
/// derived from that entry. Never a file's identity: code that reaches the
/// book's stable identity uses `book_id()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CardKey(pub [u8; 32]);

impl From<BookId> for CardKey {
    fn from(id: BookId) -> Self {
        if id.is_local() {
            Self(*blake3::hash(id.as_bytes()).as_bytes())
        } else {
            Self(*id.identity_digest())
        }
    }
}

impl From<&BookId> for CardKey {
    fn from(id: &BookId) -> Self {
        Self::from(*id)
    }
}

impl std::fmt::Display for CardKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

impl BookCardRow {
    pub fn key(&self) -> CardKey {
        match self.source {
            BookSource::Library { book_id } | BookSource::Catalog { book_id, .. } => CardKey::from(book_id),
            BookSource::ExternalCatalog { entry, .. } => entry,
        }
    }

    /// The file of a book in the library; none for catalog books,
    /// which are added rather than read or downloaded.
    pub fn library_file(&self) -> Option<BookId> {
        match self.source {
            BookSource::Library { book_id } => Some(book_id),
            BookSource::Catalog { .. } | BookSource::ExternalCatalog { .. } => None,
        }
    }

    /// The stable book identity, when known. An external entry gains one on import.
    pub fn book_id(&self) -> Option<BookId> {
        match self.source {
            BookSource::Library { book_id } | BookSource::Catalog { book_id, .. } => Some(book_id),
            BookSource::ExternalCatalog { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashedBook {
    #[serde(rename = "content_hash")]
    pub book_id: BookId,
    pub title: String,
    pub author: String,
    pub format: String,
    pub deleted_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashedFolder {
    pub id: DirId,
    pub name: String,
    /// The folder it was in; none when that was the library's root.
    pub parent_name: Option<String>,
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

/// One credited author on a book. Only `aut` contributor roles are projected
/// here; editors, translators and narrators remain contributor metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookDetailAuthor {
    pub id: AuthorId,
    pub name: String,
}

/// A narrator credit the detail page states beside the authors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookDetailCredit {
    pub id: AuthorId,
    pub name: String,
    /// The MARC relator code, as stored: "nrt".
    pub role: String,
}

/// Complete initial payload for a book detail screen. Related books are kept
/// in the same response for native callers; browser workers may still resolve
/// the command independently from route rendering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BookDetail {
    pub book: BookCardRow,
    /// The source page for a catalog entry read outside the library.
    #[serde(default)]
    pub external_url: Option<String>,
    /// Navigation entry count for an audiobook, when its contents are known.
    #[serde(default)]
    pub audiobook_chapter_count: Option<usize>,
    /// The publisher's blurb, as stored (HTML). Only the detail page and the
    /// book overlay show it, so browse queries never read it.
    pub description: String,
    /// Where the book sits in the subject tree Browse shows: the path to each
    /// subject that lists the book itself, root first, every crumb a subject
    /// page. Curated taxonomy only — never the file's own subject headings.
    /// Empty for a book with no curated subject.
    #[serde(default)]
    pub subjects: Vec<Vec<BrowsePathSegment>>,
    /// Publication year, when known: the file's own, else the metadata server's.
    #[serde(default)]
    pub year: Option<i32>,
    /// One live placement that can act as the source for copy/move actions.
    /// A detail screen is not itself scoped to a folder.
    pub source_directory_id: Option<String>,
    /// The folder trail to that placement, root first, for a page reached
    /// without browsing to it — from the audiobook dock, a search result, or a
    /// link. The book itself is the trail's last crumb and is not repeated here.
    pub path: Vec<BrowsePathSegment>,
    pub authors: Vec<BookDetailAuthor>,
    /// Narrators, in the order the book credits them.
    pub credits: Vec<BookDetailCredit>,
    /// Where the reader is, named: "Against the Tide?" rather than a percentage
    /// nobody can picture. Absent for a book nobody has opened, and for one
    /// whose navigation is not stored.
    pub current_entry: Option<String>,
    /// The same place, as the entry in `toc` rather than as its wording.
    ///
    /// Both, because they answer different questions: a line of prose about the
    /// reader's position wants the title, and a list marking one of its own rows
    /// wants to know *which row* — and two entries can share a title while a
    /// target identifies one of them. Returning only the title made a contents
    /// list that could never mark anything.
    pub current_target: Option<String>,
    pub toc: Vec<BookTocEntry>,
    #[serde(default)]
    pub chapter_read_status: book_model::ChapterReadStatus,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_legacy_books_with_the_same_initial_digest_have_distinct_card_keys() {
        let digest = content_address::ContentHash::from_bytes([9; 32]);
        assert_ne!(CardKey::from(BookId::from(digest)), CardKey::from(BookId::for_local_content(&[4; 16], digest)));
    }

    #[test]
    fn folder_book_count_sort_uses_name_as_a_deterministic_tie_breaker() {
        let mut directories = vec![Dir::new(uuid::Uuid::new_v4(), "Zelda".to_owned(), 2, 0), Dir::new(uuid::Uuid::new_v4(), "Alice".to_owned(), 7, 0), Dir::new(uuid::Uuid::new_v4(), "Bob".to_owned(), 2, 0)];

        FolderSort::BookCount.sort(&mut directories);
        assert_eq!(directories.iter().map(|directory| (directory.name().as_str(), directory.book_count())).collect::<Vec<_>>(), vec![("Alice", 7), ("Bob", 2), ("Zelda", 2)]);
    }
}

//! What a browse page lists — the folders or subjects under one location, and
//! the books with them — and how it is fetched from the library.

use library_backend::{LibraryCommand, ReadyCommand, SessionCommand};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use gpui::{App, Entity, SharedString};
use library_model::{BrowseLocation, BrowseRow, LibraryBrowseQuery};
use sync_common::{ContentHash, DirId, ROOT_DIR_ID};
use ui_components as components;

use super::controls::BrowseControls;
use crate::library::services::LibraryContext;
use crate::library::widgets::{BookCardView, OpenBookDetail, PlaceBook, RemoveBook};

pub(crate) type BrowseFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

/// Which tree a browse page walks. Folders are real directories, with the
/// commands that go with them; subjects are queries over the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseKind {
    Folder,
    Subject,
}

impl BrowseKind {
    pub(super) fn root_location(self) -> BrowseLocation {
        match self {
            Self::Folder => BrowseLocation::Folder(ROOT_DIR_ID),
            Self::Subject => BrowseLocation::Subject(library_model::ROOT_SUBJECT_ROUTE_ID),
        }
    }

    pub(super) fn root_label(self) -> SharedString {
        match self {
            Self::Folder => "Folders".into(),
            Self::Subject => "Subjects".into(),
        }
    }

    /// Prefixes the element ids the page hands out.
    pub(super) fn list_prefix(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Subject => "subject",
        }
    }

    pub(super) fn child_icon(self) -> components::NavigationIcon {
        match self {
            Self::Folder => components::NavigationIcon::Folder,
            Self::Subject => components::NavigationIcon::Tags,
        }
    }
}

pub(super) fn request(library: &library_backend::LibraryClient, query: LibraryBrowseQuery) -> BrowseFuture<Result<BrowseResponse, String>> {
    let library = library.clone();
    let searched = !query.search.is_empty();
    Box::pin(async move { Ok(BrowseResponse { searched, data: library.browse(query).await? }) })
}

pub(super) fn download(library: &library_backend::LibraryClient, query: LibraryBrowseQuery) -> BrowseFuture<Result<(), String>> {
    let library = library.clone();
    Box::pin(async move { library.request::<()>(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::DownloadBrowse { query }))).await })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseCrumb {
    pub(crate) location: BrowseLocation,
    pub(crate) label: String,
}

/// A browsable panel with the immediate children and directly assigned books
/// of its parent location.
pub(crate) struct BrowseSection {
    pub(crate) parent: BrowseRow,
    pub(crate) children: Vec<BrowseRow>,
    pub(crate) books: Vec<Entity<BookCardView>>,
}

/// Resolved display items carry their placement and card, not layout indices.
#[derive(Clone)]
pub(super) enum BrowseItem {
    Child { row: BrowseRow, parent: Option<DirId> },
    Book(Entity<BookCardView>),
}

impl BrowseItem {
    pub(super) fn child(&self) -> Option<&BrowseRow> {
        match self {
            Self::Child { row, .. } => Some(row),
            Self::Book(_) => None,
        }
    }
    pub(super) fn id(&self, cx: &App) -> SharedString {
        match self {
            Self::Child { row, .. } => chip_id(row.id),
            Self::Book(book) => book_id(book.read(cx).content_hash()),
        }
    }
}

pub(super) fn chip_id(id: BrowseLocation) -> SharedString {
    format!("chip:{id}").into()
}

pub(super) fn book_id(hash: ContentHash) -> SharedString {
    format!("book:{hash}").into()
}
pub(super) type BrowseItems = Rc<HashMap<SharedString, BrowseItem>>;

pub(crate) struct BrowseResponse {
    pub(super) searched: bool,
    pub(super) data: app::LibraryBrowseData,
}

/// Route and displayed ancestry travel together, including missing routes
/// whose backend trail is empty. Requests cannot install one without the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BrowseDestination {
    pub(super) location: BrowseLocation,
    pub(super) path: Vec<BrowseCrumb>,
}

impl BrowseDestination {
    pub(super) fn from_path(kind: BrowseKind, path: Vec<BrowseCrumb>) -> Self {
        let location = path.last().map_or_else(|| kind.root_location(), |crumb| crumb.location);
        Self { location, path }
    }
}

/// Whether a chip still has books to bring onto this device.
pub(super) fn can_download(child: &BrowseRow) -> bool {
    child.downloaded_book_count < child.book_count
}

/// Whether a chip carries the download mark: only when it is partly on this
/// device. Which state is the common one differs from device to device, and on
/// one that holds almost nothing a mark on every remote chip would be on every
/// chip. A partly downloaded one is the exception either way — something begun
/// and not finished. A wholly remote one is downloaded from its menu instead.
pub(super) fn shows_download_mark(child: &BrowseRow) -> bool {
    child.downloaded_book_count > 0 && can_download(child)
}

/// The listing a page has installed: one location's children, sections and
/// books, and the controls its facets allow. Book cards outlive a reload, so a
/// book still in the listing keeps its entity and its decoded cover.
pub(super) struct BrowseListing {
    ui: LibraryContext,
    kind: BrowseKind,
    controls: BrowseControls,
    children: Rc<Vec<BrowseRow>>,
    sections: Vec<BrowseSection>,
    books: Rc<Vec<Entity<BookCardView>>>,
    items: BrowseItems,
    open_detail: OpenBookDetail,
    place_book: Option<PlaceBook>,
    remove_book: Option<RemoveBook>,
}

impl BrowseListing {
    pub(super) fn new(ui: LibraryContext, kind: BrowseKind, open_detail: OpenBookDetail, place_book: Option<PlaceBook>, remove_book: Option<RemoveBook>) -> Self {
        Self { ui, kind, controls: Self::controls_for(kind, BrowseControls::library()), children: Rc::new(Vec::new()), sections: Vec::new(), books: Rc::new(Vec::new()), items: Rc::new(HashMap::new()), open_detail, place_book, remove_book }
    }

    fn controls_for(kind: BrowseKind, mut controls: BrowseControls) -> BrowseControls {
        controls.supports_direct_child_books = kind == BrowseKind::Subject;
        controls
    }

    /// Installs a response, and hands back the trail the library says leads to
    /// it, where it knows one.
    pub(super) fn install(&mut self, response: BrowseResponse, cx: &mut App) -> Vec<BrowseCrumb> {
        let searched = response.searched;
        let app::LibraryBrowseData { contents, sections, path, language_counts, format_counts } = response.data;
        let children = if searched { contents.children } else { sections.iter().map(|section| section.parent.clone()).collect() };
        let books = contents.books;
        self.controls = Self::controls_for(self.kind, BrowseControls::library_with_facets(&language_counts, &format_counts));
        self.children = Rc::new(children);
        // A card outlives a reload wherever its book is still listed — in the
        // grid or in a section card — so a refresh keeps every decoded cover
        // rather than drawing each book blank until its cover loads again. A
        // library change reloads the listing, and during a scan that is often.
        let mut previous: HashMap<sync_common::ContentHash, Vec<Entity<BookCardView>>> = HashMap::new();
        for card in self.books.iter().chain(self.sections.iter().flat_map(|section| section.books.iter())) {
            previous.entry(card.read(cx).content_hash()).or_default().push(card.clone());
        }
        let mut card_for = |book: library_model::BookCardRow, cx: &mut App| match previous.get_mut(&book.content_hash).and_then(Vec::pop) {
            Some(card) => {
                let source_directory = book.source_directory;
                card.update(cx, |card, _| card.replace_row(book, source_directory));
                card
            }
            None => {
                let source_directory = book.source_directory;
                BookCardView::create_in_grid(book, self.ui.clone(), source_directory, self.open_detail.clone(), self.place_book.clone(), self.remove_book.clone(), cx)
            }
        };
        let sections = sections.into_iter().map(|section| BrowseSection { parent: section.parent, children: section.children, books: section.books.into_iter().map(|book| card_for(book, cx)).collect() }).collect();
        let books = Rc::new(books.into_iter().map(|book| card_for(book, cx)).collect());
        self.sections = sections;
        self.books = books;
        let direct = self.children.iter().cloned().map(|row| BrowseItem::Child { row, parent: None });
        let nested = self.sections.iter().flat_map(|section| section.children.iter().cloned().map(move |row| BrowseItem::Child { row, parent: if let BrowseLocation::Folder(id) = section.parent.id { Some(id) } else { None } }));
        let books = self.books.iter().cloned().map(BrowseItem::Book);
        self.items = Rc::new(direct.chain(nested).chain(books).map(|item| (item.id(cx), item)).collect());
        path.into_iter().map(|segment| BrowseCrumb { location: segment.id, label: segment.name }).collect()
    }

    pub(super) fn items(&self) -> BrowseItems {
        self.items.clone()
    }
    pub(super) fn item(&self, id: &SharedString) -> Option<&BrowseItem> {
        self.items.get(id)
    }
    pub(super) fn child(&self, id: BrowseLocation) -> Option<&BrowseRow> {
        self.items.get(&chip_id(id))?.child()
    }

    pub(super) fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }

    pub(super) fn sections(&self) -> &[BrowseSection] {
        &self.sections
    }

    pub(super) fn books(&self) -> Rc<Vec<Entity<BookCardView>>> {
        self.books.clone()
    }

    pub(super) fn controls(&self) -> &BrowseControls {
        &self.controls
    }
}

//! What a browse page lists — the folders or subjects under one location, and
//! the books with them.

use library_backend::LibraryBrowseData;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, Entity, SharedString};
use library_model::{BrowseLocation, BrowseRow, CardKey};
use sync_common::{DirId, ROOT_DIR_ID};
use ui_components as components;

use super::controls::BrowseControls;
use crate::library::services::LibraryContext;
use crate::library::widgets::{BookCardEvents, BookCardView};

/// What a browse page lists. Folders are real directories, with the
/// commands that go with them; subjects and authors are queries over the
/// library, Books is the flat library, and Home is its shelves of recent activity.
/// Catalog is the public-domain catalog's subject tree: books not in the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseKind {
    Home,
    Books,
    Folder,
    Subject,
    Author,
    Catalog,
}

impl BrowseKind {
    pub(super) fn root_location(self) -> BrowseLocation {
        match self {
            Self::Home => BrowseLocation::Home(None),
            Self::Books => BrowseLocation::Books,
            Self::Folder => BrowseLocation::Folder(ROOT_DIR_ID),
            Self::Subject => BrowseLocation::Subject(library_model::ROOT_SUBJECT_ROUTE_ID),
            Self::Author => BrowseLocation::Author(None),
            Self::Catalog => BrowseLocation::Catalog(library_model::ROOT_SUBJECT_ROUTE_ID),
        }
    }

    pub(super) fn root_label(self) -> SharedString {
        match self {
            Self::Home => "Home".into(),
            Self::Books => "All books".into(),
            Self::Folder => "Folders".into(),
            Self::Subject => "Subjects".into(),
            Self::Author => "Authors".into(),
            Self::Catalog => "Catalog".into(),
        }
    }

    /// Prefixes the element ids the page hands out.
    pub(super) fn list_prefix(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Books => "books",
            Self::Folder => "folder",
            Self::Subject => "subject",
            Self::Author => "author",
            Self::Catalog => "catalog",
        }
    }

    /// The graph a kind can be shown as, and the layout it opens in: only the
    /// trees have one. Books, Home's shelves and the author index are flat.
    pub(super) fn graph_layout(self) -> Option<components::GraphLayoutKind> {
        match self {
            Self::Folder => Some(components::GraphLayoutKind::Columns),
            Self::Subject | Self::Catalog => Some(components::GraphLayoutKind::AdaptiveRadial),
            Self::Home | Self::Author | Self::Books => None,
        }
    }

    pub(super) fn child_icon(self) -> components::NavigationIcon {
        match self {
            Self::Home => components::NavigationIcon::House,
            Self::Books => components::NavigationIcon::BookOpen,
            Self::Folder => components::NavigationIcon::Folder,
            Self::Subject => components::NavigationIcon::Tags,
            Self::Author => components::NavigationIcon::User,
            Self::Catalog => components::NavigationIcon::Tags,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseCrumb {
    pub(crate) location: BrowseLocation,
    pub(crate) label: String,
}

/// A browsable panel with the immediate children and directly assigned books
/// of its parent location.
#[derive(Clone)]
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
            Self::Book(book) => book_id(book.read(cx).key()),
        }
    }
}

pub(super) fn chip_id(id: BrowseLocation) -> SharedString {
    format!("chip:{id}").into()
}

pub(super) fn book_id(key: CardKey) -> SharedString {
    format!("book:{key}").into()
}
pub(super) type BrowseItems = Rc<HashMap<SharedString, BrowseItem>>;

/// The listed items among `ids`, in their order.
pub(super) fn resolve_items(ids: &[SharedString], items: &HashMap<SharedString, BrowseItem>) -> Vec<BrowseItem> {
    ids.iter().filter_map(|id| items.get(id).cloned()).collect()
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

/// Whether a chip still has books to bring onto this device. The browser
/// streams books it does not hold instead of downloading them, and catalog
/// and feed books are added to the library, not downloaded.
pub(super) fn can_download(child: &BrowseRow) -> bool {
    cfg!(not(target_arch = "wasm32")) && !matches!(child.id, BrowseLocation::Catalog(_) | BrowseLocation::Opds { .. }) && child.downloaded_book_count < child.book_count
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
    pub(super) controls: BrowseControls,
    pub(super) children: Rc<Vec<BrowseRow>>,
    pub(super) sections: Vec<BrowseSection>,
    pub(super) books: Rc<Vec<Entity<BookCardView>>>,
    /// The children of the locations above this one, for the graph.
    pub(super) ancestors: Vec<(BrowseLocation, Vec<BrowseRow>)>,
    /// Every child and book, by the id its paginator child carries.
    pub(super) items: BrowseItems,
    /// A feed page's next page, appended to this listing when the grid reaches its end.
    pub(super) next: Option<BrowseLocation>,
    /// The feed refused this device; the page asks to sign in.
    pub(super) sign_in: bool,
    /// How the book cards reach the page.
    book_events: BookCardEvents,
    folder_commands: bool,
}

impl BrowseListing {
    /// `folder_commands` offers copying, moving and removing books on their
    /// cards, for a listing of real folders.
    pub(super) fn new(ui: LibraryContext, kind: BrowseKind, book_events: BookCardEvents, folder_commands: bool) -> Self {
        Self {
            ui,
            kind,
            controls: Self::controls_for(kind, BrowseControls::library()),
            children: Rc::new(Vec::new()),
            sections: Vec::new(),
            books: Rc::new(Vec::new()),
            ancestors: Vec::new(),
            items: Rc::new(HashMap::new()),
            next: None,
            sign_in: false,
            book_events,
            folder_commands,
        }
    }

    fn controls_for(kind: BrowseKind, mut controls: BrowseControls) -> BrowseControls {
        controls.supports_direct_child_books = matches!(kind, BrowseKind::Subject | BrowseKind::Catalog);
        if kind == BrowseKind::Books {
            controls.collection_sorts.clear();
        }
        // Catalog books have no reading history to sort by.
        if kind == BrowseKind::Catalog {
            controls.book_sorts.clear();
        }
        // Home's shelves and their books keep the order that makes them what
        // they are: most recent first.
        if kind == BrowseKind::Home {
            controls.collection_sorts.clear();
            controls.book_sorts.clear();
        }
        controls
    }

    /// Installs a response, and hands back the trail the library says leads to
    /// it, where it knows one.
    /// `searched` says whether `data` answers a search.
    pub(super) fn install(&mut self, data: LibraryBrowseData, searched: bool, cx: &mut App) -> Vec<BrowseCrumb> {
        let LibraryBrowseData { contents, sections, path, ancestors, language_counts, format_counts, feed } = data;
        self.ancestors = ancestors;
        // A search lists its matches flat; the sections are the unsearched
        // listing's, and the page lays its children out as cards only if any.
        // A feed page has no sections: its children are its navigation links.
        let sections = if searched { Vec::new() } else { sections };
        let children = if searched || feed.is_some() { contents.children } else { sections.iter().map(|section| section.parent.clone()).collect() };
        let books = contents.books;
        (self.controls, self.next, self.sign_in) = match feed {
            Some(feed) => (BrowseControls::feed(feed.facets), feed.next, feed.sign_in),
            None => (Self::controls_for(self.kind, BrowseControls::library_with_facets(&language_counts, &format_counts)), None, false),
        };
        self.children = Rc::new(children);
        // A card outlives a reload wherever its book is still listed — in the
        // grid or in a section card — so a refresh keeps every decoded cover
        // rather than drawing each book blank until its cover loads again. A
        // library change reloads the listing, and during a scan that is often.
        let mut previous: HashMap<CardKey, Vec<Entity<BookCardView>>> = HashMap::new();
        for card in self.books.iter().chain(self.sections.iter().flat_map(|section| section.books.iter())) {
            previous.entry(card.read(cx).key()).or_default().push(card.clone());
        }
        let mut card_for = |book: library_model::BookCardRow, cx: &mut App| match previous.get_mut(&book.key()).and_then(Vec::pop) {
            Some(card) => {
                card.update(cx, |card, _| card.replace_row(book));
                card
            }
            None => BookCardView::create_in_grid(book, self.ui.clone(), self.book_events.clone(), self.folder_commands, self.kind == BrowseKind::Books, cx),
        };
        let sections = sections.into_iter().map(|section| BrowseSection { parent: section.parent, children: section.children, books: section.books.into_iter().map(|book| card_for(book, cx)).collect() }).collect();
        let books = Rc::new(books.into_iter().map(|book| card_for(book, cx)).collect());
        self.sections = sections;
        self.books = books;
        let direct = self.children.iter().cloned().map(|row| BrowseItem::Child { row, parent: None });
        let nested = self.sections.iter().flat_map(|section| section.children.iter().cloned().map(move |row| BrowseItem::Child { row, parent: section.parent.id.folder() }));
        let books = self.books.iter().cloned().map(BrowseItem::Book);
        self.items = Rc::new(direct.chain(nested).chain(books).map(|item| (item.id(cx), item)).collect());
        path.into_iter().map(|segment| BrowseCrumb { location: segment.id, label: segment.name }).collect()
    }

    /// Adds a feed's next page after what is listed. A book or chip the
    /// listing already has is not listed twice.
    pub(super) fn append(&mut self, data: LibraryBrowseData, cx: &mut App) {
        self.next = data.feed.and_then(|feed| feed.next);
        let mut items = (*self.items).clone();
        let mut children = (*self.children).clone();
        for row in data.contents.children {
            let item = BrowseItem::Child { row: row.clone(), parent: None };
            if items.insert(item.id(cx), item).is_none() {
                children.push(row);
            }
        }
        let mut books = (*self.books).clone();
        for book in data.contents.books {
            let id = book_id(book.key());
            if items.contains_key(&id) {
                continue;
            }
            let card = BookCardView::create_in_grid(book, self.ui.clone(), self.book_events.clone(), self.folder_commands, false, cx);
            items.insert(id, BrowseItem::Book(card.clone()));
            books.push(card);
        }
        self.children = Rc::new(children);
        self.books = Rc::new(books);
        self.items = Rc::new(items);
    }

    pub(super) fn item(&self, id: &SharedString) -> Option<&BrowseItem> {
        self.items.get(id)
    }

    pub(super) fn child(&self, id: BrowseLocation) -> Option<&BrowseRow> {
        self.items.get(&chip_id(id))?.child()
    }
}

//! Synchronous library data adapter for the shared browse page.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{AnyElement, App, AppContext, Context, Entity, IntoElement};
use library_model::{BrowseBookSort, BrowseContents, BrowseRow, LibraryBrowseQuery, LibraryFileTypeFilter};
use subject_projection::ROOT_SUBJECT_PATH;
use sync_common::ROOT_DIR_ID;

use super::adapter::{BrowseAdapter, BrowseCrumb, BrowseQuery, BrowseRequest};
use super::controls::{BrowseBookOrder, BrowseControls, BrowseFormatFilter};
use super::detail::BookDetailPage;
use super::folder_actions::{FolderActions, FolderTargets};
use super::page::{BrowsePage, BrowsePageConfig};
use crate::library::services::LibraryContext;
use crate::library::widgets::{BookCardView, BookPlacement, OpenBookDetail, PlaceBook, RemoveBook};

/// One book, as a place. A detail page is not a folder, but it *is* somewhere
/// the reader has gone: giving it a location means the breadcrumb ends on the
/// title, Escape leaves it the way it leaves a folder, and Back is the same
/// `open_path` that goes up the tree. None of that had to be written.
const BOOK_LOCATION_PREFIX: &str = "book:";

pub(super) fn book_location(content_hash: sync_common::ContentHash) -> String {
    format!("{BOOK_LOCATION_PREFIX}{content_hash}")
}

fn book_at_location(location: &str) -> Option<sync_common::ContentHash> {
    location.strip_prefix(BOOK_LOCATION_PREFIX)?.parse().ok()
}

#[derive(Clone, Copy)]
enum LibraryBrowseKind {
    Folder,
    Subject,
}

pub(crate) struct LibraryBrowseAdapter {
    ui: LibraryContext,
    kind: LibraryBrowseKind,
    controls: BrowseControls,
    children: Rc<Vec<BrowseRow>>,
    chip_groups: Vec<library_model::BrowseChipGroup>,
    books: Rc<Vec<Entity<BookCardView>>>,
    open_detail: OpenBookDetail,
    place_book: Option<PlaceBook>,
    remove_book: Option<RemoveBook>,
    /// What the card handed over on its way out, waiting for the navigation it
    /// started to arrive. It cannot be fetched from the card during `install`:
    /// that runs inside the card's own update, and reading a leased entity
    /// panics.
    pending_detail: Option<(library_model::BookCardRow, Option<crate::library::widgets::LoadedCover>)>,
    /// The book whose page is open. The grid's cards are deliberately left
    /// installed underneath it, so going back reuses their entities — and with
    /// them their decoded covers — instead of building the folder again from
    /// nothing.
    detail: Option<Entity<BookDetailPage>>,
    detail_visible: bool,
}

pub(crate) enum LibraryBrowseResponse {
    Book(sync_common::ContentHash),
    Contents { contents: BrowseContents, chip_groups: Vec<library_model::BrowseChipGroup>, path: Vec<BrowseCrumb>, language_counts: Vec<library_model::LibraryLanguageCount>, format_counts: Vec<library_model::LibraryFormatCount> },
}

impl LibraryBrowseAdapter {
    fn new(ui: LibraryContext, kind: LibraryBrowseKind, open_detail: OpenBookDetail, place_book: Option<PlaceBook>, remove_book: Option<RemoveBook>) -> Self {
        let mut controls = BrowseControls::library();
        controls.supports_direct_child_books = matches!(kind, LibraryBrowseKind::Subject);
        Self { ui, kind, controls, children: Rc::new(Vec::new()), chip_groups: Vec::new(), books: Rc::new(Vec::new()), open_detail, place_book, remove_book, pending_detail: None, detail: None, detail_visible: false }
    }

    pub(super) fn prepare_detail(&mut self, book: library_model::BookCardRow, cover: Option<crate::library::widgets::LoadedCover>) {
        self.pending_detail = Some((book, cover));
    }

    fn has_cached_book_detail(&self, content_hash: sync_common::ContentHash, cx: &gpui::App) -> bool {
        self.detail.as_ref().is_some_and(|detail| detail.read(cx).content_hash() == content_hash)
    }

    fn backend_query(query: &BrowseQuery) -> LibraryBrowseQuery {
        let file_types = query
            .formats
            .iter()
            .filter_map(|format| match format {
                BrowseFormatFilter::Library(format) => Some(*format),
                BrowseFormatFilter::Any => None,
            })
            .collect::<Vec<LibraryFileTypeFilter>>();
        let book_sort = match query.book_sort {
            Some(BrowseBookOrder::Library(sort)) => sort,
            None => BrowseBookSort::default(),
        };
        LibraryBrowseQuery {
            location: query.location.clone(),
            search: query.search.clone(),
            file_types,
            chip_sort: query.chip_sort.unwrap_or_default(),
            book_sort,
            languages: query.languages.iter().map(|language| (*language).to_owned()).collect(),
            hide_finished: query.hide_finished,
            include_direct_child_books: query.include_direct_child_books,
        }
    }
}

impl BrowseAdapter for LibraryBrowseAdapter {
    fn include_group_in_path(&self) -> bool {
        matches!(self.kind, LibraryBrowseKind::Folder)
    }
    fn child_count(child: &BrowseRow) -> Option<usize> {
        Some(child.book_count)
    }
    fn can_download_child(child: &BrowseRow) -> bool {
        child.downloaded_book_count < child.book_count
    }

    fn download_child(&self, query: &BrowseQuery) -> Option<super::adapter::BrowseFuture<Result<(), String>>> {
        let library = self.ui.backend().clone();
        let query = Self::backend_query(query);
        let kind = self.kind;
        Some(Box::pin(async move {
            match kind {
                LibraryBrowseKind::Folder => library.download_folder(query).await,
                LibraryBrowseKind::Subject => library.download_subject(query).await,
            }
        }))
    }
    fn library_context(&self) -> Option<&LibraryContext> {
        Some(&self.ui)
    }
    type Response = LibraryBrowseResponse;
    type Book = BookCardView;

    fn child_icon(&self) -> ui_components::NavigationIcon {
        match self.kind {
            LibraryBrowseKind::Folder => ui_components::NavigationIcon::Folder,
            LibraryBrowseKind::Subject => ui_components::NavigationIcon::Tags,
        }
    }

    fn refresh_on_library_changes(&self) -> bool {
        true
    }

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response> {
        let query = Self::backend_query(query);
        // A book needs no query to arrive at: the card that was clicked is
        // still installed and holds everything the page opens with.
        if let Some(content_hash) = book_at_location(&query.location) {
            return BrowseRequest::Ready(LibraryBrowseResponse::Book(content_hash));
        }
        // These maintenance lanes run additional full-library and enrichment
        // work. In a browser they execute on the same single-threaded worker as
        // navigation, so awaiting them here can prevent every browse request
        // from completing. Keep them off the web page's critical load path.
        let library = self.ui.backend().clone();
        let kind = self.kind;
        let request = async move {
            match kind {
                LibraryBrowseKind::Folder => library.folder_contents(query).await,
                LibraryBrowseKind::Subject => library.subject_contents(query).await,
            }
        };
        BrowseRequest::Async(Box::pin(async move {
            let response: bokheim_backend::LibraryBrowseData = request.await?;
            let contents = response.contents;
            let chip_groups = response.chip_groups;
            let language_counts = response.language_counts;
            let format_counts = response.format_counts;
            let path = response.path.into_iter().map(|segment| BrowseCrumb { location: segment.id, label: segment.name }).collect();
            Ok(LibraryBrowseResponse::Contents { contents, chip_groups, path, language_counts, format_counts })
        }))
    }

    fn install(&mut self, response: Self::Response, cx: &mut App) -> Option<Vec<BrowseCrumb>> {
        let (contents, chip_groups, path, language_counts, format_counts) = match response {
            LibraryBrowseResponse::Contents { contents, chip_groups, path, language_counts, format_counts } => (contents, chip_groups, path, language_counts, format_counts),
            LibraryBrowseResponse::Book(content_hash) => {
                // Deliberately leaves `children` and `books` alone: the grid is
                // where the reader came from and where they are going back to.
                match self.pending_detail.take().filter(|(book, _)| book.content_hash == content_hash) {
                    Some((book, cover)) => self.detail = Some(BookDetailPage::create(book, cover, self.ui.clone(), cx)),
                    // A reload of this same location — a scan committed while
                    // the page was open — arrives with nothing handed over. The
                    // page already on screen is for this book, so it stays.
                    None if self.detail.as_ref().is_some_and(|detail| detail.read(cx).content_hash() == content_hash) => {}
                    None => {
                        log::warn!("nothing to open the page for book {content_hash} with");
                        self.detail = None;
                    }
                }
                self.detail_visible = self.detail.is_some();
                return None;
            }
        };
        self.detail_visible = false;
        self.controls = BrowseControls::library_with_facets(&language_counts, &format_counts);
        self.controls.supports_direct_child_books = matches!(self.kind, LibraryBrowseKind::Subject);
        self.children = Rc::new(contents.children);
        self.chip_groups = chip_groups;
        let mut existing = self.books.iter().map(|book| (book.read(cx).content_hash(), book.clone())).collect::<HashMap<_, _>>();
        self.books = Rc::new(
            contents
                .books
                .into_iter()
                .map(|book| {
                    let source_directory = book.source_directory;
                    if let Some(existing) = existing.remove(&book.content_hash) {
                        existing.update(cx, |existing, _| existing.replace_row(book, source_directory));
                        existing
                    } else {
                        BookCardView::create_in_grid(book, self.ui.clone(), source_directory, self.open_detail.clone(), self.place_book.clone(), self.remove_book.clone(), cx)
                    }
                })
                .collect(),
        );
        matches!(self.kind, LibraryBrowseKind::Folder).then_some(path)
    }

    fn chip_groups(&self) -> &[library_model::BrowseChipGroup] {
        &self.chip_groups
    }

    fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }

    fn books(&self) -> Rc<Vec<Entity<Self::Book>>> {
        self.books.clone()
    }

    fn controls(&self) -> &BrowseControls {
        &self.controls
    }

    fn custom_body(&self) -> Option<AnyElement> {
        self.detail_visible.then(|| self.detail.clone()).flatten().map(IntoElement::into_any_element)
    }

    fn prepare_book(book: &Entity<Self::Book>, window: &mut gpui::Window, cx: &mut App) {
        let resolution = BookCardView::thumbnail_resolution(window);
        book.update(cx, |book, cx| book.prepare(resolution, cx));
    }

    fn activate_book(book: &Entity<Self::Book>, window: &mut gpui::Window, cx: &mut gpui::App) {
        BookCardView::activate(book, window, cx);
    }

    fn set_book_focused(book: &Entity<Self::Book>, focused: bool, cx: &mut App) {
        book.update(cx, |book, book_cx| book.set_focused(focused, book_cx));
    }

    fn set_book_selected(book: &Entity<Self::Book>, selected: bool, cx: &mut App) {
        book.update(cx, |book, book_cx| book.set_selected(selected, book_cx));
    }

    fn set_book_marquee(book: &Entity<Self::Book>, marquee: bool, cx: &mut App) {
        book.update(cx, |book, book_cx| book.set_marquee(marquee, book_cx));
    }

    fn supports_detailed_cards() -> bool {
        true
    }

    fn set_book_detailed(book: &Entity<Self::Book>, detailed: bool, cx: &mut App) {
        book.update(cx, |book, book_cx| book.set_detailed(detailed, book_cx));
    }

    fn book_content_hash(book: &Entity<Self::Book>, cx: &App) -> Option<sync_common::ContentHash> {
        Some(book.read(cx).content_hash())
    }
    fn book_title(book: &Entity<Self::Book>, cx: &App) -> String {
        book.read(cx).title().to_owned()
    }

    fn path_to(&self, child: &BrowseRow) -> Result<Option<Vec<BrowseCrumb>>, String> {
        match self.kind {
            LibraryBrowseKind::Folder => Ok(None),
            LibraryBrowseKind::Subject => {
                let segments = child.id.split(" / ").filter(|segment| !segment.is_empty() && *segment != ROOT_SUBJECT_PATH).collect::<Vec<_>>();
                let mut location = String::new();
                let path = segments
                    .iter()
                    .enumerate()
                    .map(|(index, segment)| {
                        if !location.is_empty() {
                            location.push_str(" / ");
                        }
                        location.push_str(segment);
                        let label = if index + 1 == segments.len() { child.name.clone() } else { (*segment).to_owned() };
                        BrowseCrumb { location: location.clone(), label }
                    })
                    .collect();
                Ok(Some(path))
            }
        }
    }

    fn has_cached_book_detail(&self, content_hash: sync_common::ContentHash, cx: &gpui::App) -> bool {
        self.has_cached_book_detail(content_hash, cx)
    }

    fn prepare_book_detail(&mut self, book: library_model::BookCardRow, cover: Option<crate::library::widgets::LoadedCover>) {
        self.prepare_detail(book, cover);
    }

    fn clear_cached_book_detail(&mut self) {
        self.pending_detail = None;
        self.detail = None;
        self.detail_visible = false;
    }

    fn show_cached_book_detail(&mut self) {
        self.detail_visible = true;
    }

    fn hide_cached_book_detail(&mut self) {
        self.detail_visible = false;
    }
}

/// Opening a book's page is navigation, so it is the page that performs it —
/// the adapter only hands each card the means to ask. The handle is cloned
/// before the page exists, which is what `cx.entity()` inside a construction is
/// for.
fn open_detail(cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> OpenBookDetail {
    let page = cx.entity();
    Rc::new(move |book, cover, cx| {
        let crumb = BrowseCrumb { location: book_location(book.content_hash), label: book.title.clone() };
        page.update(cx, |page, cx| {
            page.adapter_mut().prepare_detail(book, cover);
            page.open_leaf(crumb, cx);
        });
    })
}

pub(crate) fn folder(ui: LibraryContext, cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> BrowsePage<LibraryBrowseAdapter> {
    let changes = ui.updates();
    let folder_actions = cx.new(|_| FolderActions::new(ui.clone(), ROOT_DIR_ID, None));
    let place_book_actions = folder_actions.clone();
    let place_book: PlaceBook = Rc::new(move |book, source, placement, cx| {
        let targets = FolderTargets { folders: Vec::new(), folder_parents: Default::default(), books: vec![book], source };
        place_book_actions.update(cx, |actions, cx| match placement {
            BookPlacement::Copy => actions.open_copy_targets(targets, cx),
            BookPlacement::Move => actions.open_move_targets(targets, cx),
        });
    });
    let remove_actions = folder_actions.clone();
    let remove_book: RemoveBook = Rc::new(move |book, source, description, window, cx| {
        let targets = FolderTargets { folders: Vec::new(), folder_parents: Default::default(), books: vec![book], source };
        remove_actions.update(cx, |actions, cx| actions.trash_targets(targets, description, window, cx));
    });
    let adapter = LibraryBrowseAdapter::new(ui, LibraryBrowseKind::Folder, open_detail(cx), Some(place_book), Some(remove_book));
    let config = BrowsePageConfig::new(ROOT_DIR_ID.to_string(), "Folders", "folder").with_changes(changes).with_folder_actions(folder_actions).with_split();
    BrowsePage::new(adapter, config, cx)
}

pub(crate) fn subject(ui: LibraryContext, cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> BrowsePage<LibraryBrowseAdapter> {
    let changes = ui.updates();
    let adapter = LibraryBrowseAdapter::new(ui, LibraryBrowseKind::Subject, open_detail(cx), None, None);
    let config = BrowsePageConfig::new(ROOT_SUBJECT_PATH, "Subjects", "subject").with_changes(changes).with_split();
    BrowsePage::new(adapter, config, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn a_book_is_a_location_that_only_a_book_can_be_read_out_of() {
        let content_hash: sync_common::ContentHash = HASH.parse().unwrap();
        assert_eq!(book_at_location(&book_location(content_hash)), Some(content_hash));

        assert_eq!(book_at_location(&ROOT_DIR_ID.to_string()), None, "a folder id is not a book");
        assert_eq!(book_at_location(ROOT_SUBJECT_PATH), None);
        assert_eq!(book_at_location("book:"), None, "the prefix alone names nothing");
        assert_eq!(book_at_location("book:not-a-hash"), None);
        assert_eq!(book_at_location(&HASH.to_uppercase()), None, "a bare hash is not a location either");
    }
}

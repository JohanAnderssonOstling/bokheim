//! Synchronous library data adapter for the shared browse page.

use std::rc::Rc;

use epub_svelte_lib::library::{LibraryFileTypeFilter, SubjectPath};
use epub_svelte_lib::{BackendError, BrowseContents, BrowseRow, DirId, ROOT_DIR_ID, ROOT_SUBJECT_PATH};
use gpui::{Context, Entity};

use super::browse_page::{BrowseAdapter, BrowseCrumb, BrowsePage, BrowseQuery, BrowseRequest};
use crate::library::services::LibraryContext;
use crate::library::widgets::{BookCardView, BookGrid};

type LoadContents = Rc<dyn Fn(&BrowseQuery) -> Result<BrowseContents, BackendError>>;

pub(crate) struct LibraryBrowseAdapter {
    load_contents: LoadContents,
    children: Rc<Vec<BrowseRow>>,
    books: BookGrid,
}

impl LibraryBrowseAdapter {
    pub(super) fn new(books: BookGrid, load_contents: impl Fn(&BrowseQuery) -> Result<BrowseContents, BackendError> + 'static) -> Self {
        Self { load_contents: Rc::new(load_contents), children: Rc::new(Vec::new()), books }
    }
}

impl BrowseAdapter for LibraryBrowseAdapter {
    type Response = BrowseContents;
    type Book = BookCardView;

    fn request(&self, query: &BrowseQuery) -> BrowseRequest<Self::Response> {
        BrowseRequest::Ready((self.load_contents)(query).map_err(|error| error.to_string()))
    }

    fn install(&mut self, response: Self::Response, cx: &mut Context<BrowsePage<Self>>) {
        self.children = Rc::new(response.children);
        self.books.set_books(response.books, cx);
    }

    fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }

    fn books(&self) -> Rc<Vec<(Entity<Self::Book>, usize)>> {
        self.books.virtual_spanned_views()
    }
}

pub(crate) fn folder(ui: LibraryContext, cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> BrowsePage<LibraryBrowseAdapter> {
    let backend = ui.library.clone();
    let changes = ui.contents_changes();
    let path = ui.folder_navigation_path().into_iter().map(|crumb| BrowseCrumb { location: crumb.id.to_string(), label: crumb.label }).collect();
    let persistence = ui.clone();
    let books = BookGrid::new(ui, true);
    let adapter = LibraryBrowseAdapter::new(books, move |query| backend.folder_contents(&query.location, &query.search, query.file_type.unwrap_or_default()));
    BrowsePage::new(
        adapter,
        ROOT_DIR_ID.to_string(),
        "Folders",
        path,
        "folder",
        Some(LibraryFileTypeFilter::All),
        Some(changes),
        move |path| {
            let persisted = path.iter().map(|crumb| DirId::parse_str(&crumb.location).map(|id| app_contract::NavigationCrumb { id, label: crumb.label.clone() })).collect::<Result<Vec<_>, _>>();
            match persisted {
                Ok(path) => persistence.persist_folder_navigation_path(path),
                Err(error) => log::warn!("failed to persist folder navigation: {error}"),
            }
        },
        cx,
    )
}

pub(crate) fn subject(ui: LibraryContext, cx: &mut Context<BrowsePage<LibraryBrowseAdapter>>) -> BrowsePage<LibraryBrowseAdapter> {
    let backend = ui.library.clone();
    let changes = ui.contents_changes();
    let path = ui.subject_navigation_path().into_iter().map(|crumb| BrowseCrumb { location: crumb.id.to_string(), label: crumb.label }).collect();
    let persistence = ui.clone();
    let books = BookGrid::new(ui, true);
    let adapter = LibraryBrowseAdapter::new(books, move |query| backend.subject_contents(&query.location, &query.search, query.file_type.unwrap_or_default()));
    BrowsePage::new(
        adapter,
        ROOT_SUBJECT_PATH.to_owned(),
        "Subjects",
        path,
        "subject",
        Some(LibraryFileTypeFilter::All),
        Some(changes),
        move |path| {
            persistence.persist_subject_navigation_path(path.iter().map(|crumb| app_contract::NavigationCrumb { id: SubjectPath::new(crumb.location.clone()), label: crumb.label.clone() }).collect());
        },
        cx,
    )
}

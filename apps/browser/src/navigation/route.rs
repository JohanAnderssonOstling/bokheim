//! Browser route definitions.

use std::cell::Cell;
use sync_common::LibraryId;
use ui_text::text;

use crate::library::BrowseKind;

/// A page displaying the selected library's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LibraryRoute {
    Home,
    Books,
    Authors,
    Folders,
    Subjects,
    Catalog,
    Trash,
    Notebook,
    Search,
}

impl LibraryRoute {
    pub(crate) fn browse_kind(self) -> Option<BrowseKind> {
        match self {
            Self::Home => Some(BrowseKind::Home),
            Self::Books => Some(BrowseKind::Books),
            Self::Folders => Some(BrowseKind::Folder),
            Self::Subjects => Some(BrowseKind::Subject),
            Self::Authors => Some(BrowseKind::Author),
            Self::Catalog => Some(BrowseKind::Catalog),
            Self::Trash | Self::Notebook | Self::Search => None,
        }
    }
}

/// A page shared across library selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GlobalRoute {
    Settings,
}

/// The currently displayed browser destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Route {
    Library(LibraryRoute),
    Global(GlobalRoute),
}

/// One pane's destination, independent of its library's cached page contents.
pub(super) struct PageNavigation {
    pub library_id: Cell<Option<LibraryId>>,
    // Retain the pane's library destination while a global page is open.
    library_route: Cell<LibraryRoute>,
    global_route: Cell<Option<GlobalRoute>>,
    library_view: Cell<LibraryRoute>,
}

impl PageNavigation {
    pub fn new(library_id: Option<LibraryId>) -> Self {
        Self { library_id: Cell::new(library_id), library_route: Cell::new(LibraryRoute::Home), global_route: Cell::new(None), library_view: Cell::new(LibraryRoute::Books) }
    }

    pub fn route(&self) -> Route {
        self.global_route.get().map(Route::Global).unwrap_or_else(|| Route::Library(self.library_route()))
    }

    pub fn library_route(&self) -> LibraryRoute {
        self.library_route.get()
    }

    pub fn library_view(&self) -> LibraryRoute {
        self.library_view.get()
    }

    pub fn navigate(&self, route: Route) {
        match route {
            Route::Library(route) => {
                self.library_route.set(route);
                if super::LIBRARY_VIEWS.contains(&route) {
                    self.library_view.set(route);
                }
                self.global_route.set(None);
            }
            Route::Global(route) => self.global_route.set(Some(route)),
        }
    }
}

impl Route {
    pub(crate) fn presentation(self) -> (&'static str, ui_components::NavigationIcon) {
        use ui_components::NavigationIcon;
        match self {
            Self::Library(LibraryRoute::Home) => (text!(browse::home), NavigationIcon::House),
            Self::Library(LibraryRoute::Books) => (text!(browse::books), NavigationIcon::BookOpen),
            Self::Library(LibraryRoute::Authors) => (text!(browse::authors), NavigationIcon::User),
            Self::Library(LibraryRoute::Folders) => (text!(browse::folders), NavigationIcon::Folder),
            Self::Library(LibraryRoute::Subjects) => (text!(browse::subjects), NavigationIcon::Tags),
            Self::Library(LibraryRoute::Catalog) => (text!(browse::catalog), NavigationIcon::Building2),
            Self::Library(LibraryRoute::Trash) => (text!(browse::trash), NavigationIcon::Trash2),
            Self::Library(LibraryRoute::Notebook) => (text!(common::notes), NavigationIcon::Notebook),
            Self::Library(LibraryRoute::Search) => (text!(common::search), NavigationIcon::Search),
            Self::Global(GlobalRoute::Settings) => (text!(common::settings), NavigationIcon::Settings),
        }
    }
}

impl From<LibraryRoute> for Route {
    fn from(route: LibraryRoute) -> Self {
        Self::Library(route)
    }
}

impl From<GlobalRoute> for Route {
    fn from(route: GlobalRoute) -> Self {
        Self::Global(route)
    }
}

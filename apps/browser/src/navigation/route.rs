//! Browser route definitions.

use ui_text::text;

use crate::library::BrowseKind;

/// A page whose state belongs to one library session.
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
    pub(crate) fn available_in_opds(self) -> bool {
        matches!(self, Self::Books | Self::Search | Self::Notebook)
    }

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

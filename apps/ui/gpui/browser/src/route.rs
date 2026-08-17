//! Browser routes, split by ownership scope.
//!
//! A [`LibraryRoute`] addresses a page whose state lives inside the selected
//! library and is discarded when that selection changes. A [`UniversalRoute`]
//! addresses a page that outlives library switches.
//!
//! Keeping these as separate types means a selected-library root can only be asked
//! for a route it actually owns, so route hosts are exhaustive matches rather
//! than fallible lookups.

/// Native names for the route identities owned by the shared frontend
/// contract. GPUI still owns route hosting; it no longer owns a duplicate list.
pub use app_contract::{LibraryFeature as LibraryRoute, UniversalFeature as UniversalRoute};

/// The currently displayed page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    Library(LibraryRoute),
    Universal(UniversalRoute),
}

impl Default for Route {
    fn default() -> Self {
        Self::Library(LibraryRoute::default())
    }
}

impl Route {
    /// The library route this addresses, if any. Universal routes leave the
    /// library page underneath untouched.
    pub(crate) const fn library(self) -> Option<LibraryRoute> {
        match self {
            Self::Library(route) => Some(route),
            Self::Universal(_) => None,
        }
    }

    pub(crate) const fn universal(self) -> Option<UniversalRoute> {
        match self {
            Self::Universal(route) => Some(route),
            Self::Library(_) => None,
        }
    }
}

impl From<LibraryRoute> for Route {
    fn from(route: LibraryRoute) -> Self {
        Self::Library(route)
    }
}

impl From<UniversalRoute> for Route {
    fn from(route: UniversalRoute) -> Self {
        Self::Universal(route)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universal_routes_carry_no_library_route() {
        assert_eq!(Route::Universal(UniversalRoute::Settings).library(), None);
    }

    #[test]
    fn library_routes_carry_no_universal_route() {
        assert_eq!(Route::Library(LibraryRoute::Home).universal(), None);
    }

    #[test]
    fn default_route_is_a_library_page() {
        assert_eq!(Route::default(), Route::Library(LibraryRoute::Folders));
    }

    #[test]
    fn legacy_sessions_naming_a_universal_section_fall_back() {
        // Saved sessions from before the split may hold "settings" where a
        // library route is now expected; the field default must absorb that
        // rather than the whole session failing to load.
        assert!(serde_json::from_str::<LibraryRoute>("\"settings\"").is_err());
        assert_eq!(LibraryRoute::default(), LibraryRoute::Folders);
    }
}

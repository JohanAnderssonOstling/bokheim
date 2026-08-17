//! Presentation records shared across pages.

#[cfg(test)]
use crate::route::LibraryRoute;
use epub_svelte_lib::LibraryId;

pub use epub_svelte_lib::library::LibraryFolderEntry as FolderCrumb;
pub use epub_svelte_lib::library::LibrarySubjectEntry as SubjectCrumb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AccountFlow {
    #[default]
    SignIn,
    Register,
    VerifyEmail,
    RequestPasswordReset,
    ResetPassword,
}

/// Where browsing was left off inside one library.
///
/// Held per library rather than once globally: switching library and switching
/// back is the primary flow of a multi-library app, and a single slot means the
/// library you return to has forgotten where you were.
pub type LibraryPlace = app_contract::LibraryPlace<FolderCrumb, SubjectCrumb>;

/// Where the browser reopens after a restart.
///
/// Only library routes are recorded: Store, Sync and Settings are places you
/// visit and leave, so quitting on one restores the library page underneath
/// instead.
///
/// Sessions written before this became per-library simply lose their browse
/// position — the old flat fields are unknown to serde now and are ignored,
/// which leaves the rest of the session (reader preferences, window geometry)
/// intact. That is why `SESSION_VERSION` is deliberately not bumped for this.
pub type BrowserSession = app_contract::BrowserSession<LibraryId, FolderCrumb, SubjectCrumb>;

/// Forgets libraries that are no longer in the catalogue, so the serialized map
/// does not accumulate entries for libraries that were removed.
pub(crate) fn retain_library_places(places: &mut std::collections::HashMap<LibraryId, LibraryPlace>, present: impl Fn(&LibraryId) -> bool) {
    places.retain(|library_id, _| present(library_id));
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrowserWindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(seed: u128) -> LibraryId {
        LibraryId::from_u128(seed)
    }

    #[test]
    fn each_library_keeps_its_own_place() {
        let mut session = BrowserSession::default();
        session.places.insert(library(1), LibraryPlace { route: LibraryRoute::Authors, ..LibraryPlace::default() });
        session.places.insert(library(2), LibraryPlace { route: LibraryRoute::Subjects, ..LibraryPlace::default() });

        let encoded = serde_json::to_string(&session).expect("sessions serialize");
        let decoded: BrowserSession = serde_json::from_str(&encoded).expect("sessions deserialize");

        assert_eq!(decoded.place(&library(1)).route, LibraryRoute::Authors);
        assert_eq!(decoded.place(&library(2)).route, LibraryRoute::Subjects);
    }

    #[test]
    fn a_library_never_visited_falls_back_to_the_default_place() {
        let session = BrowserSession::default();
        assert_eq!(session.place(&library(9)), LibraryPlace::default());
        assert_eq!(LibraryPlace::default().route, LibraryRoute::Folders);
    }

    /// The old flat `library_route`/`folder_path`/`subject_path` fields are
    /// gone. Serde ignores unknown fields, so such a session still loads and
    /// only the browse position is lost — the alternative, a hard error, would
    /// discard the reader preferences and window geometry alongside it.
    #[test]
    fn a_session_written_before_the_split_still_loads() {
        let session: BrowserSession = serde_json::from_str(r#"{"active_library_id":"00000000-0000-0000-0000-000000000001","library_route":"authors","folder_path":[],"subject_path":[]}"#).expect("an older session must not be discarded");
        assert_eq!(session.active_library_id, Some(library(1)));
        assert!(session.places.is_empty());
    }

    #[test]
    fn removing_a_library_forgets_its_place() {
        let mut session = BrowserSession::default();
        session.places.insert(library(1), LibraryPlace::default());
        session.places.insert(library(2), LibraryPlace::default());

        retain_library_places(&mut session.places, |id| *id == library(1));

        assert!(session.places.contains_key(&library(1)));
        assert!(!session.places.contains_key(&library(2)), "a removed library must not linger in the serialized session");
    }

    #[test]
    fn every_library_route_survives_a_round_trip() {
        for route in LibraryRoute::ALL {
            let mut session = BrowserSession::default();
            session.places.insert(library(1), LibraryPlace { route: *route, ..LibraryPlace::default() });
            let encoded = serde_json::to_string(&session).expect("sessions serialize");
            let decoded: BrowserSession = serde_json::from_str(&encoded).expect("sessions deserialize");
            assert_eq!(decoded.place(&library(1)).route, *route, "{route:?} did not round-trip");
        }
    }
}

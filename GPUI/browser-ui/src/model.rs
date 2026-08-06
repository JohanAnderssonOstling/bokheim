//! Presentation records shared across pages.

use crate::route::LibraryRoute;

pub use epub_svelte_lib::library::LibraryFolderEntry as FolderCrumb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AccountFlow {
    #[default]
    SignIn,
    Register,
    VerifyEmail,
    RequestPasswordReset,
    ResetPassword,
}

/// Where the browser reopens after a restart.
///
/// Only library routes are recorded: Settings and Sync are places you visit and
/// leave, so quitting on one restores the library page underneath instead.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BrowserSession {
    pub active_library_id: Option<String>,
    /// Named `section` before the universal/library split, when it could also
    /// hold `store`, `sync` or `settings`.
    ///
    /// `#[serde(default)]` alone is not enough: it covers a *missing* field,
    /// but an unknown variant is a hard error that would discard the entire
    /// session. The custom deserializer maps anything unrecognized — including
    /// the universal routes — onto the default instead.
    #[serde(default, alias = "section", deserialize_with = "library_route_or_default")]
    pub library_route: LibraryRoute,
    #[serde(default)]
    pub folder_path: Vec<FolderCrumb>,
}

/// Maps a stored route name onto a [`LibraryRoute`], falling back to the
/// default for universal routes and anything else unrecognized.
///
/// The names are spelled out rather than delegated to the derived
/// `Deserialize`, because that is precisely what errors on an unknown variant.
/// [`every_library_route_survives_a_round_trip`] guards the duplication.
///
/// [`every_library_route_survives_a_round_trip`]: tests::every_library_route_survives_a_round_trip
fn library_route_or_default<'de, D>(deserializer: D) -> Result<LibraryRoute, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw = String::deserialize(deserializer)?;
    Ok(match raw.as_str() {
        "home" => LibraryRoute::Home,
        "authors" => LibraryRoute::Authors,
        "folders" => LibraryRoute::Folders,
        "search" => LibraryRoute::Search,
        _ => LibraryRoute::default(),
    })
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

    #[test]
    fn sessions_recorded_before_the_split_still_load() {
        // Older sessions stored a `section` that could name a universal route.
        let session: BrowserSession = serde_json::from_str(r#"{"active_library_id":"lib","section":"settings"}"#).expect("an unknown route must not drop the whole session");
        assert_eq!(session.library_route, LibraryRoute::Folders);
        assert_eq!(session.active_library_id.as_deref(), Some("lib"));
    }

    #[test]
    fn a_recorded_library_route_round_trips() {
        let session: BrowserSession = serde_json::from_str(r#"{"active_library_id":"lib","library_route":"home"}"#).expect("library routes deserialize");
        assert_eq!(session.library_route, LibraryRoute::Home);
    }

    #[test]
    fn a_missing_route_falls_back_without_losing_the_rest() {
        let session: BrowserSession = serde_json::from_str(r#"{"active_library_id":"lib"}"#).expect("an absent route is allowed");
        assert_eq!(session.library_route, LibraryRoute::Folders);
    }

    /// Guards the hand-written name mapping in `library_route_or_default`
    /// against drifting from the serde representation it shadows.
    #[test]
    fn every_library_route_survives_a_round_trip() {
        for route in LibraryRoute::ALL {
            let encoded = serde_json::to_string(route).expect("routes serialize");
            let session: BrowserSession = serde_json::from_str(&format!(r#"{{"library_route":{encoded}}}"#)).expect("routes deserialize");
            assert_eq!(session.library_route, *route, "{encoded} did not round-trip");
        }
    }

    #[test]
    fn a_universal_route_recorded_by_an_older_build_is_not_fatal() {
        for stored in ["store", "sync", "settings", "something-else-entirely"] {
            let session: BrowserSession = serde_json::from_str(&format!(r#"{{"active_library_id":"lib","section":"{stored}"}}"#)).expect("unknown routes must not discard the session");
            assert_eq!(session.library_route, LibraryRoute::Folders);
            assert_eq!(session.active_library_id.as_deref(), Some("lib"));
        }
    }
}

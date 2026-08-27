//! Device-local browser navigation state.

use std::collections::HashMap;
use std::io;

use crate::{DirId, LibraryId};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NavigationCrumb<Id> {
    pub id: Id,
    #[serde(alias = "name")]
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryPlace {
    #[serde(default)]
    pub folder_path: Vec<NavigationCrumb<DirId>>,
    #[serde(default)]
    pub subject_path: Vec<NavigationCrumb<library_model::SubjectPath>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BrowserSession {
    pub active_library_id: Option<LibraryId>,
    #[serde(default)]
    pub places: HashMap<LibraryId, LibraryPlace>,
}

impl BrowserSession {
    pub fn place(&self, library_id: &LibraryId) -> LibraryPlace {
        self.places.get(library_id).cloned().unwrap_or_default()
    }

    pub fn retain_places(&mut self, present: impl Fn(&LibraryId) -> bool) {
        self.places.retain(|library_id, _| present(library_id));
    }
}

const BROWSER_SESSION_FILE: &str = "browser_session.json";

impl crate::app::BackendContext {
    pub(crate) fn load_browser_session(&self) -> io::Result<BrowserSession> {
        self.read_platform_value(BROWSER_SESSION_FILE)?.map_or_else(|| Ok(BrowserSession::default()), |bytes| serde_json::from_slice(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)))
    }

    pub(crate) fn save_browser_session(&self, session: &BrowserSession) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(session).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.write_platform_value(BROWSER_SESSION_FILE, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppDataLocation;

    #[test]
    fn missing_session_uses_defaults_and_saved_session_round_trips() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        assert_eq!(context.load_browser_session().unwrap(), BrowserSession::default());

        let mut session = BrowserSession::default();
        session.active_library_id = Some(crate::LibraryId::from_u128(7));
        context.save_browser_session(&session).unwrap();

        assert_eq!(context.load_browser_session().unwrap(), session);
    }

    #[test]
    fn persisted_navigation_crumb_reads_the_previous_hierarchy_row_shape() {
        let crumb: NavigationCrumb<String> = serde_json::from_value(serde_json::json!({
            "id": "child",
            "name": "Child",
            "book_count": 42,
            "downloaded_book_count": 7
        }))
        .unwrap();

        assert_eq!(crumb, NavigationCrumb { id: "child".to_owned(), label: "Child".to_owned() });
        assert_eq!(serde_json::to_value(crumb).unwrap(), serde_json::json!({ "id": "child", "label": "Child" }));
    }
}
